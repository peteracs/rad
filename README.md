<div align="center">

# Rad

### Stateful programs that explain themselves.

Rad is a programming language for simulations, game servers, agents, and policy
engines where state must be inspectable, forkable, replayable, and hard to
mutate by accident.

[Playground](https://peteracs.github.io/rad/) ·
[Documentation](https://peteracs.github.io/rad/docs/) ·
[Rust API](https://peteracs.github.io/rad/rustdoc/rad_vm/) ·
[Examples](https://peteracs.github.io/rad/docs/examples/catalog.html) ·
[Language spec](https://peteracs.github.io/rad/docs/reference/spec.html)

[![CI](https://github.com/peteracs/rad/actions/workflows/ci.yml/badge.svg)](https://github.com/peteracs/rad/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-mdBook-8b5cf6)](https://peteracs.github.io/rad/docs/)
[![Playground](https://img.shields.io/badge/playground-WASM-0ea5e9)](https://peteracs.github.io/rad/)
[![License: MIT](https://img.shields.io/badge/license-MIT-22c55e)](LICENSE)

</div>

Most languages can tell you where a value is now. Rad can tell you why it is
there, fork the entire world to test a change, prove what the change did *not*
touch, and replay the exact session that produced the bug.

The trick is ownership: Rad's runtime owns persistent program state—entities,
components, resources, events, and provenance—instead of treating state as an
opaque side effect of arbitrary objects. That one decision makes debugging
capabilities that are usually external infrastructure into ordinary language
operations.

## Debug state, not symptoms

Ask why a resource has its current value:

```rad
print(why_resource(Tally))
```

```text
resource Tally = { drains: 1 }   (set in frame 4)
  <- by `on Drained` handler
  <- Drained {} emitted in frame 3
  <- by `on Hit` handler
  <- Hit { amount: 10 } emitted in frame 2
  <- by top-level code
```

Test a fix against a copy-on-write future, then assert its entire blast radius:

```rad
let before = fork()
let candidate = simulate(before, [system::ApplyFix], 1)

assert_only_changed(before, candidate, [Health])
```

Tests usually assert what changed. `assert_only_changed` checks the negative
space: if the fix also changes `Gold`, `Position`, a resource, or an entity's
lifetime, the test fails and names it.

## The features that make Rad different

| Capability | What it gives you |
|---|---|
| **Causal state** | `why()` and `why_resource()` trace writes through the exact event instances and handlers that caused them. |
| **Cheap alternate futures** | `fork()`, `simulate()`, `simulate_many()`, and `simulate_par()` evaluate copy-on-write worlds without touching live state. |
| **Blast-radius assertions** | `diff()` and `assert_only_changed()` compare worlds in O(archetypes), without scanning every entity. |
| **Deterministic parallel simulation** | The same inputs produce bit-identical rollout results at any native worker count; a rollout's seed can reproduce that future alone. |
| **Compiler-enforced authority** | System signatures bound transitive reads, writes, event emission, host IO, and async execution across helpers, imports, callbacks, transitions, and synchronous event chains. |
| **Write ownership** | Components, individual fields, and resources have one canonical owner; outside code can read state but cannot encode an unauthorized mutation. |
| **Atomic contract transactions** | `requires`, transitive `changes_only`, `ensures`, and post-commit effects turn multi-entity lifecycle changes into one rollback-safe cause. |
| **Native-fidelity data** | Opaque IDs, fixed-width scalars, enums, bitflags, ABI layouts, and endian codecs preserve representation without collapsing semantic namespaces. |
| **Maintained views and ordered indexes** | Derived subsets and traversal order update transactionally instead of being rebuilt by readers. |
| **Reviewable hot paths** | Query plans expose scans, sorts, allocations, complexity, and transitive cost paths; frame budgets are enforced before deployment. |
| **Stateful model checking** | Generate lifecycle histories, check temporal properties, shrink failures, and replay the minimal `.radr` counterexample from the CLI. |
| **Contained native extensions** | Verify ABI/effects/layout/determinism, isolate crashes and timeouts, pin plugin generations, and replay recorded host results. |
| **Record, replay, and time travel** | Record a session, inspect earlier frames, ask `why` in the past, or replay the same inputs against edited source. |
| **Capability sandboxing** | Run proposed Rad code against a fork with a builtin mask, component-write ACL, and fuel/memory budgets; the host decides what commits. |
| **Mergeable worlds** | Inspect, serialize, diff, patch, and three-way merge forks with deterministic conflict handling. |
| **Runtime-owned presentation state** | The WASM/WebGPU host consumes exact bounded packets and survives resize and device loss without putting GPU handles into world state. |

## Authority is executable architecture

A system signature is not merely a query. It is the maximum live-world
authority that the body—and everything it calls synchronously—may exercise:

```rad
system RemoveEntity(
    live: mut LiveMembership,
    writes WireIdentity,
    emits EntityRetired,
) {
    rewrite_wire(self) // imported/helper writes are checked transitively
    emit EntityRetired { target: self }
    live.active = false
}
```

Omit either grant and compilation fails with the concrete call path. `io true`
and `async true` explicitly opt a system into transitively reachable host or
task effects; both are denied by default. Higher-order helpers are specialized
for each system's statically resolved callback arguments, so unrelated callers
do not inherit one another's authority.

Entity-name operations use narrow synthetic authorities instead of pretending
an index lookup is effect-free: `get_entity`/`require_entity` read
`"$entity_names"`, while `name_of` reads `"$entity_identity"`. Named spawn and
despawn operations write the corresponding indexes.

Inspect the same graph used by the compiler and parallel scheduler:

```bash
rad effects RemoveEntity --json --file app.rad
rad writers WireIdentity --file app.rad
rad readers LiveMembership --file app.rad
rad path mission_frame "->" full_scan --file app.rad
```

The reverse writer index is benchmarked at **307.9× faster** than a full scan
over 50,000 callables (3.0839 µs versus 949.65 µs median). There is one graph
for diagnostics, scheduling, and tools.

## Try it in 60 seconds

The fastest route is the [WASM playground](https://peteracs.github.io/rad/).

To run the native VM:

```bash
git clone https://github.com/peteracs/rad.git
cd rad
cargo build --release -p rad-cli -p rad-ffi-worker -j 1
target/release/rad examples/demo.rad
```

Then try the features that motivated the language:

```bash
target/release/rad projects/dogfood/causality/main.rad
target/release/rad projects/dogfood/speculation/blast_radius.rad
target/release/rad projects/dogfood/worldmerge/main.rad
target/release/rad projects/dogfood/authority-effects/main.rad
target/release/rad projects/dogfood/workpulse/main.rad
target/release/rad projects/dogfood/riskbridge/main.rad
```

## A small language with a serious runtime

Rad includes:

- a Rust lexer, parser, type checker, bytecode compiler, and VM;
- ECS components, singleton resources, indexed fields, systems, phases, and
  deterministic conflict-aware parallel scheduling;
- functions, closures, generics, structs, sum types, exhaustive matching,
  state machines, immutable-by-default bindings, and explicit unique ownership;
- pure and `readonly` effect contracts, value-oriented pipelines, events,
  async handlers, modules, strict exports, formatting, linting, snapshots, and
  an LSP;
- native extensions, a WASM runtime, a browser playground, and a recoverable
  WebGPU presentation boundary.

The repository does not stop at syntax samples. [`projects/dogfood`](projects/dogfood)
contains game runtimes, speculative planners, sandboxes, replay workflows,
causal constraints, browser presentation, and independently checked exact-search
projects built on the same shipping VM. The
[production dogfood portfolio](https://peteracs.github.io/rad/docs/project/production-dogfood-portfolio.html)
binds eleven cumulative services to their negative fixtures, scale workloads,
replays, operational inspections, and one exact-generation release report.

## Where Rad fits

Rad is aimed at systems where proposed changes should be evaluated before they
become authoritative:

- authoritative game and simulation servers;
- agent planning and tool-execution sandboxes;
- policy, workflow, and control-plane engines;
- deterministic test harnesses and replayable incident reproductions.

It is not trying to replace Rust or C for kernels, drivers, or manual memory
layout. Rad is the stateful decision layer around those components.

## Status

Rad is pre-release software at version **0.5.0**. The Rust VM is the single
shipping implementation. The features above are implemented and exercised in
the repository, but 0.x releases do not carry a compatibility promise yet.

Start with the [documentation](https://peteracs.github.io/rad/docs/), then see
the [language guarantees](https://peteracs.github.io/rad/docs/reference/guarantees.html),
[builtins](https://peteracs.github.io/rad/docs/reference/builtins.html),
[enterprise DX semantics](https://peteracs.github.io/rad/docs/reference/enterprise-dx-v05.html),
[examples](https://peteracs.github.io/rad/docs/examples/catalog.html), and
[roadmap](https://peteracs.github.io/rad/docs/project/roadmap.html).

## Contributing

Issues, language-design discussions, benchmarks, and dogfood reports are
welcome. See [Contributing](docs/src/project/contributing.md) and the
[RFC process](docs/src/project/rfcs.md).

## License

[MIT](LICENSE)
