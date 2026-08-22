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

Snapshot captured from the working tree on **2026-08-22**. These numbers are a
dated receipt, not values that future edits should treat as live constants.

| Scope | Rust files | Lines | File-local 250-line chunks |
|---|---:|---:|---:|
| root files | 65 | 17,759 | 111 |
| `ast/` | 5 | 1,423 | 8 |
| `builtins/` | 3 | 2,076 | 9 |
| `causality/` | 7 | 1,832 | 11 |
| `checker/` | 52 | 25,752 | 130 |
| `compiler/` | 38 | 13,287 | 73 |
| `constraint_types/` | 2 | 1,009 | 5 |
| `ffi/` | 7 | 2,303 | 13 |
| `host_value/` | 2 | 1,227 | 6 |
| `internal_tests/` | 11 | 5,169 | 27 |
| `lexer/` | 7 | 2,902 | 15 |
| `linter/` | 2 | 847 | 4 |
| `merge/` | 2 | 1,273 | 7 |
| `module_loader/` | 4 | 2,491 | 11 |
| `parser/` | 15 | 6,328 | 33 |
| `relation/` | 16 | 5,113 | 28 |
| `replay/` | 3 | 1,804 | 8 |
| `sandbox/` | 3 | 1,534 | 7 |
| `types/` | 6 | 1,872 | 10 |
| `value/` | 5 | 2,332 | 12 |
| `vm/` | 69 | 28,148 | 148 |
| `wasm/` | 4 | 2,127 | 10 |
| `wire/` | 3 | 1,154 | 6 |
| `world/` | 12 | 4,621 | 24 |
| **Total** | **343** | **134,383** | **716** |

At capture time the largest Rust source was 999 lines, so no source exceeded
the repository's 1,000-line gate.

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
rg.exe --files core/vm/src -g "*.rs"
python tooling/check_architecture.py
python tooling/check_line_limits.py
cargo fmt --all -- --check
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
