# Repository Audit

This page records the current status of every living top-level directory and
major subdirectory after the repo cleanup. If a folder is not listed here, it is
not part of the intended source layout.

Status key:

- **Active core**: implementation of Rad itself.
- **Active project**: software built with Rad, used as dogfood or examples.
- **Active docs**: canonical documentation source.
- **Validation**: automated or manual checks.
- **Tooling**: support code that should not define language semantics.
- **Frozen legacy**: preserved code that is not part of normal development.
- **Generated**: ignored local output; do not commit it.

## Top Level

| Path | Status | Notes |
|---|---|---|
| `.github/` | Tooling | CI, issue templates, PR template, and release notes template. |
| `benches/` | Validation | Stress programs, bootstrap benchmarks, profile comparisons, and external baselines. |
| `core/` | Active core | Language/runtime implementation and core-adjacent Rust crates. |
| `docs/` | Active docs | mdBook source and theme. |
| `examples/` | Active project | Canonical small examples and host examples. |
| `projects/` | Active project | Larger dogfood apps, playground, and tutorials. |
| `tests/` | Validation | Snapshot/conformance tests, focused feature fixtures, and manual checks. |
| `tooling/` | Tooling | Editor support, helper scripts, and `rad new` templates. |
| `target/` | Generated | Cargo build output; ignored. |

Removed top-level shapes: `vm/`, `compiler/`, `simcore/`, `docs-site/`,
`labs/`, `benchmarks/`, `editors/`, `scripts/`, `templates/`, and
`playground/`. New work should not recreate them.

## Core

For the 250-line review tracker covering every file under `core/vm/src`, see
the [Core VM Source Audit](core-vm-source-audit.md).

| Path | Status | Notes |
|---|---|---|
| `core/vm/` | Active core | Rust crate for the CLI, VM, checker, compiler, formatter, LSP, WASM bindings, and tests. |
| `core/vm/src/checker/` | Active core | Static analysis and type checking. |
| `core/vm/src/compiler/` | Active core | Rust AST-to-bytecode compiler. |
| `core/vm/src/lexer/` | Active core | Lexer submodules. |
| `core/vm/src/parser/` | Active core | Parser submodules. |
| `core/vm/src/vm/` | Active core | Bytecode VM internals and builtins. |
| `core/vm/tests/` | Validation | Rust integration tests for the VM crate. |
| `core/vm/benches/` | Validation | Criterion benchmarks tied to the VM crate. |
| `core/vm/scripts/` | Tooling | VM-specific doc/example helper scripts. |
| `core/c-backend/` | Frozen legacy | Historical C/AOT backend experiment. Not authoritative; see [C Backend Freeze](c-backend-freeze.md). |
| `core/c-backend/src/` | Frozen legacy | Rad compiler sources plus C runtime support files, preserved but not maintained. |
| `core/c-backend/repro/` | Frozen legacy | Historical reproductions only. |
| `core/simcore/` | Active core | Rust/native/wasm simulation core used by the MOBA dogfood path. |
| `core/simcore/src/` | Active core | Sim core implementation. |
| `core/simcore/tests/` | Validation | Golden corpus tests. |

## Docs

| Path | Status | Notes |
|---|---|---|
| `docs/src/` | Active docs | mdBook source root. |
| `docs/src/examples/` | Active docs | Narrative docs for examples and dogfood projects. |
| `docs/src/getting-started/` | Active docs | Installation and first-use docs. |
| `docs/src/guide/` | Active docs | Language guide. |
| `docs/src/project/` | Active docs | Changelog, roadmap, repo map, audit, contributing, and RFC process. |
| `docs/src/reference/` | Active docs | Spec, builtins, architecture, memory model, performance, and compatibility docs. |
| `docs/theme/` | Active docs | mdBook theme overrides. |
| `docs/book/` | Generated | mdBook output; ignored. |

## Projects

| Path | Status | Notes |
|---|---|---|
| `projects/dogfood/` | Active project | Larger Rad applications used to pressure-test language features. |
| `projects/dogfood/budget/` | Active project | Budget dogfood app. |
| `projects/dogfood/causality/` | Active project | Provenance/why dogfood app. |
| `projects/dogfood/deathsight/` | Active project | Browser/game dogfood source. |
| `projects/dogfood/moba/` | Active project | MOBA simulation dogfood. |
| `projects/dogfood/moba/gen/` | Active project | Checked-in generated Rad content used by the MOBA corpus. |
| `projects/dogfood/moba/kit/` | Active project | Champion/ability kit modules. |
| `projects/dogfood/moba/tools/` | Tooling | Generators for MOBA content. |
| `projects/dogfood/opsdesk/` | Active project | Operations desk dogfood app. |
| `projects/dogfood/orianna_gui/` | Active project | Orianna browser arena source. |
| `projects/dogfood/radgui/` | Active project | Generic GUI dogfood source. |
| `projects/dogfood/radgui/targets/` | Active project | RADGUI app targets. |
| `projects/dogfood/radsheet/` | Active project | Spreadsheet dogfood app. |
| `projects/dogfood/radsheet/demo/` | Active project | Demo scripts/state fixtures; runtime state is ignored. |
| `projects/dogfood/radsheet/incident/` | Active project | Incident reproduction source. |
| `projects/dogfood/radtrack/` | Active project | Offline-first tracker dogfood app. |
| `projects/dogfood/radtrack/demo/` | Active project | Demo fixtures; runtime state is ignored. |
| `projects/dogfood/schema/` | Active project | Schema/migration dogfood app. |
| `projects/dogfood/speculation/` | Active project | Fork/simulate dogfood app. |
| `projects/dogfood/sudoku/` | Active project | Sudoku dogfood app. |
| `projects/dogfood/sudoku/data/` | Active project | Sudoku data fixtures. |
| `projects/dogfood/syncdesk/` | Active project | Sync/network dogfood app. |
| `projects/dogfood/tactics/` | Active project | Tactics dogfood app. |
| `projects/dogfood/tactics/bots/` | Active project | Tactics bot modules. |
| `projects/dogfood/timetravel/` | Active project | Replay/time-travel dogfood app. |
| `projects/dogfood/todo/` | Active project | Todo dogfood app. |
| `projects/dogfood/worldmerge/` | Active project | Merge/conflict dogfood app. |
| `projects/moba-rad/` | Active project | Networked MOBA dogfood stack: RAD authority server, Rust WebTransport edge proxy, and a browser client. |
| `projects/moba-rad/client/` | Active project | Vite + TypeScript + Three.js client owning input, prediction, reconciliation, and rendering. |
| `projects/moba-rad/client/src/netcode/` | Active project | Prediction, reconciliation, and ack/diagnostic logic. |
| `projects/moba-rad/client/test/` | Validation | Node-based unit tests for the client netcode and transport modules. |
| `projects/moba-rad/docs/` | Active docs | Stack-local overview, runbook, protocol ownership, and netcode notes. |
| `projects/moba-rad/server/` | Active project | RAD authority server: simulation, packet grammar, validation, snapshots, and replay. |
| `projects/moba-rad/server/src/test/` | Validation | `.rad` smoke suites run via `npm test`. |
| `projects/moba-rad/server/edge-proxy/` | Active project | Rust WebTransport/HTTP3 terminator. Deliberately outside the root Cargo workspace so QUIC dependencies stay out of the main build. |
| `projects/playground/` | Active project | Browser playground, hosts, demos, relay, and JS tests. |
| `projects/playground/demos/` | Active project | Standalone browser visual prototypes and their local assets. |
| `projects/playground/relay/` | Tooling | WebSocket relay for collaborative playground tests. |
| `projects/playground/test/` | Validation | Node-based playground session tests. |
| `projects/tutorial/` | Active project | Tutorial source projects. |
| `projects/tutorial/task-board/` | Active project | Collaborative task board tutorial. |
| `projects/tutorial/task-board/03_replay/` | Active project | Replay checkpoint assets for the tutorial. |

## Validation

| Path | Status | Notes |
|---|---|---|
| `benches/baselines/` | Validation | External baseline harnesses and results. |
| `benches/baselines/collab/` | Validation | Collaboration benchmark baselines. |
| `benches/baselines/micro/` | Validation | Microbenchmark baselines. |
| `tests/conformance/` | Validation | Snapshot-backed language behavior tests. |
| `tests/conformance/modules/` | Validation | Module/import conformance fixtures. |
| `tests/conformance/modules/adversarial_graph/` | Validation | Import graph stress fixtures. |
| `tests/features/` | Validation | Focused feature attack/regression fixtures. |
| `tests/features/destructure/` | Validation | Destructuring feature fixtures. |
| `tests/manual/` | Validation | Manual checks that are not part of the automated contract. |

## Tooling

| Path | Status | Notes |
|---|---|---|
| `.github/ISSUE_TEMPLATE/` | Tooling | Structured issue templates. |
| `.github/workflows/` | Tooling | CI, benchmark, soundness, matrix, and playground workflows. |
| `tooling/editors/` | Tooling | Editor integrations. |
| `tooling/editors/vscode/` | Tooling | VS Code extension. |
| `tooling/editors/vscode/src/` | Tooling | VS Code extension implementation. |
| `tooling/editors/vscode/syntaxes/` | Tooling | TextMate grammar. |
| `tooling/scripts/` | Tooling | Repository helper scripts. |
| `tooling/templates/` | Tooling | `rad new` templates. |

## Policy

There is no intentional `archive/`, `deprecated/`, or `old/` source tree.
Local scratch belongs outside the repo or in ignored root folders such as
`scratch/` and `temp/`. Generated output belongs under ignored build/output
directories such as `target/`, `docs/book/`, `core/c-backend/target/`,
`core/vm/pkg/`, and `projects/playground/pkg*/`.
