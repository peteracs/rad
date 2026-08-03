# Repository Map

This repository is organized around five living areas: the Rad language core,
documentation, programs built with Rad, validation, and supporting tooling.
When a file does not clearly fit one of those areas, it probably should not be
checked in. For a folder-by-folder status ledger, see the
[Repository Audit](repo-audit.md).

## Language Core

The core is the implementation of Rad itself.

### Core implementation roles

These names are easy to confuse because they all sit under `core/`, but they
serve different jobs:

| Name | Role | When it is used |
|---|---|---|
| `core/vm/` | Primary Rad implementation: native `rad` CLI, parser, checker, bytecode compiler, VM, formatter, LSP, snapshot runner, and WASM `RadRuntime`. | Normal language development, CLI runs, tests, playground sessions, browser embeds, and documentation receipts. |
| `core/c-backend/` | Frozen legacy C/AOT experiment. It is preserved for history and possible future revival, but it is not authoritative. | Do not use for normal development or health checks. See [C Backend Freeze](c-backend-freeze.md). |
| `core/simcore/` | Specialized Rust/native/WASM simulation kernel for the MOBA damage core, mirrored against the debuggable Rad spec. | Hot-path MOBA damage batches and golden-corpus checks. It is not the general Rad language runtime. |

| Path | Purpose |
|---|---|
| `core/vm/` | Rust implementation of the Rad CLI, parser, checker, bytecode compiler, VM, formatter, LSP, WASM bindings, and snapshot runner. |
| `core/vm/src/lexer.rs`, `core/vm/src/lexer/` | Tokenization. |
| `core/vm/src/parser.rs`, `core/vm/src/parser/` | Syntax parsing. |
| `core/vm/src/ast.rs` | Shared AST definitions. |
| `core/vm/src/checker/` | Static analysis and type checking. |
| `core/vm/src/compiler/` | AST-to-bytecode lowering inside the Rust VM. |
| `core/vm/src/vm/` | Bytecode execution and builtin implementation. |
| `core/vm/src/{parser,checker,compiler}/causal.rs`, `core/vm/src/vm/settlement.rs` | Experimental RFC-0001/RFC-0002 Causal Laws front end, settlement kernel, and candidate validation. |
| `core/vm/src/{constraint_types,constraint_reference}.rs` | Pointer-free rejection contracts, versioned limits, attempt replay data, and the pure constraint oracle. |
| `core/vm/src/boolean_lattice.rs` | Exact finite OR-closure, frequency, separation, signature, and closure-audit kernels used by computational-mathematics workloads. |
| `core/vm/src/causality/settlement.rs` | Settlement/proposal/resolution fan-in provenance and `why()` rendering. |
| `core/vm/src/value.rs`, `core/vm/src/world.rs` | Runtime values and ECS world storage. |
| `core/c-backend/` | Frozen legacy C backend, with source, runtime, old harnesses, and reproductions kept out of normal health checks. |
| `core/simcore/` | Native/wasm Rust sim core used by the MOBA dogfood path. |

Generated compiler output belongs under ignored build directories such as
`target/` or `core/c-backend/target/`, not in source control.

## Documentation

The canonical documentation lives in `docs/`.

| Path | Purpose |
|---|---|
| `README.md` | Short entry point and quick build instructions. |
| `docs/src/SUMMARY.md` | mdBook table of contents. |
| `docs/src/guide/` | User-facing language guide. |
| `docs/src/reference/` | Spec, builtins, architecture, memory model, performance, and compatibility references. |
| `docs/src/examples/` | Narrative docs for examples and dogfood projects. |
| `docs/src/project/` | Changelog, roadmap, contributing guide, RFC process, and repository map. |
| `docs/theme/` | mdBook theme overrides. |

Source discoveries must update the relevant page under `docs/src/`; do not add
one-off notes elsewhere in the repository. If a new page is truly needed, add
it to `docs/src/SUMMARY.md` in the same change and link it from the closest
existing guide/reference/example page.

## Programs Built With Rad

These folders are consumers of the language. They are useful examples and
dogfood projects, but they are not the language implementation.

| Path | Purpose |
|---|---|
| `examples/` | Canonical example `.rad` programs and host examples. |
| `projects/dogfood/` | Larger dogfood applications and feature stress projects. |
| `projects/dogfood/causal-laws/` | RFC-0001 damage settlement vertical slice. |
| `projects/dogfood/causal-constraints/` | RFC-0002 movement validation commit/rejection dogfood. |
| `projects/dogfood/frankl-search/` | Computational-mathematics dogfood: exhaustive small case, deterministic `N=13` generator search, exact cyclic-universe/deletion-obstruction study, causal explanations, and independent certificate verifiers. |
| `projects/dogfood/collatz-lab/` | Structural Collatz dogfood: pruned affine residue universes, exact cycle-word equations, Causal Laws/constraints/causality/replay, and a Python-bigint certificate verifier. |
| `projects/moba-rad/` | Networked MOBA dogfood stack: RAD authority server, Rust WebTransport edge proxy, and browser client. The authority owns all game rules; the proxy forwards opaque datagrams and must stay dumb. |
| `projects/playground/` | Browser hosts, interactive demos, relay, JS session tests, and public playground shell. |
| `projects/playground/demos/` | Standalone browser visual prototypes and their local assets. |
| `projects/tutorial/` | Tutorial projects. |

## Validation

Validation proves that the language behavior is stable.

| Path | Purpose |
|---|---|
| `tests/conformance/` | Snapshot-backed language behavior tests. |
| `tests/features/` | Focused feature attack/regression fixtures. |
| `tests/manual/` | Manual local checks that are not part of the automated contract. |
| `core/vm/src/*tests*.rs` | Rust unit, property, composition, migration, and fuzz-oriented tests. |
| `projects/moba-rad/client/test/` | Node unit tests for client netcode, prediction, and transport. |
| `projects/moba-rad/server/src/test/` | `.rad` smoke suites for the authority server. |
| `benches/` | Stress inputs, benchmark harnesses, and external baselines. |

## Tooling

Tooling supports development but should not carry core semantics.

| Path | Purpose |
|---|---|
| `.github/` | CI, soundness, playground deploy, issue templates, and PR template. |
| `tooling/editors/` | Editor integration such as the VS Code extension. |
| `tooling/scripts/` | Repo helper scripts. |
| `tooling/templates/` | `rad new` templates. |

If something is generated, stale, or only useful for one local debugging
session, keep it out of the repo.
