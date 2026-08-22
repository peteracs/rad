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


def run_checked(argv: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(argv, cwd=ROOT, text=True, capture_output=True)


def run_manifest_command(
    rad: Path, command: dict[str, object]
) -> tuple[subprocess.CompletedProcess[str], str]:
    setup = command.get("setup")
    if setup:
        setup_argv = [str(value) for value in setup]
        if setup_argv[0] == "cargo" and "-j" not in setup_argv and "--jobs" not in setup_argv:
            raise RuntimeError("Rust setup commands must pin a single compiler job")
        prepared = run_checked(setup_argv)
        if prepared.returncode:
            return prepared, "setup"

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
        completed = subprocess.run(
            [str(rad)] + [str(value) for value in command.get("args", [])],
            cwd=ROOT,
            text=True,
            input=command.get("stdin"),
            capture_output=True,
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
    return completed, "run"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True, type=Path)
    args = parser.parse_args()
    rad = args.rad.resolve()
    failures: list[str] = []

    generated = run_checked(
        [sys.executable, str(ROOT / "tooling/language_surface.py"), "--check"]
    )
    if generated.returncode:
        failures.append(generated.stdout.strip() or generated.stderr.strip())

    coverage_generated = run_checked(
        [
            sys.executable,
            str(ROOT / "tooling/generate_language_coverage.py"),
            "--rad",
            str(rad),
            "--check",
        ]
    )
    if coverage_generated.returncode:
        failures.append(
            coverage_generated.stdout.strip() or coverage_generated.stderr.strip()
        )

    surface = json.loads(SURFACE.read_text(encoding="utf-8"))
    manifest = tomllib.loads(COVERAGE.read_text(encoding="utf-8"))
    if manifest.get("surface_owner_digest") != surface["ownerDigest"]:
        failures.append("coverage manifest belongs to a different compiler surface digest")
    if manifest.get("evidence_kind") != "compiler-ast-and-token-report":
        failures.append("coverage evidence must come from compiler AST/token reports")

    rows = manifest.get("coverage", [])
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

    reports: dict[str, dict[str, object]] = {}
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

        if source not in reports:
            completed = run_checked([str(rad), "surface", source, "--json"])
            if completed.returncode:
                failures.append(
                    f"rad surface failed for {source}:\n{completed.stdout}{completed.stderr}"
                )
                reports[source] = {}
            else:
                reports[source] = json.loads(completed.stdout)
        report_key = GROUPS[prefix][1]
        if name not in reports[source].get(report_key, []):
            failures.append(
                f"compiler report does not observe {identifier} in assigned source {source}"
            )

    builtin_names = [entry["name"] for entry in surface["builtins"]]
    builtin_variants = {entry["name"]: entry["variant"] for entry in surface["builtins"]}
    builtin_rows = manifest.get("builtin_coverage", [])
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
        if source not in reports:
            completed = run_checked([str(rad), "surface", source, "--json"])
            if completed.returncode:
                failures.append(
                    f"rad surface failed for builtin evidence {source}:\n"
                    f"{completed.stdout}{completed.stderr}"
                )
                reports[source] = {}
            else:
                reports[source] = json.loads(completed.stdout)
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

    commands = manifest.get("command", [])
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
    for command in commands:
        argv = [str(rad)] + command.get("args", [])
        try:
            completed, stage = run_manifest_command(rad, command)
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

    if failures:
        print("language surface gate: FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print(
        "language surface gate: PASS - "
        f"{len(rows)} executable syntax rows, {len(exclusions)} internal exclusions, "
        f"{len(builtin_rows)} executable builtin rows/API entries"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
