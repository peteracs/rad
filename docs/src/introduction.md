# Rad

**A language for debugging and testing stateful simulations.**

Two questions cost more hours than any others when a simulation misbehaves:

1. **Why is this value wrong?**
2. **What else did my fix break?**

Rad answers both with one-line builtins. It can do that because the runtime owns
the program's persistent state — entities, components, resources, and in-flight
events all live in a copy-on-write archetype store, and the ECS API is the only
channel for mutating the world. There is no mutable global variable or implicit
shared state between systems for a value to hide in. That single architectural
decision makes a family of normally-expensive debugging tools cheap enough to
ship as language primitives instead of as a separate tooling project.

---

## 1. `why()` — the causal chain of a value

The runtime keeps a provenance ledger of every main-timeline write and every
event emission. Handler causes link to the exact emit record of the event
*instance* being handled, so the chain is causal rather than merely correlated
in time.

```rad
component Health { hp: 100 }
component Gold { amount: 50 }
resource Tally { drains: 0 }

event Hit { amount }
event Drained { }

let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 50 })

on Hit(e) {
    let h = get(hero, Health) |> unwrap
    set(hero, Health { hp: h.hp - e.amount })
    if h.hp - e.amount < 80 {
        set(hero, Gold { amount: 0 })   // <- the bug
        emit Drained { }
    }
}

on Drained(_d) {
    let t = get_resource(Tally) |> unwrap
    set_resource(Tally, Tally { drains: t.drains + 1 })
}

print(why_resource(Tally))
```

Running `rad projects/dogfood/causality/main.rad` prints the chain across two
event hops, frame by frame:

```text
resource Tally = { drains: 1 }   (set in frame 4)
  <- by `on Drained` handler
  <- Drained {} emitted in frame 3
  <- by `on Hit` handler
  <- Hit { amount: 10 } emitted in frame 2
  <- by top-level code
```

`why(entity, Component)` does the same for components. Chains cover `set`,
`spawn`, `remove`, `despawn`, system writebacks, and resource writes. Because
the ledger is frame-indexed, the same question can be asked of a *past* frame
during replay — see [time travel](#4-record-replay-and-time-travel) below.

## 2. `assert_only_changed()` — the blast radius of a change

Tests normally assert what changed. They rarely assert what *didn't*, because
scanning all program state to prove a negative is too expensive to do per
assertion. Forks are copy-on-write snapshots, so Rad compares them with
`Arc` pointer equality on columns: the cost is O(archetypes), not a world scan.
Untouched data is not read at all.

```rad
let snap = fork()
emit Hit { amount: 25 }
flush_events()
assert_only_changed(snap, fork(), [Health])   // errors if anything else moved
```

`diff(fork_a, fork_b)` returns the same information as data — a map of
component name to changed-row count. From
`rad projects/dogfood/speculation/blast_radius.rad`:

```text
diff after 3 Damage ticks: {"Health": 2}
Hit handler touched only Health (goblin hp: 5)
diff of the 'fix': {"Gold": 1, "Health": 1}
assert_only_changed(before, after, [Health]) would fail here:
  -> unexpected changes to [Gold (1 rows)]
```

That last case is the one worth dwelling on: a "fix" that healed the hero also
zeroed their gold, and the assertion names the collateral damage. Row counts are
an upper bound — a freshly cloned column counts all of its rows.

## 3. Speculation and sandboxing

`fork()` snapshots full program state — the ECS world *and* the in-flight event
queue — in O(archetypes) `Arc` refcount bumps. From there:

| Builtin | What it does |
|---|---|
| `simulate(fork, systems, ticks)` | Run systems forward on the fork without touching the live world |
| `simulate_par(fork, systems, ticks, n, seed)` | Explore `n` futures in parallel |
| `peek(fork, entity, Component)` | Read from a fork without committing |
| `commit(fork)` | Replace live program state with the fork's |
| `merge_forks(base, ours, theirs)` | Three-way merge of two divergent futures |

`simulate_par` is bit-identical for the same inputs at any thread count: each
fork's RNG seed is derived from `(seed, index)` through a SplitMix64 finalizer,
so the result does not depend on how work landed on threads.

A static effect system keeps speculation honest. The checker rejects any system
used inside `simulate()` that performs IO, calls `commit()`, or whose transitive
event-handler chain does. Relatedly, `rand_*` is rejected in plain `simulate()`
— a bare fork carries no seed, so the result would not be reproducible — but
permitted in `simulate_par()`, where forks are explicitly seeded.

**`sandbox_run(source, fork, caps_json, input?)`** runs *untrusted* Rad source
against a fork, so its effects are speculative by construction and the host
decides what commits. Three independent containment layers apply. All three are
visible firing in `rad projects/dogfood/speculation/main.rad`:

```text
Raid denied (ACL): sandbox: write to component 'Army' denied by capability grant
Spy denied (builtin mask): sandbox: builtin 'read_file' is not permitted under the capability grant
Bomb starved (fuel budget): Budget exhausted: instruction (fuel) limit reached
```

A capability grant is JSON: `write` lists the component types the guest may
write, `fuel` is an instruction budget, `mem_bytes` an allocation ceiling, and
`seed` the guest RNG seed. `rad sandbox serve` exposes the same
propose / peek / commit loop as a JSON-RPC 2.0 server over stdio, so an external
agent framework can drive it.

## 4. Record, replay, and time travel

`rad app.rad --record trace.radr` records **inputs, not state** — the RNG seed,
IO results, and clock reads. Everything else re-derives, because the interpreter
is deterministic. A session compresses to a few KB, and traces are written even
when the run crashes.

`rad replay trace.radr --serve` then replays once, keyframing the world at every
frame boundary (affordable only because snapshots are `Arc` bumps), and serves
`goto_frame`, `peek`, `diff_frames`, and `why` over JSON-RPC. Provenance is
reconstructed during that pass, so `why` works on traces recorded before the
feature existed.

`rad replay trace.radr --with fixed.rad` answers "what would my fix have done in
that exact session?" by replaying the recorded inputs against edited code and
diffing the two final worlds:

```text
The edit's blast radius (original vs edited final world):
  {Gold: 1}
```

Read as: *this fix restores the drained gold and touches nothing else.*

---

## The substrate: ECS, pipelines, and events

None of the above is a separate tool bolted onto a runtime. Each falls out of
three structural commitments that Rad makes at the language level.

```rad
component Health { hp: 100, max: 100 }
component Name  { indexed value: "" }
resource GameState { tick: 0 }

event Hit { target_name: str, amount: int }

on Hit(e) {
    let target = lookup(Name, "value", e.target_name)?
    let h = get(target, Health)?
    let mut new_hp = h.hp - e.amount
    if new_hp < 0 { new_hp = 0 }
    set(target, Health { hp: new_hp, max: h.max })
}

system HealAll(h: mut Health) {
    if h.hp < h.max { h.hp = h.hp + 1 }
}
```

- **Entity Component System** is the storage model, not a library. Because
  components live in columnar archetype storage behind `Arc`, a snapshot is a
  refcount bump and a diff is a pointer comparison. Forking, `diff`, and
  `assert_only_changed` are affordable for this reason.
- **Events** are first-class declarations with declared handlers. The `why()`
  chain exists because emissions and handler dispatch are runtime concepts the
  VM can record — in a language where events are a user-space callback list,
  there is nothing structural to attribute a write to.
- **Pipelines** (`|>`) are checked for purity: only pure or `readonly` functions
  may appear as stages, so side-effecting builtins like `set` are rejected at
  compile time. That keeps the transform layer free of the writes that
  provenance and blast-radius analysis would otherwise have to chase.

These patterns are not the novel part — ECS appears in Bevy, Flecs, Unity DOTS
and EnTT; `|>` in F#, Elixir, OCaml and Julia; event-driven messaging is the
core of Erlang. What is unusual is treating all three as a *runtime* substrate
that owns the program's persistent state, and then spending that ownership on
debugging and testing tools. See [The Three Laws](./guide/three-laws.md) for the
design rationale.

Rad is an imperative language with strong defaults, not a straitjacket. Top-level
code, `for` loops, and direct `set()` calls are ordinary — the demos above use
all three. Bindings are immutable unless declared `mut`, and assignment copies
rather than aliases, but you can still write badly structured programs if you
want to.

## Who this is for

If you have ever bisected a game server or a distributed simulation asking "why
is this value wrong?" and then "what else did my fix break?", Rad is aimed at
you. The natural fits are **networked game simulation**, **agent sandboxes**
(untrusted plans evaluated against a real world state, then accepted or
discarded), and **policy engines** where a proposed change needs to be scored
and bounded before it lands.

Rad is a poor fit for systems programming, numeric kernels, or anything where
you want manual control of memory layout.

## Quick links

- [Try it in your browser](https://peteracs.github.io/rad/)
- [Built-in functions](./reference/builtins.md) — `why`, `fork`, `diff`, `sandbox_run`, and the rest, with contracts
- [Language guarantees](./reference/guarantees.md) — the behavioral contracts, with maturity labels
- [Language specification](./reference/spec.md)
- [Installation](./getting-started/installation.md) and [Hello World](./getting-started/hello-world.md)
- [Browse examples](https://github.com/peteracs/rad/tree/main/examples)
- [GitHub repository](https://github.com/peteracs/rad)

## Status

Rad is **version 0.5.0** with a single implementation. The capabilities above
are implemented and demonstrable today; the language is not covered by a
stability promise across 0.x releases, and there is no second implementation to
check the specification against.

| Component | Status |
|---|---|
| Rust bytecode VM (`rad`) | **Working** — lexer, parser, type checker, compiler, VM; 283 conformance programs and 69 examples in-tree |
| Causality (`why`, `why_resource`) | **Working** — frame-indexed provenance ledger; retains the most recent 100,000 write and emit records, then evicts |
| Forking & speculation | **Working** — `fork`, `simulate`, `simulate_par`, `peek`, `commit`; `simulate_par` is thread-count independent |
| Blast radius (`diff`, `assert_only_changed`) | **Working** — O(archetypes) `Arc` pointer comparison; row counts are an upper bound |
| Capability sandbox | **Working** — `sandbox_run` plus `rad sandbox serve` (JSON-RPC 2.0 over stdio); builtin mask, component-write ACL, fuel/memory budgets |
| World merge | **Working** — `merge_forks` / `merge_forks_with`, field-level conflicts as data, entity-id remapping; `fork_to_bytes` / `fork_from_bytes` for cross-process merge |
| Record / replay / time travel | **Working** — `--record`, `rad replay`, `--to-frame`, `--serve`, `--with` |
| `rad` CLI | **Working** — `rad <file>` type-checks by default (`--no-check` skips); `run`, `test`, `fmt` / `fmt --check`, `lint`, `new`, `snapshot`, `play`, `replay`, `sandbox serve`, `build` |
| Browser playground | **Working** — Rust VM via WASM |
| LSP server (`rad lsp`) | **Working** — Rust checker diagnostics, hover, go-to-definition, completions, formatting |
| `build --target wasm` | **Stub** — emits the [Phase 3](./reference/wasm-phase3.md) reactor stub; the WASM guest returns no diagnostics until a real `compiler.wasm` implements checking |

### Known limits

Stated up front rather than discovered later:

- The provenance ledger is a **window, not an archive** (100,000 records each of
  writes and emits). Queries that reach past it say so instead of guessing; full
  history is reconstructible by replaying a recorded trace.
- Writes made inside `simulate()` forks and sandbox guests are deliberately
  **invisible** to `why()` — speculative values are not "this value". After a
  `commit()`, `why()` discloses that seam rather than papering over it.
- `diff` row counts are an **upper bound**: a column that was cloned counts all
  of its rows as changed.
