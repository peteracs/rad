import subprocess
import sys
from pathlib import Path

VM_ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = VM_ROOT.parent.parent
SITE = REPO_ROOT / "docs" / "src"


def main() -> None:
    ok: list[str] = []
    failed: list[tuple[str, str]] = []
    empty: list[str] = []
    for md in sorted(SITE.rglob("*.md")):
        if md.name == "SUMMARY.md":
            continue
        rel = md.relative_to(REPO_ROOT).as_posix()
        r = subprocess.run(
            [sys.executable, str(VM_ROOT / "scripts" / "merge_run.py"), "--smart", rel],
            cwd=str(VM_ROOT),
            capture_output=True,
            text=True,
        )
        if r.returncode == 0:
            ok.append(rel)
        elif r.returncode == 2:
            empty.append(rel)
        else:
            err = (r.stderr or r.stdout or "").strip()
            failed.append((rel, err[:1200]))

    print(f"PASS (merged Rad compiles): {len(ok)}")
    print(f"FAIL (merged Rad errors): {len(failed)}")
    print(f"SKIP (no Rad-like blocks): {len(empty)}")
    print()
    for rel, msg in failed:
        print(f"=== FAIL: {rel} ===")
        print(msg)
        print()


if __name__ == "__main__":
    main()
