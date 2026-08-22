#!/usr/bin/env python3
"""Generate the canonical RAD language/API surface from production owners.

This file deliberately does not contain a second list of tokens, AST nodes,
contracts, or builtins.  It extracts those lists from the lexer, AST, parser,
and builtin catalog that the compiler itself consumes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs" / "language-surface.json"
MARKDOWN_OUTPUT = ROOT / "docs/src/reference/generated/language-surface.md"
OWNERS = {
    "tokens": ROOT / "core/vm/src/lexer/engine.rs",
    "keywords": ROOT / "core/vm/src/lexer/decl.rs",
    "declarations": ROOT / "core/vm/src/ast/modules.rs",
    "statements": ROOT / "core/vm/src/ast/statements.rs",
    "expressions": ROOT / "core/vm/src/ast/expressions.rs",
    "contracts": ROOT / "core/vm/src/parser/decl/declaration_dispatch.rs",
    "builtins": ROOT / "core/vm/src/value/builtin_catalog.rs",
}


def source_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def braced_body(text: str, prefix_pattern: str) -> str:
    match = re.search(prefix_pattern, text)
    if not match:
        raise RuntimeError(f"surface owner missing pattern: {prefix_pattern}")
    opening = text.find("{", match.end() - 1)
    depth = 0
    in_string = False
    escaped = False
    for index in range(opening, len(text)):
        char = text[index]
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[opening + 1 : index]
    raise RuntimeError(f"unclosed owner block: {prefix_pattern}")


def enum_variants(path: Path, name: str) -> list[str]:
    body = braced_body(source_text(path), rf"\bpub\s+enum\s+{re.escape(name)}\b")
    variants: list[str] = []
    depth = 0
    item: list[str] = []
    for char in body:
        if char in "([{<":
            depth += 1
        elif char in ")]}>":
            depth -= 1
        if char == "," and depth == 0:
            raw = "".join(item)
            raw = re.sub(r"//[^\n]*", "", raw).strip()
            match = re.match(r"(?:#\[[^]]+\]\s*)*([A-Z][A-Za-z0-9_]*)", raw)
            if match:
                variants.append(match.group(1))
            item.clear()
        else:
            item.append(char)
    raw = re.sub(r"//[^\n]*", "", "".join(item)).strip()
    match = re.match(r"(?:#\[[^]]+\]\s*)*([A-Z][A-Za-z0-9_]*)", raw)
    if match:
        variants.append(match.group(1))
    if not variants:
        raise RuntimeError(f"no variants extracted for {name} from {path}")
    return variants


def keywords() -> list[dict[str, str]]:
    body = braced_body(source_text(OWNERS["keywords"]), r"\bmatch\s+word\s*")
    pairs = re.findall(
        r'"([a-z_]+)"\s*=>\s*Some\(TokenType::([A-Za-z0-9_]+)\)', body
    )
    if not pairs:
        raise RuntimeError("no hard keywords extracted")
    return [{"spelling": spelling, "token": token} for spelling, token in pairs]


def contracts() -> list[str]:
    text = source_text(OWNERS["contracts"])
    body = braced_body(text, r"\bfn\s+parse_callable_contracts\b")
    names = re.findall(r'^\s*"([a-z_]+)"\s*=>', body, flags=re.MULTILINE)
    if not names:
        raise RuntimeError("no callable contracts extracted")
    return names


def builtins() -> list[dict[str, str]]:
    text = source_text(OWNERS["builtins"])
    body = braced_body(text, r"\bdefine_builtins!\s*")
    pairs = re.findall(r'^\s*([A-Z][A-Za-z0-9_]*)\s*=>\s*"([a-z0-9_]+)"\s*,?', body, re.MULTILINE)
    if not pairs:
        raise RuntimeError("no builtins extracted")
    return [{"variant": variant, "name": name} for variant, name in pairs]


def build_surface() -> dict[str, object]:
    owner_hash = hashlib.sha256()
    sources: list[dict[str, str]] = []
    for role, path in sorted(OWNERS.items()):
        relative = path.relative_to(ROOT).as_posix()
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        owner_hash.update(role.encode())
        owner_hash.update(b"\0")
        owner_hash.update(relative.encode())
        owner_hash.update(b"\0")
        owner_hash.update(bytes.fromhex(digest))
        sources.append({"role": role, "path": relative, "sha256": digest})

    return {
        "formatVersion": 1,
        "ownerDigest": owner_hash.hexdigest(),
        "sources": sources,
        "tokens": enum_variants(OWNERS["tokens"], "TokenType"),
        "hardKeywords": keywords(),
        "declarations": enum_variants(OWNERS["declarations"], "Decl"),
        "statements": enum_variants(OWNERS["statements"], "Stmt"),
        "expressions": enum_variants(OWNERS["expressions"], "Expr"),
        "patterns": enum_variants(OWNERS["statements"], "Pattern"),
        "binaryOperators": enum_variants(OWNERS["expressions"], "BinOp"),
        "unaryOperators": enum_variants(OWNERS["expressions"], "UnaryOp"),
        "typeExpressions": enum_variants(OWNERS["expressions"], "TypeExpr"),
        "functionTypePurities": enum_variants(OWNERS["expressions"], "FnTypePurity"),
        "callableContracts": contracts(),
        "builtins": builtins(),
    }


def encoded_surface() -> str:
    return json.dumps(build_surface(), indent=2, ensure_ascii=False) + "\n"


def markdown_surface(surface: dict[str, object]) -> str:
    def names(title: str, values: list[str]) -> str:
        rows = "\n".join(f"| `{value}` |" for value in values)
        return f"## {title}\n\n| Compiler surface |\n|---|\n{rows}\n\n"

    out = [
        "<!-- Generated by tooling/language_surface.py. Do not edit. -->\n",
        "# Compiler-owned language surface\n\n",
        "This inventory is generated from the production lexer, AST, parser, and builtin catalog. ",
        "It is the completeness denominator for the normative reference and Sovereign Grid coverage gate. ",
        "Names such as `Error` are recovery nodes rather than source syntax; the coverage manifest records explicit exclusions.\n\n",
        f"Owner digest: `{surface['ownerDigest']}`.\n\n",
        names("Token kinds", surface["tokens"]),
        "## Hard keywords\n\n| Spelling | Token |\n|---|---|\n",
    ]
    out.extend(
        f"| `{entry['spelling']}` | `{entry['token']}` |\n"
        for entry in surface["hardKeywords"]
    )
    out.append("\n")
    for title, key in [
        ("Declaration nodes", "declarations"),
        ("Statement nodes", "statements"),
        ("Expression nodes", "expressions"),
        ("Pattern nodes", "patterns"),
        ("Binary operators", "binaryOperators"),
        ("Unary operators", "unaryOperators"),
        ("Type-expression nodes", "typeExpressions"),
        ("Function-type purities", "functionTypePurities"),
        ("Callable contracts", "callableContracts"),
    ]:
        out.append(names(title, surface[key]))
    out.append("## Builtin API names\n\n")
    out.append(
        "The 256 names below come from the one runtime catalog. Signatures and operational semantics are in the "
        "[complete builtin API catalog](builtin-api.md); this generated list makes omissions machine-detectable.\n\n"
    )
    out.append("| Name | Runtime variant |\n|---|---|\n")
    out.extend(
        f"| <a id=\"builtin-{entry['name']}\"></a>`{entry['name']}` | `{entry['variant']}` |\n"
        for entry in surface["builtins"]
    )
    return "".join(out)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail when generated output is stale")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--markdown", type=Path, default=MARKDOWN_OUTPUT)
    args = parser.parse_args()
    surface = build_surface()
    rendered = json.dumps(surface, indent=2, ensure_ascii=False) + "\n"
    markdown = markdown_surface(surface)
    if args.check:
        stale = (
            not args.output.exists()
            or args.output.read_text(encoding="utf-8") != rendered
            or not args.markdown.exists()
            or args.markdown.read_text(encoding="utf-8") != markdown
        )
        if stale:
            print(f"stale language surface: run {Path(__file__).relative_to(ROOT)}")
            return 1
        print(
            "language surface current: "
            f"{len(surface['tokens'])} tokens, "
            f"{len(surface['declarations'])} declarations, "
            f"{len(surface['statements'])} statements, "
            f"{len(surface['expressions'])} expressions, "
            f"{len(surface['patterns'])} patterns, "
            f"{len(surface['builtins'])} builtins"
        )
        return 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered, encoding="utf-8", newline="\n")
    args.markdown.parent.mkdir(parents=True, exist_ok=True)
    args.markdown.write_text(markdown, encoding="utf-8", newline="\n")
    print(f"{args.output.relative_to(ROOT)} + {args.markdown.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
