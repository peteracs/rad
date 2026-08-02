import re
import subprocess
import sys
from pathlib import Path

VM_ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = VM_ROOT.parent.parent

SKIP_LANGS = frozenset(
    {
        "bash",
        "sh",
        "shell",
        "yaml",
        "yml",
        "toml",
        "json",
        "ebnf",
        "text",
        "mermaid",
    }
)

RAD_HINT = re.compile(
    r"(?m)^\s*(component|struct|entity|event|state|system|fn|pure\s+fn|async\s+fn|"
    r"on\s+\w+|let\s+|print\(|spawn\(|emit\s|schedule\s|query\s|test\s|use\s|type\s+\w+\s*\{)",
)


def looks_like_rad_source(content: str) -> bool:
    s = content.strip()
    if not s or "::=" in s[:300]:
        return False
    return bool(RAD_HINT.search(s))


def merge_md(path: Path, smart: bool) -> str:
    text = path.read_text(encoding="utf-8")
    blocks = re.findall(r"^```([^\n]*)\n(.*?)```", text, re.MULTILINE | re.DOTALL)
    parts: list[str] = []
    for lang_line, body in blocks:
        lang = (lang_line.strip().split() or [""])[0].lower()
        if lang in SKIP_LANGS:
            continue
        if smart:
            if lang and lang != "rad":
                continue
            if not lang and not looks_like_rad_source(body):
                continue
        parts.append(body.strip())
    return "\n\n".join(parts)


def main() -> None:
    args = [a for a in sys.argv[1:] if a != "--smart"]
    smart = "--smart" in sys.argv[1:]
    rel = args[0]
    p = (REPO_ROOT / rel).resolve()
    out = REPO_ROOT / "target" / "merged_run.rad"
    merged = merge_md(p, smart)
    if not merged.strip():
        print("No Rad-like blocks after filter.", file=sys.stderr)
        sys.exit(2)
    out.write_text(merged + "\n", encoding="utf-8")
    r = subprocess.run(
        ["cargo", "run", "--quiet", "--", "--compat-v0.5-dx", str(out)],
        cwd=str(VM_ROOT),
    )
    sys.exit(r.returncode)


if __name__ == "__main__":
    main()
