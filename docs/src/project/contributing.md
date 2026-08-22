# Contributing to Rad

Thanks for your interest in contributing to Rad! This guide covers everything you need to get started.

## Prerequisites

- **Rust stable** (latest) — for the bytecode VM and `rad` CLI
- **Python 3.10+** — for helper scripts (optional)

On Windows, initialize the checkout once from PowerShell:

```powershell
./devtools/setup-windows-rust.ps1
```

RAD uses the `stable-x86_64-pc-windows-gnu` Rust host and a MinGW-w64 x64 GCC
linker on Windows. The script installs the Rust host when needed, validates GCC,
and records a rustup directory override, so every plain `cargo` command below
this checkout uses the same ABI regardless of a global MSVC default or an
unrelated `link.exe` earlier on `PATH`. Linux and macOS continue to use their
native stable host toolchains.

## Quick Start

This repository is a **Cargo workspace**: build artifacts go to **`target/` at the repo root** (not under `core/vm/target/`).

```bash
# Build the Rad CLI
cargo build -p rad-cli

# Run a .rad file (use target\debug\rad.exe on Windows)
target/debug/rad examples/demo.rad

# Run all tests
cargo test -p rad-vm
target/debug/rad snapshot tests/
```

## Project Structure

For the shorter mental model, start with the [Repository Map](repo-map.md).

```text
core/vm/                    # authoritative Rust language/runtime library
  src/
    lexer/ parser/          # source front end
    checker/ compiler/      # semantic checking and bytecode lowering
    vm/ world/ relation/    # execution and world models
    internal_tests/         # tests that genuinely need private state

adapters/
  cli/                      # `rad` executable and process I/O
  lsp/                      # Language Server Protocol transport
  webgpu/                   # browser/GPU presentation materializer

projects/
  dogfood/                  # cross-boundary workloads
  moba/                     # one MOBA ownership namespace
  playground/ tutorial/     # browser host and teaching projects

experiments/c-backend/      # frozen, non-authoritative C/AOT experiment
tests/conformance/          # canonical language behavior
docs/                       # RFC authority and mdBook source
tooling/                    # gates, editor support, scripts, templates
```

### Projects and local experiments

Checked-in experiments that are still active live under `projects/` and should be source, fixtures, and scripts only. Ad-hoc local scratch files should stay outside the repo or in a gitignored local directory; anything promoted to `examples/`, `tests/conformance/`, or templates should follow the same conventions as the rest of the tree.

### Entry `main` — explicit `-> Type`

For zero-argument **`main`**, declare an explicit return type (typically **`fn main() -> nil`** for void entrypoints). See the [Language Spec](../reference/spec.md). CI fails if a bare **`fn main() {`** appears under **`examples/`**, **`tooling/templates/`**, **`tests/conformance/`**, or **`benches/`** (see `.github/workflows/ci.yml`).

## Source-to-Wiki Audits

The wiki source in `docs/src/` is the single source of truth. A source audit
should update those pages directly; do not add loose `TECH_NOTES.md`,
`AUDIT.md`, scratch design notes, or temporary Markdown files elsewhere in the
repository.

Recommended audit loop for large files:

1. Read the source in chunks of about 250 lines.
2. For each chunk, record only facts that change user-facing docs, reference
   semantics, architecture, tests, or project status.
3. Update the closest existing docs page in the same branch:
   - language behavior -> `docs/src/reference/spec.md` or the relevant
     guide page
   - builtins and embedding APIs -> `docs/src/reference/builtins.md`
   - runtime/backend architecture -> `docs/src/reference/architecture.md`
     or [Repository Map](repo-map.md)
   - dogfood/project behavior -> `docs/src/examples/` or the matching guide
   - repo layout/status -> [Repository Map](repo-map.md) or
     [Repository Audit](repo-audit.md)
4. If a new docs page is truly needed, add it under `docs/src/`, link it from
   `docs/src/SUMMARY.md`, and link it from the nearest existing page.
5. Keep source receipts precise with paths, function names, command names, or
   test names. Avoid dumping long code excerpts into docs.

When an audit finds that code already uses a language feature, update the
feature's canonical guide/reference page and any affected project page. The
Orianna example keeps its runtime path and feature ledger in
[Game Embedding & MOBA Dogfood](../guide/game-embedding.md).

## Adding a New Language Feature

Every feature must preserve Rust VM runtime behavior and language semantics. Follow this checklist:

### The compiler-pipeline checklist (Rust)

1. **`core/vm/src/lexer.rs`** — Add new tokens if needed
2. **`core/vm/src/ast.rs`** — Add/modify AST nodes
3. **`core/vm/src/parser/`** — Parse the new syntax into AST
4. **`core/vm/src/checker/`** — Add type checking / static analysis
5. **`core/vm/src/compiler/`** — Emit bytecode for the feature
6. **`core/vm/src/vm/`** — Handle runtime behavior when needed

Host-only behavior belongs in the matching adapter. Project-specific behavior
belongs in its project. Do not make `core/vm` depend on either one.

### Required for Every Feature

- [ ] Conformance test(s) in `tests/conformance/` for expected runtime behavior
- [ ] Rust unit tests in the relevant `#[cfg(test)]` module
- [ ] Update the [Language Spec](../reference/spec.md) with the new syntax/semantics
- [ ] All existing tests still pass: `cargo test -p rad-vm` (from the repo root) and `rad snapshot tests/` using `target/debug/rad`

## Adding a Conformance Test

Conformance tests are `.rad` files in `tests/conformance/` with special comment directives:

```rad
// backend: rust           ← run through the Rust VM (default)

let x = 42
print(x)
// expect: 42             ← expected stdout line

// For error tests:
// expect-runtime-error: Some error message
```

Run `rad snapshot tests/conformance/` (or `rad snapshot tests/`) to execute conformance fixtures against `.snap` baselines.

## Running Tests

```bash
# Rust unit tests (includes checker, compiler, parser, VM tests)
cargo test -p rad-vm

# Conformance / snapshot tests (from repo root, after `cargo build -p rad-cli`)
target/debug/rad snapshot tests/

# Update baselines after intentional output changes
target/debug/rad snapshot tests/ --update

# Rust clippy (linting)
cargo clippy --workspace --all-targets -- -D warnings

# Repository ownership and 1,000-line SRP policy
python tooling/check_architecture.py
python tooling/check_line_limits.py

# Benchmarks
cargo bench -p rad-vm
py benches/compare.py
```

### Frozen C backend

`experiments/c-backend/` is frozen experimental code. It is not part of normal feature work,
not a release health gate, and not a source of truth for language behavior. See
[C Backend Freeze](c-backend-freeze.md).

Do not patch C-backend warnings or failures while developing `core/vm`. The old
harnesses remain only for archaeology and possible future revival, and require
`RAD_RUN_FROZEN_C_BACKEND=1` before they run.

Historical harness entry points:

```bash
RAD_RUN_FROZEN_C_BACKEND=1 py experiments/c-backend/test_conformance_c.py
RAD_RUN_FROZEN_C_BACKEND=1 py experiments/c-backend/test_c_backend.py
```

Do not run these commands as part of normal project health. Historical flags
include `--verbose`, `--filter PATTERN`, `--jobs N`, `--keep-artifacts`,
`--no-progress`, `--force-progress`, and `--compiler {gcc,tcc,auto}`.

- **`--compiler auto`** (the default) uses **TCC** ([Tiny C Compiler](https://bellard.org/tcc/)) if found on `PATH`, falling back to GCC. TCC compiles the full 176-test suite in ~5 s vs ~30 s with GCC on Windows.
- **`--compiler gcc`** forces GCC. On **Windows**, if `gcc` fails with exit code 1 and **no stderr**, ensure the **`bin/` directory containing `gcc.exe`** is **first** on `PATH` (so `cc1.exe` loads the matching MinGW DLLs).


- **Frozen status:** no C-backend source is currently maintained for warning
  cleanliness, strict typing, or feature parity. `core/vm` is the only active
  implementation.

Generated C output is treated as a build artifact and should not be checked in.

## Code Style

- **Rust**: Follow standard `rustfmt` conventions. Zero clippy warnings.
- **Python**: Standard PEP 8. Keep tooling and test helpers stdlib-only where practical.
- **Comments**: Explain *why*, not *what*. Avoid obvious comments.
- **Error messages**: Keep runtime error wording stable and consistent.

## Runtime Behavior Contract

Rad ships a single runtime backend (the Rust VM). All language semantics are defined by the VM:

- System execution order is deterministic (topological sort)
- State machine transitions produce consistent errors
- Event handling is double-buffered: `emit` defers work; a flush drains the pending batch in enqueue order, and handlers for a given event run in registration order
- Container values use value semantics (copy-on-bind)

Conformance tests in `tests/conformance/` encode these invariants.

## Good First Issues

Look for issues labeled `good-first-issue`. Great starting points:

- Adding a new builtin function
- Writing conformance tests for edge cases
- Improving error messages with better hints
- Adding examples that demonstrate language features
- Documentation improvements

Use the [Good First Issue template](https://github.com/peteracs/rad/issues/new?template=good_first_issue.yml) when filing new ones — it includes a "files to touch" section to help newcomers navigate the codebase.

## Proposing Language Changes

For changes that affect syntax, semantics, or the type system:

1. **Start a discussion** — open a [Language Design Discussion](https://github.com/peteracs/rad/issues/new?template=language_design.yml) to gauge interest
2. **Write an RFC** — once the idea is fleshed out, copy the [RFC template](rfc-template.md) and open a PR
3. **Implement** — after the RFC is accepted, follow the compiler-pipeline checklist above

See the [RFC process](rfcs.md) and the [public roadmap](roadmap.md) for what's planned and where to focus.

## Pull Requests

All PRs use the repository pull request template in `.github/PULL_REQUEST_TEMPLATE.md`, which includes the full test/doc checklist. Key requirements:

- `cargo test` and conformance tests pass
- Zero clippy warnings
- New behavior has conformance tests
- Language spec updated if syntax/semantics changed
- [Changelog](changelog.md) updated under `[Unreleased]`

## Questions?

Open an issue or start a discussion. We're happy to help!
