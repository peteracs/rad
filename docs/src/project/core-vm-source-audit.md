# Core VM Source Audit

This page records the current audit contract for `core/vm/src`. The detailed
June 2026 shot log and per-file line ledger were useful during the original
review, but they became misleading after the SRP split: module roots moved into
focused children while the old table continued to describe deleted monoliths as
live files. That point-in-time log remains available in Git history; it is not a
live architecture inventory.

The live sources of truth are now:

- [Repository Map](repo-map.md) for responsibility boundaries;
- [Folder Tree](folder_tree.md) for every intended directory;
- `tooling/check_architecture.py` for placement and dependency rules;
- `tooling/check_line_limits.py` for the 1,000-line SRP gate; and
- `rg.exe --files core/vm/src -g "*.rs"` for the current file inventory.

## Current snapshot

Snapshot captured from the working tree on **2026-08-23**. These numbers are a
dated receipt, not values that future edits should treat as live constants.

| Scope | Rust files | Lines | Responsibility |
|---|---:|---:|---|
| `core/syntax/src` | 35 | 11,454 | Lexer, parser, AST, source-bundle identity, and source-level native/view declarations |
| `core/vm/src` | 326 | 126,886 | Checking, lowering, runtime, world state, replay, provenance, FFI, and WASM |

The syntax extraction moved 199 frontend tests into an independently compiled
artifact. A clean no-default-feature VM test compile fell from about 131 to 95
seconds, while observed rustc private memory fell from about 1,448 to 1,316 MiB.
`cargo llvm-lines` showed 1,998,464 LLVM lines across 58,909 items and no generic
copy hotspot among the largest functions, so the boundary addresses compiler
arena breadth rather than masking a monomorphization leak with profile flags.

The model-checker implementation is split into property generators, temporal
observation, and campaign execution. The interpreter separates fuel/memory/
sandbox guards from frame dispatch. The live line-limit gate passes with no new
exception; exact justified exceptions remain limited to generated, frozen, or
indivisible artifacts listed in `tooling/line-limit-exceptions.tsv`.

## Audit contract

When a source audit changes behavior or structure:

1. Map definitions and callers with `rg.exe` before moving code.
2. Keep one semantic owner for tables, canonical names, effect policy, and
   declaration routing. Parallel projections must share an exhaustive helper
   or be compiler-enforced exhaustive matches.
3. Split by responsibility and dependency direction, not merely by line count.
   A thin module root is expected after a split and must not be documented as
   if it still contains the implementation.
4. Update the closest normative guide/reference page and the repository map.
5. Run the architecture, line-limit, formatting, test, and documentation gates.

```powershell
rg.exe --files core/syntax/src core/vm/src -g "*.rs"
python tooling/check_architecture.py
python tooling/check_line_limits.py
cargo fmt --all -- --check
cargo test -p rad-syntax
cargo test -p rad-vm --no-default-features
python tooling/scripts/check_doc_links.py
mdbook build docs
```

Deleted files are recoverable from Git history and should not remain as fake
live rows in this page. Historical benchmark/test counts belong in the
[Changelog](changelog.md), where their release context is explicit.

## Documentation targets

- Syntax and semantics: `docs/src/reference/spec.md` and the relevant guide.
- Builtins and embedding APIs: `docs/src/reference/builtins.md`.
- Runtime architecture: `docs/src/reference/architecture.md`.
- Memory and value layout: `docs/src/reference/memory-model.md`.
- WASM compiler/runtime path: `docs/src/reference/wasm-phase3.md`.
- Project/repo structure: `docs/src/project/repo-map.md`,
  `docs/src/project/folder_tree.md`, and `docs/src/project/repo-audit.md`.
