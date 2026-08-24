#!/usr/bin/env python3
"""Fail when compiler syntax, normative docs, and Sovereign Grid diverge."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
import tomllib
from collections import Counter
from concurrent.futures import Future, ThreadPoolExecutor
from functools import lru_cache
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SURFACE = ROOT / "docs/language-surface.json"
COVERAGE = ROOT / "projects/dogfood/sovereign-grid/coverage.toml"
BUILTIN_API = ROOT / "docs/src/reference/generated/builtin-api.md"
INTERNAL_ONLY = {
    "token.Error",
    "token.Eof",
    "decl.Error",
    "stmt.OnceGuardPass",
    "stmt.Error",
    "expr.Error",
}
GROUPS = {
    "token": ("tokens", "tokens"),
    "decl": ("declarations", "declarations"),
    "stmt": ("statements", "statements"),
    "expr": ("expressions", "expressions"),
    "pattern": ("patterns", "patterns"),
    "binop": ("binaryOperators", "binaryOperators"),
    "unop": ("unaryOperators", "unaryOperators"),
    "typeexpr": ("typeExpressions", "typeExpressions"),
    "fnpurity": ("functionTypePurities", "functionTypePurities"),
    "contract": ("callableContracts", "callableContracts"),
}
REQUIRED_ROW_FIELDS = {
    "id",
    "grammar_production",
    "parser_owner",
    "normative_doc_anchor",
    "positive_source",
    "positive_command",
    "positive_assertion",
    "conformance_test",
    "sovereign_grid_source",
    "stability",
}
REQUIRED_BUILTIN_ROW_FIELDS = {
    "name",
    "runtime_variant",
    "api_anchor",
    "source",
    "command",
    "assertion",
    "execution",
    "stability",
}


def surface_ids(surface: dict[str, object]) -> set[str]:
    return {
        f"{prefix}.{name}"
        for prefix, (surface_key, _) in GROUPS.items()
        for name in surface[surface_key]
    }


@lru_cache(maxsize=None)
def markdown_has_anchor(path: Path, anchor: str) -> bool:
    text = path.read_text(encoding="utf-8")
    if f'id="{anchor}"' in text or f"id='{anchor}'" in text:
        return True
    for heading in re.findall(r"^#{1,6}\s+(.+?)\s*$", text, re.MULTILINE):
        slug = heading.lower().strip()
        slug = re.sub(r"[`*_]", "", slug)
        slug = re.sub(r"[^a-z0-9 _-]", "", slug)
        slug = re.sub(r"[ _]+", "-", slug).strip("-")
        if anchor == slug:
            return True
    return False


RAD_LIMIT_NS = 1_000_000_000


def run_checked(argv: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(argv, cwd=ROOT, text=True, capture_output=True)


def run_rad_checked(argv: list[str]) -> tuple[subprocess.CompletedProcess[str], int]:
    started = time.perf_counter_ns()
    completed = subprocess.run(
        argv,
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=RAD_LIMIT_NS / 1_000_000_000,
    )
    elapsed_ns = time.perf_counter_ns() - started
    if elapsed_ns >= RAD_LIMIT_NS:
        raise RuntimeError(
            f"RAD operation exceeded hard limit: elapsedNs={elapsed_ns} limitNs={RAD_LIMIT_NS}"
        )
    return completed, elapsed_ns


def run_manifest_command(
    rad: Path, command: dict[str, object]
) -> tuple[subprocess.CompletedProcess[str], str, int]:
    setup = command.get("setup")
    if setup:
        setup_argv = [str(value) for value in setup]
        if setup_argv[0] == "cargo" and "-j" not in setup_argv and "--jobs" not in setup_argv:
            raise RuntimeError("Rust setup commands must pin a single compiler job")
        prepared = run_checked(setup_argv)
        if prepared.returncode:
            return prepared, "setup", 0

    harness = command.get("harness")
    process: subprocess.Popen[str] | None = None
    ready = ROOT / "target/sovereign-grid-surface-harness.ready"
    stop = ROOT / "target/sovereign-grid-surface-harness.stop"
    if harness:
        if harness != "sovereign-grid-protocol":
            raise RuntimeError(f"unknown language-surface harness: {harness}")
        ready.unlink(missing_ok=True)
        stop.unlink(missing_ok=True)
        process = subprocess.Popen(
            [
                sys.executable,
                str(ROOT / "tooling/sovereign_grid_protocol_harness.py"),
                "--ready",
                str(ready),
                "--stop",
                str(stop),
            ],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        deadline = time.monotonic() + 10.0
        while not ready.exists() and process.poll() is None and time.monotonic() < deadline:
            time.sleep(0.02)
        if not ready.exists():
            output = process.communicate(timeout=2.0)[0]
            raise RuntimeError(f"protocol harness did not become ready:\n{output}")

    try:
        started = time.perf_counter_ns()
        completed = subprocess.run(
            [str(rad)] + [str(value) for value in command.get("args", [])],
            cwd=ROOT,
            text=True,
            input=command.get("stdin"),
            capture_output=True,
            timeout=RAD_LIMIT_NS / 1_000_000_000,
        )
        elapsed_ns = time.perf_counter_ns() - started
        if elapsed_ns >= RAD_LIMIT_NS:
            raise RuntimeError(
                f"RAD operation exceeded hard limit: elapsedNs={elapsed_ns} limitNs={RAD_LIMIT_NS}"
            )
    finally:
        if process is not None:
            stop.write_text("stop\n", encoding="utf-8")
            try:
                process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=2.0)
            ready.unlink(missing_ok=True)
            stop.unlink(missing_ok=True)
    return completed, "run", elapsed_ns


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True, type=Path)
    args = parser.parse_args()
    rad = args.rad.resolve()
    failures: list[str] = []
    rad_timings: list[dict[str, object]] = []

    surface = json.loads(SURFACE.read_text(encoding="utf-8"))
    manifest = tomllib.loads(COVERAGE.read_text(encoding="utf-8"))
    if manifest.get("surface_owner_digest") != surface["ownerDigest"]:
        failures.append("coverage manifest belongs to a different compiler surface digest")
    if manifest.get("evidence_kind") != "compiler-ast-and-token-report":
        failures.append("coverage evidence must come from compiler AST/token reports")

    rows = manifest.get("coverage", [])
    builtin_rows = manifest.get("builtin_coverage", [])
    commands = manifest.get("command", [])
    exclusions = manifest.get("exclusion", [])
    row_ids = [row.get("id", "") for row in rows]
    excluded_ids = [row.get("id", "") for row in exclusions]
    duplicates = sorted(
        name
        for name, count in Counter(row_ids + excluded_ids).items()
        if count != 1
    )
    if duplicates:
        failures.append(f"duplicate surface rows: {', '.join(duplicates)}")

    expected = surface_ids(surface)
    represented = set(row_ids) | set(excluded_ids)
    missing = sorted(expected - represented)
    stale = sorted(represented - expected)
    if missing:
        failures.append(f"uncovered compiler surface: {', '.join(missing)}")
    if stale:
        failures.append(f"stale coverage rows: {', '.join(stale)}")
    if set(excluded_ids) != INTERNAL_ONLY:
        failures.append(
            "internal exclusion set differs from the compiler-owned recovery/sentinel set"
        )
    for exclusion in exclusions:
        if len(exclusion.get("reason", "").strip()) < 20:
            failures.append(f"exclusion needs a concrete reason: {exclusion.get('id', '')}")

    report_sources = sorted(
        {
            source
            for row in [*rows, *builtin_rows]
            if isinstance(
                source := row.get("positive_source", row.get("source")), str
            )
            and (ROOT / source).is_file()
        }
    )
    worker_count = max(1, min(8, len(report_sources) + len(commands)))
    executor = ThreadPoolExecutor(max_workers=worker_count)
    command_submission_order = sorted(
        enumerate(commands),
        key=lambda item: (
            0
            if item[1].get("setup")
            else 1
            if item[1].get("harness")
            else 2,
            item[0],
        ),
    )
    command_futures = {
        index: executor.submit(run_manifest_command, rad, command)
        for index, command in command_submission_order
    }
    report_futures: dict[str, Future[tuple[subprocess.CompletedProcess[str], int]]] = {
        source: executor.submit(
            run_rad_checked, [str(rad), "surface", source, "--json"]
        )
        for source in report_sources
    }

    reports: dict[str, dict[str, object]] = {}
    for source in report_sources:
        try:
            completed, elapsed_ns = report_futures[source].result()
            rad_timings.append(
                {
                    "name": f"surface:{source}",
                    "elapsedNs": elapsed_ns,
                    "limitNs": RAD_LIMIT_NS,
                }
            )
        except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
            failures.append(f"rad surface could not run for {source}: {error}")
            reports[source] = {}
            continue
        if completed.returncode:
            failures.append(
                f"rad surface failed for {source}:\n{completed.stdout}{completed.stderr}"
            )
            reports[source] = {}
        else:
            reports[source] = json.loads(completed.stdout)

    for row in rows:
        identifier = row.get("id", "")
        missing_fields = sorted(REQUIRED_ROW_FIELDS - row.keys())
        if missing_fields:
            failures.append(f"coverage row {identifier} lacks: {', '.join(missing_fields)}")
            continue
        prefix, separator, name = identifier.partition(".")
        if not separator or prefix not in GROUPS:
            failures.append(f"invalid surface ID: {identifier}")
            continue
        source = row["positive_source"]
        if row["sovereign_grid_source"] != source:
            failures.append(f"row {identifier} has two different positive source owners")
        source_path = ROOT / source
        if not source_path.is_file():
            failures.append(f"coverage evidence missing: {identifier} -> {source}")
            continue
        for field in ("parser_owner", "conformance_test"):
            if not (ROOT / row[field]).is_file():
                failures.append(f"row {identifier} has missing {field}: {row[field]}")
        doc_path_text, doc_separator, anchor = row["normative_doc_anchor"].partition("#")
        doc_path = ROOT / doc_path_text
        if not doc_separator or not anchor:
            failures.append(f"documentation link needs an anchor: {identifier}")
        elif not doc_path.is_file():
            failures.append(f"documentation file missing: {identifier} -> {doc_path_text}")
        elif not markdown_has_anchor(doc_path, anchor):
            failures.append(f"documentation anchor missing: {identifier} -> {anchor}")
        if row["stability"] not in {"stable", "experimental"}:
            failures.append(f"row {identifier} has invalid stability {row['stability']!r}")

        report_key = GROUPS[prefix][1]
        if name not in reports[source].get(report_key, []):
            failures.append(
                f"compiler report does not observe {identifier} in assigned source {source}"
            )

    builtin_names = [entry["name"] for entry in surface["builtins"]]
    builtin_variants = {entry["name"]: entry["variant"] for entry in surface["builtins"]}
    if [row.get("name", "") for row in builtin_rows] != builtin_names:
        failures.append("builtin coverage rows differ from Builtin::ALL order or names")
    for row in builtin_rows:
        name = row.get("name", "")
        missing_fields = sorted(REQUIRED_BUILTIN_ROW_FIELDS - row.keys())
        if missing_fields:
            failures.append(f"builtin coverage {name} lacks: {', '.join(missing_fields)}")
            continue
        if row["runtime_variant"] != builtin_variants.get(name):
            failures.append(f"builtin coverage {name} has a stale runtime variant")
        source = row["source"]
        source_path = ROOT / source
        if not source_path.is_file():
            failures.append(f"builtin evidence missing: {name} -> {source}")
            continue
        if name not in reports[source].get("builtins", []):
            failures.append(f"compiler report does not observe builtin {name} in {source}")
        api_path_text, separator, anchor = row["api_anchor"].partition("#")
        api_path = ROOT / api_path_text
        if not separator or not api_path.is_file() or not markdown_has_anchor(api_path, anchor):
            failures.append(f"builtin {name} has invalid API anchor {row['api_anchor']}")
        if row["stability"] not in {"stable", "experimental"}:
            failures.append(f"builtin {name} has invalid stability {row['stability']!r}")

    builtin_text = BUILTIN_API.read_text(encoding="utf-8")
    documented_builtins = re.findall(r'<a id="([a-z0-9_]+)"></a>', builtin_text)
    if documented_builtins != builtin_names:
        failures.append("generated builtin API entries differ from Builtin::ALL order or names")
    api_fields = (
        "Category",
        "Arity",
        "Effects",
        "Purity",
        "Errors",
        "Determinism",
        "Complexity",
        "Allocation",
        "Native",
        "WASM",
        "Sandbox",
        "Transaction body",
        "Post-commit",
        "Settlement",
    )
    sections = re.split(r'(?=<a id="[a-z0-9_]+"></a>)', builtin_text)[1:]
    if len(sections) == len(builtin_names):
        evidence_by_name = {row["name"]: row for row in builtin_rows}
        for name, section in zip(builtin_names, sections, strict=True):
            for field in api_fields:
                if f"- {field}:" not in section:
                    failures.append(f"builtin API {name} lacks {field}")
            signature = re.search(r"```text\n(.+?)\n```", section, re.DOTALL)
            if signature is None or "..." in signature.group(1):
                failures.append(f"builtin API {name} lacks an exact canonical signature")
            evidence = evidence_by_name[name]
            if evidence["source"].replace("\\", "/") not in section:
                failures.append(f"builtin API {name} lacks its executable evidence link")
            if evidence["command"] not in section:
                failures.append(f"builtin API {name} lacks its verification command")

    project_root = ROOT / "projects/dogfood/sovereign-grid"
    project_text = "\n".join(
        path.read_text(encoding="utf-8") for path in project_root.rglob("*.rad")
    ).lower()
    for forbidden in ("compatibility", "deprecated", "legacy", "fallback"):
        if re.search(rf"\b{forbidden}\b", project_text):
            failures.append(f"canonical dogfood contains forbidden term: {forbidden}")

    command_strings = {
        "rad " + " ".join(command.get("args", [])) for command in commands
    }
    for row in rows:
        if row.get("positive_command") not in command_strings:
            failures.append(
                f"row {row.get('id', '')} references an unregistered positive command"
            )
    for row in builtin_rows:
        if row.get("command") not in command_strings:
            failures.append(
                f"builtin {row.get('name', '')} references an unregistered command"
            )
    for index, command in enumerate(commands):
        argv = [str(rad)] + command.get("args", [])
        try:
            completed, stage, elapsed_ns = command_futures[index].result()
            if stage == "run":
                rad_timings.append(
                    {
                        "name": str(command.get("name", argv)),
                        "elapsedNs": elapsed_ns,
                        "limitNs": RAD_LIMIT_NS,
                    }
                )
        except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
            failures.append(f"command {command.get('name', argv)!r} could not run: {error}")
            continue
        expected_exit = int(command.get("exit", 0))
        output = completed.stdout + completed.stderr
        if completed.returncode != expected_exit:
            failures.append(
                f"command {command.get('name', argv)!r} {stage} exited {completed.returncode}, "
                f"expected {expected_exit}:\n{output}"
            )
        expected_text = command.get("contains", "")
        if expected_text and expected_text not in output:
            failures.append(
                f"command {command.get('name', argv)!r} omitted {expected_text!r}"
            )

    executor.shutdown(wait=True)

    if failures:
        print("language surface gate: FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1
    timing_path = ROOT / "target/language-surface-subsecond.json"
    timing_path.parent.mkdir(parents=True, exist_ok=True)
    timing_path.write_text(
        json.dumps(
            {
                "formatVersion": 1,
                "limitNs": RAD_LIMIT_NS,
                "operations": rad_timings,
                "maximumElapsedNs": max(
                    (int(entry["elapsedNs"]) for entry in rad_timings), default=0
                ),
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    print(
        "language surface gate: PASS - "
        f"{len(rows)} executable syntax rows, {len(exclusions)} internal exclusions, "
        f"{len(builtin_rows)} executable builtin rows/API entries"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
