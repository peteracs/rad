# Rad Language Roadmap

> Last verified against the source tree: 2026-08-23
>
> Rad is pre-release. “Shipped” means implemented in the Rust VM/CLI and
> covered by repository tests and dogfood; it is not a 1.0 stability promise.

Open an [RFC](rfcs.md) or a Language Design Discussion on GitHub for proposed
language changes. Benchmark numbers live in the performance pages rather than
this roadmap so a structural status table cannot turn into a stale performance
claim.

## Status key

| Status | Meaning |
|---|---|
| **Shipped** | Implemented and exercised by the current repository |
| **In progress** | An implementation exists, but the stated production contract is incomplete |
| **Needs RFC** | No accepted design or production implementation |

## Production-boundary language features

The eleven cumulative acceptance projects are documented in the
[Production Dogfood Portfolio](production-dogfood-portfolio.md). The normative
syntax and guarantees live in
[Authority, Transactions, and Native Data](../guide/authority-transactions-and-native-data.md)
and
[Cost, Phases, Provenance, Models, and FFI](../guide/cost-phases-provenance-models-and-ffi.md).

| Status | Feature | Production acceptance |
|---|---|---|
| **Shipped** | Transitive read/write/emit/IO/async authority, call-site-specialized system effects, scheduler integration, and `effects`/`writers`/`readers`/`path` inspection | PagerGrid |
| **Shipped** | Canonical component/resource/field write ownership, co-owners, scoped test authority, and explicit transfer | FulfillOS |
| **Shipped** | Atomic transactions with `requires`, transitive `changes_only`, `ensures`, rollback, and post-commit effects | ClearPay |
| **Shipped** | Opaque nominal types, fixed-width scalars, enums, bitflags, ABI layout, offsets, and endian conversion | ForgeLink |
| **Shipped** | Transactionally maintained materialized views, dependency revisions, change feeds, and membership explanations | MarketLens |
| **Shipped** | Ordered indexes with bounded traversal and deterministic ordering contracts | BookCore |
| **Shipped** | Transitive hot-path cost plans, full-scan rejection, instruction budgets, and separate guest/runtime/host allocation contracts | Dispatch60 |
| **Shipped** | Explicit phase graphs, synchronous/next/phase delivery, reentrancy guards, trace assertions, and deterministic ordering | MatchFlow |
| **Shipped** | Field, removal, missing-value, view-membership, and revision provenance with explicit retention truncation evidence | AccessLens |
| **Shipped** | Stateful model checking, temporal properties, deterministic shrinking, and replayable `.radr` counterexamples through `rad model-check` and `rad shrink` | WorkPulse |
| **Shipped** | ABI-verified, effect-declared, generation-safe native host calls with containment and record/replay | RiskBridge |

## Core language and developer experience

| Status | Feature | Notes |
|---|---|---|
| **Shipped** | Canonical pattern ergonomics | Zero-field shorthand and match rest patterns |
| **Shipped** | Project templates | `rad new --template workflow\|stream\|simulation\|control-plane` |
| **Shipped** | ECS resources and indexed fields | Resource singletons, hash indexes, lookup, fork/snapshot support, conflict analysis |
| **Shipped** | Readonly and unique bindings | Read-only callable effects plus compile-time unique ownership |
| **Shipped** | Generic functions, structs, and type aliases | Checked across public module boundaries |
| **Shipped** | Canonical file modules | Recursive local/remote loading, aliases, visibility, cycle checks, one identity per normalized module path |
| **Shipped** | Reproducible module lockfile | `--write-lock` records byte counts, checksums, and SHA-256 pins in `forge.lock` |
| **Shipped** | Parser recovery and native formatter/linter/LSP | Multiple diagnostics, formatting, hover, definitions, completion |
| **Shipped** | Async and deterministic parallel execution | Async handlers, conflict-aware schedule batches, parallel simulation, serial overrides |
| **Shipped** | Fork/diff/merge/replay/sandbox | Copy-on-write speculation, field conflicts, deterministic traces, capability isolation |
| **Shipped** | WebGPU presentation host | Bounded packets, generated layout contract, device-loss recovery |
| **Shipped** | Relations and causal settlements | Experimental opt-in surfaces; see their RFCs and explicit feature flags |

## Open work

| Status | Item | Required next step |
|---|---|---|
| **In progress** | Production WASM compiler guest and footprint | Replace the Phase 3 diagnostic stub with a real checked guest, then measure and optimize the resulting bundle |
| **Needs RFC** | Package registry | Package identity, version resolution, signing, publishing, and lock semantics; file/URL modules already exist |
| **Needs RFC** | Standard library distribution | Versioned `std` modules and compatibility policy |
| **Needs RFC** | Native AOT compilation | A supported backend and parity plan; the old C backend is a frozen experiment |
| **Needs RFC** | Debug adapter protocol | VS Code stepping, breakpoints, and replay-aware state inspection |
| **Needs RFC** | Visual system graph | Editor view for authority, scheduler conflicts, phases, and query costs |
| **Needs RFC** | LSP refactoring operations | Rename, references, and code actions across canonical module identities |
| **Needs RFC** | Playground sharing | Stable shareable artifacts and editor completion in the browser |

## How to influence the roadmap

1. Open an RFC for syntax, semantics, wire formats, or compatibility contracts.
2. Open a Language Design Discussion for early use cases and tradeoffs.
3. Add a production-shaped dogfood and negative fixtures for behavior changes.
4. Include deterministic replay, performance, and operational inspection when
   the feature claims those properties.
