## World Forking (Speculative Execution)

| Function | Description |
|---|---|
| `fork()` | Snapshot the **full program state** — ECS world *and* the in-flight event queue — returning a `world_fork` value |
| `simulate(fork, systems, ticks)` | Run `systems` on the forked world for `ticks` iterations, returning the updated fork. The fork's pending events fire inside the simulation; whatever it leaves in flight travels with the result |
| `simulate_par(fork, systems, ticks, n, seed, overrides?)` | Run `n` independent simulations of the same fork in parallel, returning a list of result forks. Deterministic: each fork's RNG seed is derived from `seed` and its index, so results are identical regardless of thread count. The optional 6th argument is a **list of resource values** applied to the fork before the rollouts (`[Policy { tax: 8 }]`) — seed a candidate at the call site without `commit()` |
| `simulate_many(forks, systems, ticks, seed)` | The heterogeneous sibling of `simulate_par`: run each of the **distinct** forks in the list in parallel under the same schedule, returning a list of result forks (one per input, same order). Per-fork seeds derive from `(seed, index)`, so results are deterministic regardless of thread count. This is the axis a search wants — evaluate B×K candidate worlds at once |
| `simulate_seeded(fork, systems, ticks, raw_seed)` | ONE rollout under an **exact** RNG seed — no per-index derivation. Feed it `fork_seed(f)` of a `simulate_par`/`simulate_many` result and it reproduces that single rollout bit-identically, without paying for the others |
| `fork_with(fork, resource_value)` | Return a copy of `fork` with one resource overridden (e.g. `fork_with(f, Policy { rate: 8 })`) — seed a speculative candidate **without** `commit()`ing to the live world. Events, timers, and entities ride through untouched, so it composes straight into `simulate`/`simulate_par`/`simulate_many` |
| `fork_seed(fork)` | The effective RNG seed the rollout that produced this fork ran under, or `0` for any fork that is not a simulate-family result (derived seeds are never 0). Local debug metadata: not serialized to the wire, and cleared by `fork_with` (an overridden copy is a new candidate, not a rollout's output) |
| `commit(fork)` | Replace the live program state with the fork's — world **and** pending events, exactly as captured |
| `peek(fork, entity, Component)` | Read a component value from a fork without committing |
| `peek_resource(fork, Resource)` | Read a resource value from a fork without committing — `Some(value)` or `None`. Reads simulated scores/clocks straight off result forks. |
| `sandbox_run(source, fork, caps_json, input?)` | Compile and run untrusted RAD source against a fork inside a capability-bounded guest VM. Returns `Result<world_fork, str>`. The optional 4th argument is data-only input the guest reads back with `sandbox_input()` |
| `sandbox_input()` | (Inside a sandboxed guest) the data-only input provided by the host, or `nil` |
| `sandbox_output(v)` | (Inside a sandboxed guest) report a structured, data-only result to the host. Serialized to JSON immediately; last call wins |
| `sandbox_last_output()` | (In the host) the structured value the most recent `sandbox_run` guest reported via `sandbox_output(v)`, parsed back onto the host heap, or `nil` if it reported none |
| `sandbox_last_fuel()` | (In the host) fuel consumed by the most recent `sandbox_run` (0 before any run) — the metering signal for billing or rate-limiting a plugin |
| `diff(fork_a, fork_b)` | Per-component changed-row counts between two forks, as a `map<str, int>`. O(archetypes) `Arc` pointer comparisons, not a world scan |
| `assert_only_changed(fork_a, fork_b, allowed)` | Runtime error unless every difference between the forks is in the `allowed` component list. Accepts component type refs (`Health`) or name strings (`"Health"`) |

`fork()` captures a copy-on-write snapshot of the ECS (entities, components, resources, archetypes) in O(A)
`Arc` refcount bumps. Mutation after a fork pays for copy only on touched data: `Arc::make_mut`
clones the `ValueColumn`, running an O(E) retain scan for persistent `Arc<Object>` refs (see [Architecture](./architecture.md)).

**Events are program state**, so they fork like everything else: `fork()`
captures the pending event queue alongside the world (payloads persisted,
causality ids included), and `commit()` restores it — events emitted after
the fork are rewound with the world, events pending at the fork fire when
you next `flush_events()`. A snapshot that dropped them would not be a
snapshot.

`simulate()` temporarily swaps in the fork — world *and* event queue — runs
the listed systems, and produces a new fork without touching the real world.
Events emitted inside the simulation enqueue on the simulation's own
timeline: they fire on later simulated ticks, and anything still in flight
at the end travels with the result fork (peekable, committable, mergeable).
You can chain `simulate()` on its result for multi-phase prediction:
`simulate(simulate(f, [system::A], 3), [system::B], 2)`.

`commit()` atomically replaces the live program state with the fork's. This
is not reversible — if you need to inspect a fork before committing, use
`peek()`. After a commit, `why()` honestly discloses the seam: writes made
*inside* a fork are not in the causality ledger, so explanations note when
the current value may originate from the committed timeline.

`peek()` reads a single component from the forked world without committing or modifying
any state. Returns an `Option` — `None` if the entity or component doesn't exist in the fork.
Values are deep-copied across the air gap (O(F) for F fields; string fields are O(1) via `Arc<str>`).

The type checker statically prevents systems that perform IO, call `commit()`, call unsafe
event-effect operations such as `transition`, or reach unsafe handler chains from being used
inside `simulate()`. `emit` statements are allowed: their handlers dispatch on the fork's
event queue, isolated from the live timeline. This includes `rand_*`: a plain fork carries no explicit
seed, so randomness inside `simulate()` would not be reproducible. If a speculated system
needs randomness, use `simulate_par()` — its forks are explicitly seeded, so the checker
permits `rand_int`/`rand_float`/`rand_bool` there (and only there; `rand_seed` stays banned,
since re-seeding would collapse the per-fork divergence).

### Parallel exploration: `simulate_par`

`simulate_par(fork, [system::A, system::B], ticks, n, seed)` explores `n` futures of the same
starting fork on a worker-VM pool (one VM per thread, snapshots restored via CoW `Arc` bumps).
Each fork gets an RNG seed derived from `(seed, fork_index)` with a SplitMix64 finalizer, so
runs are **bit-identical for the same inputs at any thread count**. Use it to score
alternative strategies and `commit()` the winner:

```rad
let futures = simulate_par(fork(), [system::Economy], 10, 8, 42)
let best = max_by(futures, fn(f) { return (peek(f, kingdom, Treasury) |> unwrap).gold })
commit(best)
```

For **search** — where the interesting axis is different *candidate policies*, not repeated
rollouts of one — pair `fork_with` with `simulate_many`. `fork_with` seeds each candidate off a
shared root fork without mutating the live world, and `simulate_many` evaluates them all in
parallel:

```rad
let root = fork()
let candidates = [
    fork_with(root, Policy { rate: 1 }),
    fork_with(root, Policy { rate: 5 }),
    fork_with(root, Policy { rate: 10 }),
]
let futures = simulate_many(candidates, [system::Economy], 10, 42)
let best = max_by(futures, fn(f) { return (peek(f, kingdom, Treasury) |> unwrap).gold })
commit(best)   // the ONLY write to the live world in the whole search
```

Because `fork_with` never commits, a purely speculative tree search leaves the live world
bit-identical to where it started — no `commit`/mutate/fork dance, and nothing stranded if the
search is abandoned midway.

When the candidates only differ in resource values, `simulate_par`'s optional override list
does the seeding inline — `simulate_par(root, SYSTEMS, 10, 6, 42, [Policy { rate: 5 }])` runs
six rollouts of the root with `Policy` overridden, and the live world is never written.

**Reproducing one rollout.** Every simulate-family result knows which effective RNG seed
produced it. When rollout 4 of 6 is the outlier that decides a candidate's worst-case score,
pull its seed and re-run exactly that future in isolation:

```rad
let outs = simulate_par(root, SYSTEMS, 10, 6, 42)
let outlier_seed = fork_seed(outs[4])              // never 0 for a rollout result
let again = simulate_seeded(root, SYSTEMS, 10, outlier_seed)
// `again` is bit-identical to outs[4] — one rollout's cost, not six.
```

The seed answers "which future is this" (`fork_seed(fork())` is `0` — only simulate-family
results carry one), and `simulate_seeded` is the consumer that makes it actionable: the pair
turns "the SET of rollouts is reproducible" into "each individual rollout is reproducible".

### Blast-radius assertions: `diff` and `assert_only_changed`

Tests normally assert what changed — never what *didn't*. Because forks are CoW snapshots of
100% of program state, RAD can check the negative space cheaply:

```rad
let before = fork()
emit Hit { amount: 25 }
flush_events()
assert_only_changed(before, fork(), [Health])   // error if ANYTHING else changed
```

`diff(fork_a, fork_b)` returns a `map<str, int>` of component/resource type name → number of
changed rows (an upper bound: a freshly cloned column counts all its rows). Comparison is
O(archetypes) `Arc::ptr_eq` checks on CoW columns — untouched data is never scanned, so
diffing two forks of a million-entity world where one component changed costs roughly
the number of archetypes, not a million.

`assert_only_changed(fork_a, fork_b, allowed)` raises a runtime error naming the unexpected
components and their row counts:

```text
assert_only_changed() failed: unexpected changes to [Gold (1 rows)] (allowed: [Health])
```

Spawns, despawns, component removals, and `set_resource` writes all show up in the diff,
since each structurally changes a column or the entity table.

### The speculation sandbox: `sandbox_run`

`sandbox_run(source, fork, caps_json, input?)` runs **untrusted code** (AI-generated plans,
mods, plugins) against a forked world inside a fresh guest VM. The guest never sees the live
world; the host inspects the returned fork with `peek()` and decides whether to `commit()`.

The optional `input` argument is serialized to JSON at the boundary and surfaces inside the
guest as `sandbox_input()` — pass identity and parameters through this typed, data-only
channel instead of splicing host values into the guest's source text:

```rad
match sandbox_run(bot_src, fork(), caps, { "unit": name, "round": n }) {
    Ok(f) => { /* guest read it via sandbox_input()["unit"] */ }
    Err(m) => { print(m) }
}
```

Capability grant format:

```json
{ "read": ["Reactor"], "write": ["Reactor"], "fuel": 1000000, "mem_bytes": 16777216, "seed": 7 }
```

- `write` — component types the guest may write via `set`/`spawn`/`set_resource`/system
  writebacks. Empty (the default) denies all writes; `"*"` grants everything, and is required
  for `despawn`.
- `read` — component/resource types the guest may **read** via `get`/`res`/`require`/`has`/
  `lookup`/`query`/`query_where`/`query_count`/`query_map`/`with_field`/`why`/`entities(C…)`
  and read (non-`mut`) system parameters. **Omitting the key grants read of everything** (`"*"`),
  so pre-existing grants are unchanged; an explicit list is an allowlist and an explicit `[]`
  reads nothing. The **whole-world readers** — `save_world()`, `world_digest()` (no-arg), and
  unfiltered `entities()` — cannot be keyed to one component and require the `"*"` read grant,
  the same way `despawn` requires the `"*"` write grant.
- `fuel` — instruction budget, charged on loop back-edges and calls (default 10M).
- `mem_bytes` — allocation ceiling (default 64 MiB).
- `seed` — guest RNG seed (deterministic by default).

Four enforcement layers apply: a deny-by-default **builtin mask** (no file/network/clock/
process access, no `fork`/`commit`/`sandbox_run` nesting — `commit` is never grantable), the
**component-write ACL** and the symmetric **component-read ACL** above, and the **fuel/memory
budgets**. Module imports are rejected at compile time. Any failure — a malformed capability
grant, guest compile error, capability denial, budget exhaustion — returns `Err(message)`
instead of aborting the host, so a grant computed from an untrusted plugin manifest cannot
crash the host either.

> **Confidentiality vs. integrity.** The `write` ACL together with `diff()`/
> `assert_only_changed()` is an *integrity* boundary — it bounds what a guest can change. The
> `read` ACL is the *confidentiality* boundary — it bounds what a guest can learn. A grant that
> omits `read` (or sets `"read": ["*"]`) is integrity-only: the guest can read every component
> and resource in the forked world and publish it through `print`/`sandbox_output`. If the host
> holds secrets the plugin should not see, name the readable types explicitly, e.g.
> `{ "read": ["Reactor"], "write": ["Reactor"] }`.

Unlike `simulate()`, events emitted by the guest are **not** dropped: the guest VM owns
private double-buffered event queues, so its handlers run normally inside the closed world
(captured-events mode), and pending events are drained after the guest's main completes.
Guest `print` output is surfaced to the host prefixed with `[sandbox]`.

```rad
let proposal = f"""
component Morale { level: 50 }
set(get_entity("kingdom"), Morale { level: 80 })
"""
let caps = f"""{ "write": ["Morale"], "fuel": 1000000 }"""

match sandbox_run(proposal, fork(), caps) {
    Ok(value)  => {
        let m = peek(value, kingdom, Morale) |> unwrap
        if m.level <= 100 { commit(value) }
    }
    Err(message) => { print(f"proposal rejected: {message}") }
}
```

`sandbox_run` returns only `Result<world_fork, str>`, but a guest can report a structured
result with `sandbox_output(v)` and always spends fuel. After the call, the host reads both
back from the calling VM — no need to make the guest WRITE state just to communicate, and no
need to parse `print` text:

```rad
match sandbox_run(bot_src, fork(), caps, { "round": n }) {
    Ok(f)  => {
        let plan = sandbox_last_output()   // the guest's sandbox_output(v), or nil
        let cost = sandbox_last_fuel()     // fuel spent — meter / bill / rate-limit on this
        if cost < budget { /* score `plan` on its own terms, then commit(f) */ }
    }
    Err(m) => { print(f"rejected after {sandbox_last_fuel()} fuel: {m}") }
}
```

Both accessors reflect the **most recent** `sandbox_run` on that VM (fuel is `0` and output is
`nil` before the first). They read host-side state the runtime already held; the same
telemetry is available to JSON-RPC clients as the `out` and `fuel_spent` fields of a `propose`
response.

See `projects/dogfood/speculation/main.rad` for a complete host/guest demo including hostile-proposal
deflection.

### Serving the sandbox to agent frameworks: `rad sandbox serve`

```bash
rad sandbox serve [host.rad] [--caps caps.json]
```

Starts a JSON-RPC 2.0 server over stdio (one JSON object per line) so external processes —
agent frameworks, orchestrators, anything that can pipe JSON — can drive the
speculate-inspect-commit loop against a live RAD world. `host.rad` (trusted) initializes the
world; `--caps` sets the default grant for proposals (overridable per request). Host program
output goes to stderr; stdout carries only protocol lines.

| Method | Params | Result |
|---|---|---|
| `propose` | `{source, input?, caps?}` | `{ok, fork_id, out, diff, fuel_spent, prints}` — or `{ok: false, error, fuel_spent, prints}` on guest failure |
| `peek` | `{fork_id, entity, component}` | `{found, fields}` (`entity` is a name string or id number) |
| `commit` | `{fork_id}` | `{committed: true}` — replaces the live world |
| `drop` | `{fork_id}` | `{dropped: bool}` |
| `shutdown` | — | `{bye: true}` and the server exits |

- `input` crosses a **data-only boundary**: the guest reads it with `sandbox_input()` and
  reports structured results with `sandbox_output(v)` (the `out` field). No closures, no
  heap values — JSON in, JSON out.
- `diff` is a cheap per-component changed-row summary computed by `Arc::ptr_eq` on CoW
  columns — O(archetypes), not O(entities) — so an agent can see the blast radius of its
  proposal (`{"Treasury": 1, "Morale": 1}`) without scanning the world.
- Guest failures (capability denials, budget exhaustion, compile errors) come back as
  `ok: false` with the error message and fuel accounting — the diagnostics an agent needs
  to retry.

Example session (`projects/dogfood/speculation/serve_session.jsonl` piped into the server):

```text
→ {"id":1,"method":"propose","params":{"source":"...","input":{"spend":300,"gain":30}}}
← {"id":1,"result":{"ok":true,"fork_id":1,"out":{"new_gold":700,"new_morale":80},
                    "diff":{"Morale":1,"Treasury":1},"fuel_spent":9,"prints":[]}}
→ {"id":2,"method":"peek","params":{"fork_id":1,"entity":"kingdom","component":"Morale"}}
← {"id":2,"result":{"found":true,"fields":{"level":80}}}
→ {"id":4,"method":"commit","params":{"fork_id":1}}
← {"id":4,"result":{"committed":true}}
```

### Record & replay: `rad run --record`

```text
rad app.rad --record trace.radr
```

Records an execution trace sufficient to reproduce the run bit-for-bit. RAD records
**inputs, not state**: because the interpreter is deterministic (enforced by a permanent
determinism test suite), the trace only needs the values that cross the determinism
boundary —

- the initial RNG seed (header),
- every io builtin result (`read_file`, `http_get`, `input`, `tcp_*`, …) including failures,
- every clock read (`clock`, `now_unix_s`, `now_unix_ms`).

`rand_int` is *not* recorded (pure xorshift off the seed), prints are *not* recorded
(deterministic outputs), and *recordable* io inside `simulate()`/sandboxes cannot exist:
effectful builtins (`read_file`, `http_get`, clocks, …) are statically banned in
simulation schedules and capability-denied in sandboxes, so no value crossing the
determinism boundary originates there. The one thing that *can* still reach the terminal
from inside `simulate()` is a **ghost effect** — `debug_trace()` writes to stderr but is
treated as pure by the typechecker (see §8 of `guarantees.md`). Ghost output is diagnostic
only: it is never recorded, carries no state, and may be elided under `--release`, so it
does not affect reproducibility. A full game session compresses to a few KB of JSONL:

```text
{"t":"header","version":1,"source_hash":"7fdf…","seed":9685449212088958497}
{"t":"io","f":0,"s":1,"b":"read_file","a":"51d1c937a7c98452","r":{"t":"str","v":"goblin:10,…"}}
{"t":"frame","n":0}
```

Each io record carries `f`/`s` (frame/sequence coordinates — frames are main-timeline
`flush_events` flips; speculative flushes inside `simulate()` don't advance the clock) and
`a`, a digest of the arguments. Traces are **self-contained**: the header embeds the full
authenticated module bundle, including resolved import edges, and the final record carries
both a blake3 content digest of the world and the terminal success/error outcome. Traces are
written even when the run crashes — a trace of the crash is the point.

### Replaying: `rad replay`

```text
rad replay trace.radr [--to-frame <n>] [--force]
```

Re-executes the recorded run **bit-for-bit** from nothing but the trace file. Replay-managed
builtins never execute — `read_file` returns the recorded payload even if the file was
deleted, `http_get` replays the recorded response without touching the network, and a
recorded crash is reproduced verbatim. The RNG is rewound to the recorded seed; everything
else replays for free because the interpreter is deterministic.

Three protection layers, all loud:

1. **Integrity** — embedded sources, module identities, resolved import edges, and language
   features are authenticated; a tampered trace is refused (override with `--force`).
2. **Divergence detection** — every replayed io call is checked against the trace (builtin
   name, argument digest, frame coordinate). A mismatch halts with
   `replay divergence at frame N, record #K: …` instead of debugging a timeline that never
   happened.
3. **End-to-end verification** — after replay, both the world content digest and terminal
   success/error outcome are compared. An early crash cannot verify merely because it left
   the same empty world: `Replay verified: world digest matches the recorded run` is printed
   only after the outcome check also succeeds.

`--to-frame N` halts at the start of frame `N` (handlers dispatched by the k-th
`flush_events` belong to frame k), leaving the world exactly as it was mid-history.

### Time travel: `rad replay --serve`

```text
rad replay trace.radr --serve
```

Time-travel debugging as an **API**. On startup the server replays the trace once,
keyframing the world at *every* frame boundary — affordable only because snapshots are
CoW `Arc` bumps, O(archetypes) each. After that single pass there is no re-execution:
`goto_frame` is index movement and every query reads a snapshot. JSON-RPC 2.0 over stdio,
same wire protocol family as `rad sandbox serve`:

| Method | Params | Result |
|---|---|---|
| `info` | — | `{frames, io_records, current, verified, run_error?}` |
| `goto_frame` | `{frame}` | `{frame, digest}` — moves the cursor |
| `peek` | `{entity, component, frame?}` | `{found, fields}` at the cursor or an explicit frame |
| `diff_frames` | `{a, b}` | `{diff: {Component: rows}}` — blast-radius diff pointed backwards in time |
| `why` | `{entity, component, frame?}` or `{resource, frame?}` | `{why}` — the causal chain of the value as of that frame |
| `shutdown` | — | `{bye: true}` |

Frame addressing: index `k` = world at the start of frame `k`; the highest index is the
world at program end. Crashed traces serve their timeline too (`run_error` in `info`) —
the crash state is addressable.

The agent bug-bisection loop (`projects/dogfood/timetravel/bisect_session.jsonl`):

```text
→ {"id":3,"method":"diff_frames","params":{"a":2,"b":3}}
← {"result":{"diff":{"Health":1}}}                          // Gold intact here…
→ {"id":4,"method":"diff_frames","params":{"a":3,"b":4}}
← {"result":{"diff":{"Gold":1,"Health":1}}}                 // …the drain is in frame 3
→ {"id":5,"method":"peek","params":{"frame":3,"entity":"hero","component":"Gold"}}
← {"result":{"found":true,"fields":{"amount":50}}}
→ {"id":6,"method":"peek","params":{"frame":4,"entity":"hero","component":"Gold"}}
← {"result":{"found":true,"fields":{"amount":0}}}           // bad transition confirmed
```

This is where the speculation sandbox (#1), record & replay (#2), and blast-radius diffs
(#3) converge: one wire protocol for proposing futures, replaying pasts, and diffing any
two points on either timeline.
