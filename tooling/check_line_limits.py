#!/usr/bin/env python3
"""Fail when a repository text file exceeds the line limit."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


DEFAULT_LIMIT = 1_000
DEFAULT_EXCEPTIONS = Path("tooling/line-limit-exceptions.tsv")
POLICY_REMINDER = """\
line-limit policy — 1,000 lines is an SRP review trigger, not an automatic split:
  1. Read the whole file, its composition root, siblings, and call sites. Explain
     why it grew and identify its real responsibilities before moving code.
  2. If the file genuinely has one indivisible responsibility, add one exact-path
     exception with a concrete justification. Exceptions are better than fake splits.
  3. Otherwise split mechanically first to preserve behavior, verify that step, then
     complete the semantic refactor before declaring the work done.
  4. Boundaries must be cohesive and maintainable. Move behavior to the module that
     owns it; keep shared helpers separate only when they serve multiple consumers or
     form a stable independent responsibility. Keep the result DRY.
  5. Never create quota/remainder/misc/numbered fragments, extract an arbitrary tiny
     helper, use misleading "A_and_B" buckets, minify, compress formatting, or delete
     useful comments/tests merely to get below the number.
  6. A small file is not automatically wrong. Composition roots, focused tests, and
     genuinely narrow domain modules are valid; small grab bags and threshold tails
     are not. Review the parent for SRP whenever such a tail appears.
  7. Do not balance modules by line count or treat 999 lines as a design goal.
     Responsibilities naturally differ in size; names and boundaries must describe
     the domain, not extraction order, chronology, or the mechanics of the split.
  8. Re-audit every new small file and its parent after refactoring. Remove obsolete
     fragments and composition entries; do not leave a tiny tail that merely made the
     parent pass.
  9. Preserve unrelated work. After both the mechanical and semantic stages, run
     formatting, focused tests, the full relevant suite, strict linting, this gate,
     and remove any stale exception.
"""


def repository_files(root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
    )
    return [root / item.decode("utf-8") for item in result.stdout.split(b"\0") if item]


def text_line_count(path: Path) -> int | None:
    data = path.read_bytes()
    if b"\0" in data:
        return None
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        return None
    return len(text.splitlines())


def load_exceptions(root: Path, path: Path) -> dict[str, str]:
    exceptions: dict[str, str] = {}
    for number, raw in enumerate((root / path).read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        try:
            name, reason = raw.split("\t", 1)
        except ValueError as error:
            raise ValueError(f"{path}:{number}: expected PATH<TAB>JUSTIFICATION") from error
        name = name.strip().replace("\\", "/")
        reason = reason.strip()
        if not name or len(reason) < 20:
            raise ValueError(f"{path}:{number}: exception needs an exact path and useful reason")
        if name in exceptions:
            raise ValueError(f"{path}:{number}: duplicate exception for {name}")
        exceptions[name] = reason
    return exceptions


def audit(root: Path, limit: int, exception_path: Path) -> int:
    print(POLICY_REMINDER)
    exceptions = load_exceptions(root, exception_path)
    oversized: dict[str, int] = {}
    for path in repository_files(root):
        if not path.is_file():
            continue
        count = text_line_count(path)
        if count is not None and count > limit:
            oversized[path.relative_to(root).as_posix()] = count

    stale = sorted(set(exceptions) - set(oversized))
    missing = sorted(set(oversized) - set(exceptions), key=lambda name: (-oversized[name], name))
    if not stale and not missing:
        print(
            f"line-limit gate passed: every repository text file is <= {limit} lines "
            f"or has one exact justified exception ({len(exceptions)} exceptions)"
        )
        return 0

    if missing:
        print(f"repository text files over {limit} lines without an exception:", file=sys.stderr)
        for name in missing:
            print(f"  {oversized[name]:>6}  {name}", file=sys.stderr)
    if stale:
        print("stale line-limit exceptions (remove them):", file=sys.stderr)
        for name in stale:
            print(f"  {name}", file=sys.stderr)
    return 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--limit", type=int, default=DEFAULT_LIMIT)
    parser.add_argument("--exceptions", type=Path, default=DEFAULT_EXCEPTIONS)
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        return audit(root, arguments.limit, arguments.exceptions)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"line-limit gate error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
