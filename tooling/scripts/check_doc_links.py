"""Verify every relative markdown link under docs/src (plus the root README)
resolves to a real file, and that any #anchor points at a real heading.

Usage:  py tooling/scripts/check_doc_links.py
Exits non-zero when a link is broken, so it can gate CI.
"""

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*$", re.MULTILINE)
INCLUDE = re.compile(r"\{\{#include\s+([^}\s]+)\}\}")


def slug(text: str) -> str:
    text = re.sub(r"`|\*|_", "", text)
    text = text.lower().strip()
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"[\s]+", "-", text)


def rendered_markdown(path: Path, stack: tuple[Path, ...] = ()) -> str:
    """Expand mdBook includes while preserving the owning page's link base."""
    resolved = path.resolve()
    if resolved in stack:
        chain = " -> ".join(str(item.relative_to(REPO)) for item in (*stack, resolved))
        raise ValueError(f"cyclic mdBook include: {chain}")

    body = resolved.read_text(encoding="utf-8")
    next_stack = (*stack, resolved)

    def expand(match: re.Match) -> str:
        included = (resolved.parent / match.group(1)).resolve()
        return rendered_markdown(included, next_stack)

    return INCLUDE.sub(expand, body)


def anchors_of(path: Path) -> set:
    try:
        body = rendered_markdown(path)
    except (OSError, ValueError):
        return set()
    return {slug(h) for h in HEADING.findall(body)}


def main() -> int:
    targets = sorted((REPO / "docs" / "src").rglob("*.md"))
    targets.append(REPO / "README.md")

    # Included fragments are rendered in the including page's URL context. Scan
    # that expanded page once instead of incorrectly treating private fragments
    # as independently addressable documentation pages.
    included = set()
    for md in targets:
        try:
            body = md.read_text(encoding="utf-8")
        except OSError:
            continue
        included.update((md.parent / raw).resolve() for raw in INCLUDE.findall(body))
    targets = [md for md in targets if md.resolve() not in included]

    broken = []
    checked = 0

    for md in targets:
        try:
            body = rendered_markdown(md)
        except (OSError, ValueError) as error:
            broken.append(f"{md.relative_to(REPO)} -> {error}")
            continue
        for raw in LINK.findall(body):
            href = raw.split(" ")[0].strip()
            if href.startswith(("http://", "https://", "mailto:")):
                continue
            frag = ""
            if "#" in href:
                href, frag = href.split("#", 1)
            if not href:
                if frag and frag not in anchors_of(md):
                    broken.append(f"{md.relative_to(REPO)} -> #{frag} (no such heading)")
                continue
            checked += 1
            dest = (md.parent / href).resolve()
            if not dest.exists():
                broken.append(f"{md.relative_to(REPO)} -> {href} (missing file)")
            elif frag and dest.suffix == ".md" and frag not in anchors_of(dest):
                broken.append(f"{md.relative_to(REPO)} -> {href}#{frag} (no such heading)")

    print(f"scanned {len(targets)} files, {checked} relative links")
    if broken:
        print(f"\nBROKEN ({len(broken)}):")
        for b in broken:
            print(f"  {b}")
        return 1
    print("all links resolve")
    return 0


if __name__ == "__main__":
    sys.exit(main())
