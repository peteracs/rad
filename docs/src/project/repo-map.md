# Repository Map

RAD is organized by authority: put a change in the directory that owns the
decision, state, or external boundary it implements. The exhaustive visual
directory inventory is the [Folder Tree](folder_tree.md); this page defines the
dependency rules behind that tree.

## Dependency direction

```text
projects ─────┐
adapters ─────┼──> core/vm
tests ────────┘

core/vm ──X──> adapters
core/vm ──X──> projects
```

`core/vm` is the authoritative language/runtime library. Adapters translate
that library into a process, protocol, editor, browser, or GPU environment.
Projects consume the language and may own specialized acceleration. Tests
verify the public contract. Experiments are non-authoritative.

The Rust workspace encodes the build side of this model. Cargo workspaces share
dependency resolution and build output, while separate packages keep host
adapters from becoming modules of the VM library.

## Top-level ownership

| Path | Owner and responsibility |
|---|---|
| `core/vm/` | RAD syntax, checking, bytecode, world model, execution, replay, persistence, WASM runtime boundary, and public Rust API. |
| `adapters/` | Host-facing composition that depends on the VM: CLI, LSP, and WebGPU materialization. |
| `projects/` | Applications, dogfood, tutorials, playgrounds, and project-owned acceleration. |
| `experiments/` | Frozen or exploratory implementations that are not language authority. |
| `tests/` | Implementation-independent language fixtures and repository-level contracts. |
| `examples/` | Small canonical programs and embedding examples. |
| `docs/` | Canonical RFC archive and mdBook source. |
| `tooling/` | Repository checks, editor support, generators, and project templates. |
| `benches/` | Bench workloads, comparison harnesses, and external baselines. |
| `.github/` | CI, soundness, deployment, and contribution workflows. |

Generated output belongs in ignored build directories such as `target/`,
`dist/`, or `demo-dist/`, never beside authoritative source.

## Core VM bounded contexts

The familiar compilation flow remains inside the core:

```text
source -> lexer -> parser -> AST -> checker -> compiler -> bytecode -> VM
```

| Path | Responsibility |
|---|---|
| `core/vm/src/lexer.rs`, `lexer/` | Tokenization, strings, and lexer-local tests. |
| `core/vm/src/parser.rs`, `parser/` | Syntax and declaration/expression parsing. |
| `core/vm/src/checker/` | Name/type/effect/lifecycle validation. |
| `core/vm/src/checker/authority.rs`, `checker/authority/` | Canonical callable authority graph, transitive inference, reverse indexes, and system-bound enforcement. |
| `core/vm/src/compiler/` | Checked AST to verified bytecode and runtime metadata. |
| `core/vm/src/vm/` | Execution, settlement, builtins, replay cloning, and VM-owned runtime state. |
| `core/vm/src/world.rs`, `world/` | ECS storage, allocator, operations, snapshots, and canonical encoding. |
| `core/vm/src/relation/` | One first-class relation context: front end, authoritative runtime, and derived-fact evaluation. |
| `core/vm/src/causality.rs`, `causality/` | Provenance ledger and settlement ancestry. |
| `core/vm/src/internal_tests/` | Crate-private integration suites that require private state. |
| `core/vm/tests/` | Public Rust API and subsystem integration tests. |
| `core/vm/benches/` | Criterion benchmarks tied to the core library. |

Some large private composition roots use `include!` to retain one semantic
namespace. The 1,000-line gate does not treat that as architectural
modularity. When a file reaches the threshold, review the whole responsibility
and its call sites; split only if privacy can enforce a real boundary. The gate
prints the full SRP review checklist on every run.

### Why WASM remains in core

The CLI and LSP consume public `rad-vm` APIs and therefore live in separate
adapter crates. Current WASM bindings translate private VM/GC values and own a
runtime session facade. Moving those files today would either expose unsafe
internals or make core depend on an adapter. Extract WASM only after a stable
public session interface exists.

### Shared builtin authority

`core/vm/src/value/builtin_catalog.rs` owns every builtin enum identity and
source spelling. `core/vm/src/builtins/` owns signatures and one canonical
effect table. Checker purity/read-only/effect diagnostics delegate to that
table; they do not maintain independent allowlists.

## Adapters

| Path | Responsibility |
|---|---|
| `adapters/cli/` | `rad` executable, command parsing, process I/O, and command composition. |
| `adapters/lsp/` | LSP transport, documents, diagnostics, symbols, hover, and formatting requests. |
| `adapters/webgpu/` | Bounded presentation packet validation and disposable browser/GPU resources. |

The adapter model follows dependency inversion: policy stays in core; host
mechanisms depend on its public contract.

## MOBA ownership

All MOBA code has one obvious owner:

```text
projects/moba/
├── kit/             RAD gameplay corpus and golden fixtures
├── simcore/         project-owned native/WASM hot path
└── vertical-slice/
    ├── protocol/    shared wire authority and generator
    ├── client/      browser endpoint
    ├── server/      RAD authority endpoint
    └── edge-proxy/  opaque transport endpoint
```

`protocol/contract.json` is the only hand-edited packet layout and code-table
authority. Generated endpoint bindings are checked in and `--check` is part of
both package test commands. The edge proxy is a sibling because it forwards
opaque traffic; it is not part of the server's simulation authority.

## Documentation and RFC authority

| Path | Responsibility |
|---|---|
| `docs/rfcs/` | Canonical RFC bodies. |
| `docs/src/rfcs/` | One-line mdBook <code>&#123;&#123;#include path&#125;&#125;</code> wrappers only. |
| `docs/src/guide/` | User workflows and concepts. |
| `docs/src/reference/` | Language/runtime contracts and architecture. |
| `docs/src/examples/` | Narrative project and example documentation. |
| `docs/src/project/` | Roadmap, changelog, contribution policy, and repository maps. |

The architecture gate rejects a missing, orphaned, or independently editable
RFC wrapper.

## Test taxonomy

| Location | Contract |
|---|---|
| Module-local `#[cfg(test)]` | Private implementation behavior close to its owner. |
| `core/vm/src/internal_tests/` | Cross-module tests that genuinely require crate-private state. |
| `core/vm/tests/` | Public Rust API, wire, and subsystem integration. |
| `tests/conformance/` | Canonical implementation-independent language behavior and snapshots. |
| `tests/features/` | Focused language attacks and feature regressions. |
| `tests/fixtures/` | Shared input data, including causal snapshot fixtures. |
| `tests/manual/` | Explicitly manual, environment-dependent checks. |
| `projects/*/test*` | Application-specific behavior and endpoint contracts. |

Top-level `tests/` therefore means repository/language contracts; conformance
is its most authoritative subset, not an exception to the rule.

## Dogfood and experiments

`projects/dogfood/` keeps stable workload names flat because the same workload
often spans app, security, persistence, and language research responsibilities.
Its README provides discoverability groups without path churn. Small examples
belong under `examples/`; MOBA code belongs under `projects/moba/`.

Computational workloads may share the project-owned
`projects/dogfood/native-math-kernels/` extension when a measured hot path does
not belong in the generic VM. For example, `zeta-simple-zeros/` owns its RAD
proof and certificate artifacts while the extension owns the exact cover-tree
kernel and its certificate, correlation, spacing, and window submodules.

`experiments/c-backend/` is a frozen experimental C/AOT implementation. It is
kept outside core because it does not define current syntax, checking, runtime,
WASM, or release behavior.

## Mechanical enforcement

Run:

```text
python tooling/check_architecture.py
python tooling/check_line_limits.py
node projects/moba/vertical-slice/protocol/generate.mjs --check
```

CI enforces ownership paths, dependency direction, RFC source authority,
generated protocol parity, and the 1,000-line SRP gate. The line limit permits
exact-path exceptions with concrete justifications; it expressly forbids fake
quota fragments, minification, or moving one arbitrary function merely to pass.
