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
- **Frozen experiment**: preserved code that is not part of normal development.
- **Generated**: ignored local output; do not commit it.

## Top Level

| Path | Status | Notes |
|---|---|---|
| `.github/` | Tooling | CI, issue templates, PR template, and release notes template. |
| `adapters/` | Tooling | CLI, LSP, and WebGPU host boundaries that depend on the core VM. |
| `benches/` | Validation | Stress programs, bootstrap benchmarks, profile comparisons, and external baselines. |
| `core/` | Active core | Authoritative language/runtime implementation only. |
| `docs/` | Active docs | mdBook source and theme. |
| `examples/` | Active project | Canonical small examples and host examples. |
| `experiments/` | Frozen experiment | Non-authoritative implementation research. |
| `projects/` | Active project | Larger dogfood apps, playground, and tutorials. |
| `tests/` | Validation | Snapshot/conformance tests, focused feature fixtures, and manual checks. |
| `tooling/` | Tooling | Editor support, helper scripts, and `rad new` templates. |
| `target/` | Generated | Cargo build output; ignored. |

Removed top-level shapes: `vm/`, `compiler/`, `simcore/`, `docs-site/`,
`labs/`, `benchmarks/`, `editors/`, `scripts/`, `templates/`, and
`playground/`. New work should not recreate them.

## Core

For the current audit contract, dated SRP snapshot, and live validation
commands, see the [Core VM Source Audit](core-vm-source-audit.md). The
machine-checked directory inventory is the [Folder Tree](folder_tree.md).

| Path | Status | Notes |
|---|---|---|
| `core/syntax/` | Active core | Independently compiled lexer, parser, AST, source bundle, and source-level native/view declarations. |
| `core/vm/` | Active core | Rust library for checking, bytecode, VM, world state, replay, relation runtime, and WASM bindings. |
| `core/vm/src/builtins/` | Active core | Builtin signatures, effects, and type schemes. |
| `core/vm/src/causality/` | Active core | Provenance storage, explanation, and retention behavior. |
| `core/vm/src/checker/` | Active core | Static analysis and type checking. |
| `core/vm/src/compiler/` | Active core | Rust AST-to-bytecode compiler. |
| `core/vm/src/ffi/` | Active core | ABI contracts, plugin generations, verification, containment, and host-call records. |
| `core/vm/src/host_value/` | Active core | Owned host/VM value boundary. |
| `core/vm/src/module_loader/` | Active core | Canonical module graph, aliases, lockfiles, and authenticated loading. |
| `core/vm/src/relation/` | Active core | Relation front end, authoritative store, and derivation under one bounded context. |
| `core/vm/src/replay/` | Active core | Deterministic trace encoding, identity, execution, and tests. |
| `core/vm/src/sandbox/` | Active core | Capability isolation and speculative guest execution. |
| `core/vm/src/types/` | Active core | Semantic input/product and type-system support. |
| `core/vm/src/value/` | Active core | NaN-boxed representation, builtin identity, and value operations. |
| `core/vm/src/vm/` | Active core | Bytecode VM internals and builtins. |
| `core/vm/src/wasm/` | Active core | Browser runtime/session API and rendering bridge. |
| `core/vm/src/wire/` | Active core | Canonical world/delta codec and validation. |
| `core/vm/src/world/` | Active core | ECS storage, indexes, views, snapshots, and operations. |
| `core/vm/src/internal_tests/` | Validation | Cross-module tests that require crate-private state. |
| `core/vm/tests/` | Validation | Rust integration tests for the VM crate. |
| `core/vm/benches/` | Validation | Criterion benchmarks tied to the VM crate. |
| `core/vm/scripts/` | Tooling | VM-specific doc/example helper scripts. |
| `experiments/c-backend/` | Frozen experiment | Historical C/AOT backend experiment. Not authoritative; see [C Backend Freeze](c-backend-freeze.md). |
| `experiments/c-backend/src/` | Frozen experiment | Rad compiler sources plus C runtime support files, preserved but not maintained. |
| `experiments/c-backend/repro/` | Frozen experiment | Historical reproductions only. |
| `projects/moba/simcore/` | Active project | Rust/native/wasm simulation core owned by the MOBA project. |
| `projects/moba/simcore/src/` | Active project | Project-specific accelerated implementation. |
| `projects/moba/simcore/tests/` | Validation | Golden corpus tests. |

## Docs

| Path | Status | Notes |
|---|---|---|
| `docs/src/` | Active docs | mdBook source root. |
| `docs/src/examples/` | Active docs | Narrative docs for examples and dogfood projects. |
| `docs/src/getting-started/` | Active docs | Installation and first-use docs. |
| `docs/src/guide/` | Active docs | Language guide. |
| `docs/src/project/` | Active docs | Changelog, roadmap, repo map, audit, contributing, and RFC process. |
| `docs/src/reference/` | Active docs | Canonical spec, builtins, architecture, memory model, performance, and host API references. |
| `docs/theme/` | Active docs | mdBook theme overrides. |
| `docs/book/` | Generated | mdBook output; ignored. |

## Projects

The [Folder Tree](folder_tree.md) is the exhaustive directory inventory. This
table records the project-level boundaries that carry architecture or release
status; it deliberately does not duplicate every dogfood subdirectory.

| Path | Status | Notes |
|---|---|---|
| `projects/dogfood/` | Active project | Forty-two production, research, regression, and teaching applications. See the folder tree for the complete list. |
| [Production portfolio](production-dogfood-portfolio.md) | Validation | PagerGrid, FulfillOS, ClearPay, ForgeLink, MarketLens, BookCore, Dispatch60, MatchFlow, AccessLens, WorkPulse, and RiskBridge cumulatively accept the eleven production-boundary features. |
| `projects/dogfood/native-math-kernels/` | Active project | Project-owned native acceleration and extension ABI dogfood. |
| `projects/moba/kit/` | Active project | MOBA simulation corpus, generated content, kit modules, tools, and golden receipts. |
| `projects/moba/simcore/` | Active project | Rust/native/WASM simulation core with golden tests. |
| `projects/moba/vertical-slice/` | Active project | Networked MOBA stack: RAD authority, shared protocol, Rust WebTransport edge proxy, and browser client. |
| `projects/moba/vertical-slice/protocol/` | Active project | Canonical wire contract and binding generator. |
| `projects/moba/vertical-slice/client/` | Active project | Vite + TypeScript + Three.js client owning input, prediction, reconciliation, and rendering. |
| `projects/moba/vertical-slice/client/test/` | Validation | Node-based unit tests for the client netcode and transport modules. |
| `projects/moba/vertical-slice/docs/` | Active docs | Stack-local overview, runbook, protocol ownership, and netcode notes. |
| `projects/moba/vertical-slice/server/` | Active project | RAD authority server: simulation, transport-owned packet grammar, validation, snapshots, and replay. |
| `projects/moba/vertical-slice/server/src/test/` | Validation | `.rad` smoke suites run via `npm test`. |
| `projects/moba/vertical-slice/edge-proxy/` | Active project | Rust WebTransport/HTTP3 terminator. Deliberately outside the root Cargo workspace so QUIC dependencies stay out of the main build. |
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
directories such as `target/`, `docs/book/`, `experiments/c-backend/target/`,
`core/vm/pkg/`, and `projects/playground/pkg*/`.
