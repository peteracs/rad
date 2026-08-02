"""
Extract fenced code blocks from docs and run each through `rad` (cargo run).
Usage: py core/vm/scripts/run_doc_examples.py  (from the repository root)
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

VM_ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = VM_ROOT.parent.parent
DOCS_ROOTS = [
    REPO_ROOT / "docs" / "src",
]
OUT_DIR = REPO_ROOT / "target" / "doc_example_runs"

SKIP_LANGS = frozenset(
    {
        "bash",
        "sh",
        "shell",
        "zsh",
        "powershell",
        "pwsh",
        "yaml",
        "yml",
        "toml",
        "json",
        "ebnf",
        "text",
        "plaintext",
        "diff",
        "ignore",
        "mermaid",
        "rust",
        "md",
        "markdown",
    }
)

FENCE_RE = re.compile(r"^```([^\n`]*)\n(.*?)```", re.MULTILINE | re.DOTALL)

RAD_HINT = re.compile(
    r"(?m)^\s*(component|struct|entity|event|state|system|fn|pure\s+fn|async\s+fn|"
    r"on\s+\w+|let\s+|print\(|spawn\(|emit\s|schedule\s|query\s|test\s|use\s|type\s+\w+\s*\{)",
)


def looks_like_rad_source(content: str) -> bool:
    s = content.strip()
    if not s:
        return False
    if "::=" in s[:500] or s.lstrip().startswith("(*"):
        return False
    if RAD_HINT.search(s):
        return True
    return False


def should_run_block(lang: str | None, content: str) -> tuple[bool, str]:
    lang = (lang or "").strip().lower()
    if lang in SKIP_LANGS:
        return False, f"skipped language `{lang or '(none)'}`"
    if lang == "rad":
        return True, "explicit rad"
    if lang:
        return False, f"skipped unknown language `{lang}`"
    if not looks_like_rad_source(content):
        return False, "unmarked block does not look like Rad (heuristic)"
    return True, "unmarked, heuristic Rad"


def collect_md_files() -> list[Path]:
    files: list[Path] = []
    for root in DOCS_ROOTS:
        if not root.is_dir():
            continue
        for p in root.rglob("*.md"):
            files.append(p)
    return sorted(files)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    results: list[tuple[str, int, str, str, str]] = []

    for md_path in collect_md_files():
        try:
            rel = md_path.relative_to(VM_ROOT.parent)
        except ValueError:
            rel = md_path
        text = md_path.read_text(encoding="utf-8", errors="replace")
        idx = 0
        for m in FENCE_RE.finditer(text):
            idx += 1
            lang_raw = m.group(1).strip()
            content = m.group(2)
            if lang_raw and " " in lang_raw:
                lang = lang_raw.split()[0]
            else:
                lang = lang_raw

            run, reason = should_run_block(lang if lang_raw else None, content)
            loc = f"{rel}:{idx}"
            if not run:
                results.append((str(rel), idx, "skip", reason, ""))
                continue

            rad_path = OUT_DIR / f"{rel.as_posix().replace('/', '_')}_{idx:03d}.rad"
            rad_path.parent.mkdir(parents=True, exist_ok=True)
            rad_path.write_text(content.rstrip() + "\n", encoding="utf-8")

            proc = subprocess.run(
                ["cargo", "run", "--quiet", "--", str(rad_path)],
                cwd=str(VM_ROOT),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
            )
            err_out = (proc.stderr or "") + ("\n" + proc.stdout if proc.stdout else "")
            err_out = err_out.strip()

            if proc.returncode == 0:
                status = "ok"
                detail = err_out if err_out else ""
            else:
                status = "fail"
                detail = err_out[:4000] if err_out else "(no output)"

            results.append((str(rel), idx, status, reason, detail))

    lines = []
    bad: list[str] = []
    for rel, idx, status, reason, detail in results:
        if status == "skip":
            lines.append(f"[skip] {rel} block {idx} — {reason}")
        elif status == "ok":
            warn = ""
            if detail and ("Error" in detail or "error" in detail):
                warn = " (unexpected?)"
            lines.append(f"[ok]   {rel} block {idx} — {reason}{warn}")
            if detail and ("Warning" in detail or "warning" in detail):
                for dl in detail.splitlines()[:8]:
                    lines.append(f"       {dl}")
        else:
            lines.append(f"[FAIL] {rel} block {idx} — {reason}")
            bad.append(f"{rel} block {idx}")
            for dl in detail.splitlines()[:25]:
                lines.append(f"       {dl}")
            if len(detail.splitlines()) > 25:
                lines.append("       ...")

    report = "\n".join(lines)
    print(report)
    print()
    print(f"Summary: total runnable blocks checked = {sum(1 for r in results if r[2] != 'skip')}")
    print(f"Failures: {len(bad)}")
    if bad:
        print("Failed:")
        for b in bad:
            print(f"  - {b}")
        return 1
    return 0


if sys.platform == "win32":
    import io

    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

if __name__ == "__main__":
    raise SystemExit(main())
