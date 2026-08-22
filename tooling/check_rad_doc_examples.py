#!/usr/bin/env python3
"""Prove normative RAD blocks are canonical, source-owned, and executable."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REFERENCE = ROOT / "docs/src/reference"
PROJECT = (ROOT / "projects/dogfood/sovereign-grid").resolve()
FENCE = re.compile(r"^```([^\n`]*)\n(.*?)^```\s*$", re.M | re.S)
INCLUDE = re.compile(r"^\s*\{\{#include\s+([^}:]+\.rad)(?::[^}]*)?\}\}\s*$")
REQUIRED = {
    "main.rad",
    "schema.rad",
    "language/surface.rad",
    "language/values.rad",
    "owners/grid_owner.rad",
    "systems/control_loop.rad",
    "tests/grid_model.rad",
    "tests/shared_world.rad",
    "builtins/values_collections_text.rad",
    "builtins/ecs_queries_provenance.rad",
    "builtins/speculation_persistence.rad",
    "builtins/host_io.rad",
    "builtins/host_network.rad",
    "builtins/ffi_native.rad",
    "experimental/causal_dispatch.rad",
    "experimental/relations.rad",
    "experimental/relation_builtins.rad",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True, type=Path)
    args = parser.parse_args()
    failures: list[str] = []
    included: set[str] = set()
    block_count = 0

    for markdown in sorted(REFERENCE.rglob("*.md")):
        text = markdown.read_text(encoding="utf-8")
        for index, match in enumerate(FENCE.finditer(text), start=1):
            info = match.group(1).strip().lower()
            if not info.startswith("rad"):
                continue
            block_count += 1
            location = f"{markdown.relative_to(ROOT).as_posix()} block {index}"
            if info != "rad":
                failures.append(f"{location}: RAD fences cannot be ignored or reclassified")
                continue
            include = INCLUDE.fullmatch(match.group(2))
            if include is None:
                failures.append(f"{location}: use an mdBook include from executable Sovereign Grid source")
                continue
            source = (markdown.parent / include.group(1)).resolve()
            try:
                relative = source.relative_to(PROJECT).as_posix()
            except ValueError:
                failures.append(f"{location}: include escapes the canonical dogfood project")
                continue
            if not source.is_file():
                failures.append(f"{location}: missing include {source}")
                continue
            included.add(relative)

    missing = sorted(REQUIRED - included)
    if missing:
        failures.append("executable tour is missing: " + ", ".join(missing))
    if included - REQUIRED:
        failures.append("unreviewed executable-tour sources: " + ", ".join(sorted(included - REQUIRED)))
    if block_count != len(REQUIRED):
        failures.append(f"expected {len(REQUIRED)} normative RAD blocks, found {block_count}")

    if failures:
        print("RAD documentation examples: FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    completed = subprocess.run(
        [sys.executable, str(ROOT / "tooling/check_language_surface.py"), "--rad", str(args.rad)],
        cwd=ROOT,
    )
    if completed.returncode:
        return completed.returncode
    print(f"RAD documentation examples: PASS - {block_count} source-owned blocks, complete syntax/API gate executed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
