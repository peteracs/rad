# Rad

**A language for debugging and testing stateful simulations.**

Rad is an imperative language with a Rust bytecode VM. The runtime owns the
program's persistent state — entities, components, resources, and in-flight
events all live in a copy-on-write archetype store. That makes a family of
normally-expensive debugging tools cheap enough to ship as language builtins.

Two questions, one line of code each:

**"Why is this value wrong?"**

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

The runtime keeps a provenance ledger of every main-timeline write and emission.
Handler causes link to the exact emit record of the event *instance* handled, so
the chain is causal rather than just temporally adjacent.

**"What else did my fix break?"**

```rad
let snap = fork()
emit Hit { amount: 25 }
flush_events()
assert_only_changed(snap, fork(), [Health])   // errors if anything else moved
```

Tests normally assert what changed. This asserts the negative space across the
program's state, and costs O(archetypes) `Arc` pointer comparisons rather than a
world scan. When a "fix" quietly touches something unrelated, it says so —
from `rad projects/dogfood/speculation/blast_radius.rad`:

```text
diff of the 'fix': {"Gold": 1, "Health": 1}
assert_only_changed(before, after, [Health]) would fail here:
  -> unexpected changes to [Gold (1 rows)]
```

## The rest of the toolkit

| Capability | Builtins / commands |
|---|---|
| Speculative execution | `fork()`, `simulate()`, `simulate_par()`, `peek()`, `commit()` |
| Untrusted code, safely | `sandbox_run(source, fork, caps)`, `rad sandbox serve` |
| Three-way state merge | `merge_forks()`, `merge_forks_with()`, `fork_to_bytes()` |
| Record & replay | `rad app.rad --record trace.radr`, `rad replay` |
| Time-travel debugging | `rad replay trace.radr --serve` (`goto_frame`, `diff_frames`, `why`) |
| Retroactive edits | `rad replay trace.radr --with fixed.rad` |
| Causal settlements (experimental) | `intent`, `law`, `resolver`, `settle`, `why()` fan-in |

`simulate_par` is bit-identical for the same inputs at any thread count — each
fork's RNG seed is derived from `(seed, index)` via a SplitMix64 finalizer. A
static effect system rejects systems that do IO or call `commit()` inside
`simulate()`, including through their transitive event-handler chains.

`sandbox_run` runs untrusted Rad source against a fork behind three independent
layers — a deny-by-default builtin mask, a component-write ACL, and fuel/memory
budgets — so the host decides what commits.

## Why ECS, pipelines, and events

They are the substrate, not the headline. Columnar archetype storage behind
`Arc` is what makes a snapshot a refcount bump and a diff a pointer comparison.
First-class events with declared handlers are what give `why()` something real
to attribute a write to. Pipeline purity checking keeps the transform layer free
of writes. ECS, `|>`, and event-driven messaging are all well-trodden ground on
their own; spending them on debugging and testing tooling is the point.

## Try it

Runnable demos, each producing the output quoted above:

```bash
rad projects/dogfood/causality/main.rad             # why() across two event hops
rad projects/dogfood/speculation/blast_radius.rad   # diff / assert_only_changed
rad projects/dogfood/speculation/main.rad           # capability sandbox
rad projects/dogfood/worldmerge/main.rad            # three-way world merge
rad projects/dogfood/causal-laws/main.rad --experimental-laws # typed causal fan-in
```

Build:

```bash
cargo build -p rad-vm
target/debug/rad examples/demo.rad
```

On Windows:

```powershell
cargo build -p rad-vm
target\debug\rad.exe examples\demo.rad
```

## Status

Version **0.5.0**, single implementation. The capabilities above are implemented
and demonstrable today; there is no cross-implementation conformance check and
no stability promise across 0.x releases. The full per-component status table
and known limits are in the [Introduction](docs/src/introduction.md#status).

## Documentation

- [Introduction](docs/src/introduction.md) — start here
- [Language guide](docs/src/SUMMARY.md)
- [Built-in functions](docs/src/reference/builtins.md) — `why`, `fork`, `diff`, `sandbox_run`, and contracts
- [Language guarantees](docs/src/reference/guarantees.md) — behavioral contracts with maturity labels
- [Language spec](docs/src/reference/spec.md)
- [Causal Laws guide](docs/src/guide/causal-laws.md) — experimental typed intents and atomic settlements
- [RFC-0001](docs/rfcs/0001-causal-settlements.md) — normative v0 semantics
- [RFC-0002 draft](docs/rfcs/0002-candidate-constraints.md) — validation-only candidate constraints
- [Performance](docs/src/reference/performance.md)
- [Contributing](docs/src/project/contributing.md)
- [Repository map](docs/src/project/repo-map.md)
- [Repository audit](docs/src/project/repo-audit.md)
- [Changelog](docs/src/project/changelog.md)
- [Roadmap](docs/src/project/roadmap.md)
- [RFC process](docs/src/project/rfcs.md)

## License

Rad is available under the [MIT License](LICENSE).
