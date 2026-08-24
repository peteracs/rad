#!/usr/bin/env python3
"""Generate Sovereign Grid's explicit compiler-surface evidence manifest.

The denominator comes from docs/language-surface.json. The numerator comes
from `rad surface --json`, which walks the compiler AST and lexer output; this
generator never searches source text for hand-written coverage markers.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SURFACE = ROOT / "docs/language-surface.json"
OUTPUT = ROOT / "projects/dogfood/sovereign-grid/coverage.toml"
BUILTIN_API = ROOT / "docs/src/reference/generated/builtin-api.md"
BUILTIN_INDEX = ROOT / "docs/src/reference/generated/builtin-index.md"
SYNTAX_INDEX = ROOT / "docs/src/reference/generated/syntax-rules.md"
REPOSITORY_BLOB = "https://github.com/peteracs/rad/blob/main/"
CATEGORY_ORDER = [
    "values-control",
    "collections",
    "text-json",
    "buffers-bitsets",
    "host-publications",
    "native-layout",
    "ecs-resources",
    "queries-indexes-views",
    "events-phases",
    "provenance",
    "transactions-speculation",
    "persistence-wire-replay",
    "testing-models",
    "host-io-network",
    "relations",
    "ffi",
]

GROUPS = {
    "token": "tokens",
    "decl": "declarations",
    "stmt": "statements",
    "expr": "expressions",
    "pattern": "patterns",
    "binop": "binaryOperators",
    "unop": "unaryOperators",
    "typeexpr": "typeExpressions",
    "fnpurity": "functionTypePurities",
    "contract": "callableContracts",
}

INTERNAL = {
    "token.Error": "lexer recovery token; no valid source program can request it",
    "token.Eof": "lexer sentinel inserted after source input, not source syntax",
    "decl.Error": "parser recovery declaration, never a valid source declaration",
    "stmt.OnceGuardPass": "compiler-inserted exactly-once guard node, not source syntax",
    "stmt.Error": "parser recovery statement, never a valid source statement",
    "expr.Error": "parser recovery expression, never a valid source expression",
}

EVIDENCE = [
    {
        "name": "stable-main",
        "source": "projects/dogfood/sovereign-grid/main.rad",
        "command": [
            "projects/dogfood/sovereign-grid/main.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "assertion": "sovereign-grid: stable workflow complete",
        "stability": "stable",
    },
    {
        "name": "workflow-tests",
        "source": "projects/dogfood/sovereign-grid/tests/workflow.rad",
        "command": ["test", "projects/dogfood/sovereign-grid/tests"],
        "assertion": "Results: 10 passed, 0 failed, 10 total",
        "stability": "stable",
    },
    {
        "name": "shared-world-tests",
        "source": "projects/dogfood/sovereign-grid/tests/shared_world.rad",
        "command": ["test", "projects/dogfood/sovereign-grid/tests"],
        "assertion": "Results: 10 passed, 0 failed, 10 total",
        "stability": "stable",
    },
    {
        "name": "model-tests",
        "source": "projects/dogfood/sovereign-grid/tests/grid_model.rad",
        "command": ["test", "projects/dogfood/sovereign-grid/tests"],
        "assertion": "PASS  grid_model.rad :: model_GridLifecycleModel",
        "stability": "stable",
    },
    {
        "name": "causal-settlement",
        "source": "projects/dogfood/sovereign-grid/experimental/causal_dispatch.rad",
        "command": [
            "projects/dogfood/sovereign-grid/experimental/causal_dispatch.rad",
            "--experimental-laws",
            "--strict-types",
            "--deny-warnings",
        ],
        "assertion": "sovereign-grid: causal settlement complete",
        "stability": "experimental",
    },
    {
        "name": "relation-builtins",
        "source": "projects/dogfood/sovereign-grid/experimental/relation_builtins.rad",
        "command": [
            "projects/dogfood/sovereign-grid/experimental/relation_builtins.rad",
            "--experimental-laws",
            "--relation-schema",
            "projects/dogfood/sovereign-grid/experimental/relation_runtime.rad",
            "--relation-module",
            "sovereign::surface",
            "--experimental-relations",
            "--strict-types",
            "--deny-warnings",
        ],
        "assertion": "sovereign-grid: relation builtins complete",
        "stability": "experimental",
    },
    {
        "name": "value-collection-text-builtins",
        "source": "projects/dogfood/sovereign-grid/builtins/values_collections_text.rad",
        "command": [
            "projects/dogfood/sovereign-grid/builtins/values_collections_text.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "assertion": "sovereign-grid: values collections text builtins complete",
        "stability": "stable",
    },
    {
        "name": "ecs-query-provenance-builtins",
        "source": "projects/dogfood/sovereign-grid/builtins/ecs_queries_provenance.rad",
        "command": [
            "projects/dogfood/sovereign-grid/builtins/ecs_queries_provenance.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "assertion": "sovereign-grid: ECS query provenance builtins complete",
        "stability": "stable",
    },
    {
        "name": "speculation-persistence-builtins",
        "source": "projects/dogfood/sovereign-grid/builtins/speculation_persistence.rad",
        "command": [
            "projects/dogfood/sovereign-grid/builtins/speculation_persistence.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "assertion": "sovereign-grid: speculation persistence builtins complete",
        "stability": "stable",
    },
    {
        "name": "sandbox-guest-builtins",
        "source": "projects/dogfood/sovereign-grid/builtins/sandbox_guest.rad",
        "command": [
            "projects/dogfood/sovereign-grid/builtins/speculation_persistence.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "assertion": "sovereign-grid: speculation persistence builtins complete",
        "execution": "sandboxed-by-speculation-persistence",
        "stability": "stable",
    },
    {
        "name": "host-io-builtins",
        "source": "projects/dogfood/sovereign-grid/builtins/host_io.rad",
        "command": [
            "projects/dogfood/sovereign-grid/builtins/host_io.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "stdin": "alpha\nbeta\ngamma\n",
        "assertion": "sovereign-grid: host IO builtins complete",
        "stability": "stable",
    },
    {
        "name": "host-network-builtins",
        "source": "projects/dogfood/sovereign-grid/builtins/host_network.rad",
        "command": [
            "projects/dogfood/sovereign-grid/builtins/host_network.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "harness": "sovereign-grid-protocol",
        "assertion": "sovereign-grid: host network builtins complete",
        "stability": "stable",
    },
    {
        "name": "ffi-native-builtins",
        "source": "projects/dogfood/sovereign-grid/builtins/ffi_native.rad",
        "command": [
            "projects/dogfood/sovereign-grid/builtins/ffi_native.rad",
            "--strict-types",
            "--deny-warnings",
        ],
        "setup": [
            "python",
            "tooling/build_sovereign_grid_plugin.py",
        ],
        "assertion": "sovereign-grid: FFI native builtins complete",
        "stability": "stable",
    },
]

# Prefer focused executable fixtures over the broad syntax workflow when a
# builtin occurs in both. This makes each API row point to the smallest useful
# production example while keeping the denominator runtime-owned.
BUILTIN_EVIDENCE_ORDER = [
    "value-collection-text-builtins",
    "ecs-query-provenance-builtins",
    "speculation-persistence-builtins",
    "sandbox-guest-builtins",
    "host-io-builtins",
    "host-network-builtins",
    "ffi-native-builtins",
    "relation-builtins",
    "model-tests",
    "causal-settlement",
    "stable-main",
    "workflow-tests",
    "shared-world-tests",
]

DOCS = {
    "token": "docs/src/reference/spec_parts/lexical.md#lexical-structure",
    "binop": "docs/src/reference/spec_parts/lexical.md#operators-and-precedence",
    "unop": "docs/src/reference/spec_parts/lexical.md#operators-and-precedence",
    "decl": "docs/src/reference/spec_parts/declarations.md#declarations",
    "stmt": "docs/src/reference/spec_parts/statements.md#statements",
    "expr": "docs/src/reference/spec_parts/expressions.md#expressions",
    "pattern": "docs/src/reference/spec_parts/expressions.md#patterns",
    "typeexpr": "docs/src/reference/spec_parts/types.md#type-system",
    "fnpurity": "docs/src/reference/spec_parts/types.md#function-types",
    "contract": "docs/src/reference/spec_parts/declarations.md#callable-contracts",
}

OWNERS = {
    "token": "core/syntax/src/lexer/engine.rs",
    "decl": "core/syntax/src/ast/modules.rs",
    "stmt": "core/syntax/src/ast/statements.rs",
    "expr": "core/syntax/src/ast/expressions.rs",
    "pattern": "core/syntax/src/ast/statements.rs",
    "binop": "core/syntax/src/ast/expressions.rs",
    "unop": "core/syntax/src/ast/expressions.rs",
    "typeexpr": "core/syntax/src/ast/expressions.rs",
    "fnpurity": "core/syntax/src/ast/expressions.rs",
    "contract": "core/syntax/src/parser/decl/declaration_dispatch.rs",
}

CONFORMANCE = {
    "token": "core/syntax/src/lexer/tests.rs",
    "decl": "core/syntax/src/parser/tests.rs",
    "stmt": "core/syntax/src/parser/tests.rs",
    "expr": "core/syntax/src/parser/tests.rs",
    "pattern": "core/vm/src/checker/match_test.rs",
    "binop": "core/vm/src/compiler/tests/execution_helpers.rs",
    "unop": "core/vm/src/compiler/tests/execution_helpers.rs",
    "typeexpr": "core/vm/src/checker/tests/functions_and_variants.rs",
    "fnpurity": "core/vm/src/checker/tests/effects_and_mutability.rs",
    "contract": "core/vm/src/checker/tests/semantic_features.rs",
}


def quoted(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def run_surface(rad: Path, source: str) -> dict[str, object]:
    started = time.perf_counter_ns()
    completed = subprocess.run(
        [str(rad), "surface", source, "--json"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=1.0,
    )
    elapsed_ns = time.perf_counter_ns() - started
    if elapsed_ns >= 1_000_000_000:
        raise RuntimeError(
            f"rad surface exceeded hard limit for {source}: elapsedNs={elapsed_ns}"
        )
    if completed.returncode:
        raise RuntimeError(f"rad surface failed for {source}:\n{completed.stdout}{completed.stderr}")
    return json.loads(completed.stdout)


def builtin_api_with_evidence(
    source: str, rows: list[dict[str, str]]
) -> str:
    """Attach checked examples without creating a second builtin catalog."""
    source = re.sub(
        r"^- Executable evidence:.*\n- Verification command:.*\n?",
        "",
        source,
        flags=re.MULTILINE,
    )
    sections = re.split(r'(?=<a id="[a-z0-9_]+"></a>)', source)
    if len(sections) - 1 != len(rows):
        raise RuntimeError("builtin API sections differ from generated coverage rows")
    rendered = [sections[0]]
    for section, row in zip(sections[1:], rows, strict=True):
        anchor = re.match(r'<a id="([a-z0-9_]+)"></a>', section)
        if anchor is None or anchor.group(1) != row["name"]:
            raise RuntimeError("builtin API order differs from Builtin::ALL")
        source_path = row["source"].replace("\\", "/")
        rendered.append(section.rstrip())
        rendered.append(
            "\n- Executable evidence: "
            f"[{source_path}]({REPOSITORY_BLOB}{source_path})\n"
            f"- Verification command: `{row['command']}`\n\n"
        )
    return "".join(rendered)


def builtin_category_index(source: str, rows: list[dict[str, str]]) -> str:
    sections = re.split(r'(?=<a id="[a-z0-9_]+"></a>)', source)[1:]
    grouped: dict[str, list[tuple[str, str]]] = {}
    for section, row in zip(sections, rows, strict=True):
        category = re.search(r"^- Category: `([^`]+)`$", section, re.MULTILINE)
        if category is None:
            raise RuntimeError(f"builtin API {row['name']} has no category")
        grouped.setdefault(category.group(1), []).append((row["name"], row["source"]))
    unknown = sorted(set(grouped) - set(CATEGORY_ORDER))
    if unknown:
        raise RuntimeError(f"builtin API has uncategorized groups: {', '.join(unknown)}")
    out = [
        "<!-- Generated by tooling/generate_language_coverage.py. Do not edit. -->\n",
        "# Builtin API by category\n\n",
        "Every runtime builtin appears exactly once. Each entry links to its exact contract and "
        "to executable Sovereign Grid evidence.\n\n",
    ]
    for category in CATEGORY_ORDER:
        entries = grouped.get(category, [])
        if not entries:
            raise RuntimeError(f"builtin API category {category} is empty")
        out.append(f"## {category.replace('-', ' ').title()}\n\n")
        out.append("| Builtin | Executable evidence |\n|---|---|\n")
        for name, evidence in entries:
            source_path = evidence.replace("\\", "/")
            out.append(
                f"| [`{name}`](builtin-api.md#{name}) | "
                f"[{source_path}]({REPOSITORY_BLOB}{source_path}) |\n"
            )
        out.append("\n")
    return "".join(out)


def syntax_rule_index(rows: list[dict[str, str]]) -> str:
    out = [
        "<!-- Generated by tooling/generate_language_coverage.py. Do not edit. -->\n",
        "# Syntax rules and executable evidence\n\n",
        "Every source-reachable compiler surface has one stable rule ID, a normative definition, "
        "compiler-owned conformance evidence, and an executable Sovereign Grid source.\n\n",
    ]
    current = ""
    for row in rows:
        prefix = row["id"].split(".", 1)[0]
        if prefix != current:
            current = prefix
            out.append(f"## {prefix}\n\n")
            out.append(
                "| Rule ID | Grammar production | Normative rule | Dogfood | Conformance | Stability |\n"
                "|---|---|---|---|---|---|\n"
            )
        anchor = row["normative_doc_anchor"].split("#", 1)[1]
        source_path = row["positive_source"].replace("\\", "/")
        conformance = row["conformance_test"].replace("\\", "/")
        rule_anchor = row["id"].lower().replace(".", "-").replace("_", "-")
        out.append(
            f"| <a id=\"{rule_anchor}\"></a>`{row['id']}` | `{row['grammar_production']}` | "
            f"[spec](../spec.md#{anchor}) | "
            f"[{source_path}]({REPOSITORY_BLOB}{source_path}) | "
            f"[{conformance}]({REPOSITORY_BLOB}{conformance}) | `{row['stability']}` |\n"
        )
    return "".join(out)


def canonical_text(text: str) -> str:
    """Return UTF-8 text with exactly one trailing newline."""
    return text.rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True, type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rad = args.rad.resolve()
    surface = json.loads(SURFACE.read_text(encoding="utf-8"))
    reports = [(entry, run_surface(rad, entry["source"])) for entry in EVIDENCE]

    lines = [
        "# Generated by tooling/generate_language_coverage.py. Do not edit.\n",
        "format_version = 1\n",
        f"surface_owner_digest = {quoted(surface['ownerDigest'])}\n",
        "evidence_kind = \"compiler-ast-and-token-report\"\n\n",
    ]
    builtin_rows: list[dict[str, str]] = []
    syntax_rows: list[dict[str, str]] = []
    for prefix, key in GROUPS.items():
        for name in surface[key]:
            identifier = f"{prefix}.{name}"
            if identifier in INTERNAL:
                lines.extend(
                    [
                        "[[exclusion]]\n",
                        f"id = {quoted(identifier)}\n",
                        f"reason = {quoted(INTERNAL[identifier])}\n\n",
                    ]
                )
                continue
            evidence = next(
                (entry for entry, report in reports if name in report[key]),
                None,
            )
            if evidence is None:
                raise RuntimeError(f"Sovereign Grid does not cover compiler surface {identifier}")
            command = "rad " + " ".join(evidence["command"])
            lines.extend(
                [
                    "[[coverage]]\n",
                    f"id = {quoted(identifier)}\n",
                    f"grammar_production = {quoted(identifier)}\n",
                    f"parser_owner = {quoted(OWNERS[prefix])}\n",
                    f"normative_doc_anchor = {quoted(DOCS[prefix])}\n",
                    f"positive_source = {quoted(evidence['source'])}\n",
                    f"positive_command = {quoted(command)}\n",
                    f"positive_assertion = {quoted(evidence['assertion'])}\n",
                    f"conformance_test = {quoted(CONFORMANCE[prefix])}\n",
                    f"sovereign_grid_source = {quoted(evidence['source'])}\n",
                    f"stability = {quoted(evidence['stability'])}\n\n",
                ]
            )
            syntax_rows.append(
                {
                    "id": identifier,
                    "grammar_production": identifier,
                    "normative_doc_anchor": DOCS[prefix],
                    "positive_source": evidence["source"],
                    "conformance_test": CONFORMANCE[prefix],
                    "stability": evidence["stability"],
                }
            )

    by_name = {entry["name"]: (entry, report) for entry, report in reports}
    for builtin in surface["builtins"]:
        name = builtin["name"]
        evidence = next(
            (
                by_name[evidence_name][0]
                for evidence_name in BUILTIN_EVIDENCE_ORDER
                if name in by_name[evidence_name][1]["builtins"]
            ),
            None,
        )
        if evidence is None:
            raise RuntimeError(f"Sovereign Grid does not execute builtin {name}")
        command = "rad " + " ".join(evidence["command"])
        builtin_rows.append(
            {
                "name": name,
                "source": evidence["source"],
                "command": command,
            }
        )
        lines.extend(
            [
                "[[builtin_coverage]]\n",
                f"name = {quoted(name)}\n",
                f"runtime_variant = {quoted(builtin['variant'])}\n",
                f"api_anchor = {quoted('docs/src/reference/generated/builtin-api.md#' + name)}\n",
                f"source = {quoted(evidence['source'])}\n",
                f"command = {quoted(command)}\n",
                f"assertion = {quoted(evidence['assertion'])}\n",
                f"execution = {quoted(evidence.get('execution', 'direct'))}\n",
                f"stability = {quoted(evidence['stability'])}\n\n",
            ]
        )

    for evidence in EVIDENCE:
        lines.append("[[command]]\n")
        lines.append(f"name = {quoted(evidence['name'])}\n")
        args_text = ", ".join(quoted(value) for value in evidence["command"])
        lines.append(f"args = [{args_text}]\n")
        lines.append("exit = 0\n")
        lines.append(f"contains = {quoted(evidence['assertion'])}\n\n")
        if "stdin" in evidence:
            lines.insert(len(lines) - 1, f"stdin = {quoted(evidence['stdin'])}\n")
        if "harness" in evidence:
            lines.insert(len(lines) - 1, f"harness = {quoted(evidence['harness'])}\n")
        if "setup" in evidence:
            setup_text = ", ".join(quoted(value) for value in evidence["setup"])
            lines.insert(len(lines) - 1, f"setup = [{setup_text}]\n")

    rendered = canonical_text("".join(lines))
    if not BUILTIN_API.is_file():
        raise RuntimeError(
            "builtin API is missing; run the runtime-owned builtin reference generator first"
        )
    enriched_api = canonical_text(
        builtin_api_with_evidence(
            BUILTIN_API.read_text(encoding="utf-8"), builtin_rows
        )
    )
    category_index = canonical_text(builtin_category_index(enriched_api, builtin_rows))
    rules_index = canonical_text(syntax_rule_index(syntax_rows))
    if args.check:
        if (
            not OUTPUT.exists()
            or OUTPUT.read_text(encoding="utf-8") != rendered
            or BUILTIN_API.read_text(encoding="utf-8") != enriched_api
            or not BUILTIN_INDEX.is_file()
            or BUILTIN_INDEX.read_text(encoding="utf-8") != category_index
            or not SYNTAX_INDEX.is_file()
            or SYNTAX_INDEX.read_text(encoding="utf-8") != rules_index
        ):
            print(f"stale language coverage: run {Path(__file__).relative_to(ROOT)} --rad <rad>")
            return 1
        print(
            f"language coverage current: {rendered.count('[[coverage]]')} source rows, "
            f"{rendered.count('[[builtin_coverage]]')} builtin rows"
        )
        return 0
    OUTPUT.write_text(rendered, encoding="utf-8", newline="\n")
    BUILTIN_API.write_text(enriched_api, encoding="utf-8", newline="\n")
    BUILTIN_INDEX.write_text(category_index, encoding="utf-8", newline="\n")
    SYNTAX_INDEX.write_text(rules_index, encoding="utf-8", newline="\n")
    print(
        f"{OUTPUT.relative_to(ROOT)}: {rendered.count('[[coverage]]')} source rows, "
        f"{rendered.count('[[builtin_coverage]]')} builtin rows"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
