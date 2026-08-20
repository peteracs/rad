#!/usr/bin/env python3
"""Enforce repository ownership boundaries that paths alone can prove."""

from __future__ import annotations

import subprocess
import sys
import re
from pathlib import Path, PurePosixPath


FORBIDDEN_PREFIXES = {
    "core/c-backend/": "non-authoritative experiments belong under experiments/",
    "core/simcore/": "project-specific simulation belongs to projects/moba/",
    "projects/dogfood/moba/": "all MOBA-owned code belongs under projects/moba/",
    "projects/moba-rad/": "the MOBA vertical slice belongs under projects/moba/",
    "projects/rad-webgpu/": "host integrations belong under adapters/",
}

FORBIDDEN_DIRECTORY_NAMES = {
    "causality_sections",
    "lexer_sections",
}

RFC_SOURCE = Path("docs/rfcs")
RFC_WRAPPERS = Path("docs/src/rfcs")
FOLDER_TREE = Path("docs/src/project/folder_tree.md")


def repository_files(root: Path) -> list[PurePosixPath]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
    )
    return [
        PurePosixPath(item.decode("utf-8"))
        for item in result.stdout.split(b"\0")
        if item
    ]


def audit(root: Path) -> list[str]:
    files = repository_files(root)
    errors: list[str] = []

    for path in files:
        name = path.as_posix()
        for prefix, reason in FORBIDDEN_PREFIXES.items():
            if name.startswith(prefix):
                errors.append(f"{name}: {reason}")
        bad_parts = FORBIDDEN_DIRECTORY_NAMES.intersection(path.parts)
        for part in sorted(bad_parts):
            errors.append(
                f"{name}: directory '{part}' describes a file-splitting mechanism, "
                "not a responsibility"
            )
        if name.startswith("core/") and not name.startswith("core/vm/"):
            errors.append(
                f"{name}: core/ is reserved for the authoritative language/runtime"
            )

    manifest = (root / "core/vm/Cargo.toml").read_text(encoding="utf-8")
    for forbidden in ("../../adapters", "../../projects", "../adapters", "../projects"):
        if forbidden in manifest:
            errors.append(
                f"core/vm/Cargo.toml: core must not depend on {forbidden!r}"
            )

    source_names = {
        path.name for path in (root / RFC_SOURCE).glob("*.md") if path.is_file()
    }
    wrapper_names = {
        path.name for path in (root / RFC_WRAPPERS).glob("*.md") if path.is_file()
    }
    for missing in sorted(source_names - wrapper_names):
        errors.append(f"{RFC_WRAPPERS / missing}: missing RFC book wrapper")
    for orphan in sorted(wrapper_names - source_names):
        errors.append(f"{RFC_WRAPPERS / orphan}: wrapper has no canonical RFC source")
    for name in sorted(source_names & wrapper_names):
        wrapper = root / RFC_WRAPPERS / name
        expected = f"{{{{#include ../../rfcs/{name}}}}}\n"
        actual = wrapper.read_text(encoding="utf-8")
        if actual != expected:
            errors.append(
                f"{wrapper.relative_to(root).as_posix()}: must be exactly {expected.strip()!r}"
            )

    folder_tree = root / FOLDER_TREE
    if folder_tree.is_file():
        actual_directories: set[str] = set()
        for path in files:
            parent = path.parent
            while parent != PurePosixPath("."):
                actual_directories.add(f"{parent.as_posix()}/")
                parent = parent.parent
        documented_directories = set(
            re.findall(
                r"^\s+- `([^`]+/)` - ",
                folder_tree.read_text(encoding="utf-8"),
                flags=re.MULTILINE,
            )
        )
        for missing in sorted(actual_directories - documented_directories):
            errors.append(f"{FOLDER_TREE}: missing repository directory {missing}")
        for stale in sorted(documented_directories - actual_directories):
            errors.append(f"{FOLDER_TREE}: documents nonexistent directory {stale}")

    return sorted(set(errors))


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    try:
        errors = audit(root)
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"architecture gate error: {error}", file=sys.stderr)
        return 2
    if errors:
        print("repository architecture violations:", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1
    print("architecture gate passed: ownership paths, dependency direction, and RFC authority are canonical")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
