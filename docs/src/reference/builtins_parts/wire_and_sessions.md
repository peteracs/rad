### Distributed world merge: `fork_to_bytes()`, `fork_from_bytes()`

A fork is full program state — world, names, id-allocator, resources, and
in-flight events. The wire codec moves that state between processes and
machines, so two copies of a program can diverge offline and merge one world
on reconnect:

```rad
// machine A
tcp_write(conn, fork_to_bytes(fork()))

// machine B
let theirs = fork_from_bytes(tcp_read(conn, 1048576)) |> unwrap
let merged = merge_forks(base, fork(), theirs) |> unwrap
commit(merged)
flush_events()        // events that were in flight on machine A fire here
```

| builtin | type | effect |
|---|---|---|
| `fork_to_bytes(fork)` | `(world_fork) -> str` | pure |
| `fork_from_bytes(bytes)` | `(str) -> Result<world_fork, str>` | `ecs` |

Guarantees, each backed by a composition test:

- **Roundtrip is identity.** `fork_from_bytes(fork_to_bytes(f))` is
  value-identical to `f` — entities keep their runtime ids, names, and
  component data; the id allocator transfers exactly (a spawn after ingesting
  the copy lands on the same id as a spawn after committing the original);
  pending events survive with their causality ids. Re-encoding the decoded
  fork is **byte-identical** (canonical encoding).
- **The wire preserves authoritative facts.** Full forks and deltas carry the
  sealed relation manifest, assertion allocator, canonical assertions,
  ancestry/capabilities, entity generations, and verified unique indexes.
  A missing or mismatched relation section fails before the reconstructed
  world can execute.
- **The wire is transparent to merge semantics.** Merging a fork that
  crossed a process boundary produces state-identical results to the same
  merge performed in-process — byte-for-byte through every state section.
  The one honest difference is provenance: the wire path labels records
  that crossed machines with their payload digest, the in-process path has
  no seam to invent.
- **Provenance rides the wire.** The payload carries the sender's ledger
  closure: for every value alive in the fork, the last write that produced
  it, plus the transitive emit chain behind those writes, plus emit records
  for in-flight events. `commit()` ingests it — foreign emit ids are
  remapped to fresh local ids (the in-flight queue's included, so handlers
  that fire *after* the commit still chain back to remote emits), entity
  ids follow any merge remap, and every ingested record is labeled
  `[via wire <digest>]`. The receiver names what it verified, not what the
  sender claims.
- **Schema drift runs `migrate` blocks on ingest.** The payload embeds its
  schema like `save_world()`; a receiver with a newer declaration migrates
  each component as it decodes. Two machines may disagree on schema version
  and still merge.
- **Corruption is an `Err`, not a crash.** The payload carries a blake3
  integrity digest; tampered or truncated bytes are rejected with a digest
  mismatch, garbage is a parse error. Network input is a system boundary.
- **Record & replay compose for free.** Bytes arrive through io (`tcp_read`,
  `read_file`), so a recorded session that ingested a remote fork replays
  bit-identically with no network present.
- **Big payloads ship packed (RADPACK).** Bodies past ~4 KB are emitted as
  `RADPACK1:<tag> <digest> <base64(deflate(body))>` — measured 6-8x smaller
  on realistic worlds. Small payloads stay plain JSON (readable,
  grep-able). Packed and plain are two current representations of the same
  payload, selected by size. The digest is always blake3 of the
  *uncompressed canonical body* and always the second space-separated token,
  so `split(bytes, " ")[1]` names the same world in either representation.
  Recorded tapes (`--record`) use a raw-binary sibling (`RADPACKZ`, zstd) —
  files don't pay the base64 tax.

See `projects/dogfood/syncdesk/` for the flagship: a long-running server and offline
clients on separate processes — concurrent divergence, merge on reconnect,
field-level conflict reports, an in-flight event that rides the wire and
fires on the server, cross-machine `why()`, and a world that survives server
restarts via `save_world()`/`load_world()`.

### Delta sync: `fork_delta()`, `fork_apply()`

After the first full transfer, the world never needs to cross the wire
again. `fork_delta(base, f)` encodes only the **divergence** of `f` relative
to `base` — and within an entity or resource the base already holds, only
the **changed fields of the changed components** (`ent_patch` / `res_patch`
entries; an hp tick ships `[eid, [["Stats", [["hp", 27]]]], []]`). Full rows
travel only for spawns, renames, newly attached components, and layout
drift. Despawns, the in-flight queue, the id allocator, and the provenance
closure **restricted to touched values** complete the payload. Delta sync pays double: it shrinks state
and history at once, because the receiver already ingested the base's
provenance when it ingested the base.

```rad
// receiver, once: full transfer establishes the shared base
let base = fork_from_bytes(bytes) |> unwrap
commit(base)

// sender, every sync after: divergence only
let delta = fork_delta(base, fork())          // KBs, not MBs

// receiver: rebuild the sender's fork on its own copy of the base
let theirs = fork_apply(base, delta) |> unwrap
let merged = merge_forks(base, fork(), theirs) |> unwrap
```

| builtin | type | effect |
|---|---|---|
| `fork_delta(base, fork)` | `(world_fork, world_fork) -> str` | pure |
| `fork_apply(base, delta)` | `(world_fork, str) -> Result<world_fork, str>` | `ecs` |

Guarantees, each backed by a composition test:

- **Apply reconstructs exactly.** `fork_apply(base, fork_delta(base, f))`
  is state-identical to `f` (canonical full encodings match byte-for-byte):
  edits, spawns, despawns, renames, resources, allocator, and pending
  events all survive. The reconstruction's provenance honestly carries a
  `wire <digest>` origin — that is the one difference, and it is disclosed,
  not hidden.
- **Cost tracks the divergence, not the world.** Touched entities are found
  by CoW pointer comparison (O(divergence) when the forks share lineage,
  full-scan fallback when they don't), and every candidate is re-verified
  by value, so false positives cost a comparison, never bytes.
- **The reconstruction shares lineage with the receiver's base.** Apply is
  a CoW restore plus surgical edits — untouched columns stay shared — so
  the O(divergence) merge fast path works on wire-delivered forks, and a
  merge over a delta-delivered fork equals the in-process merge.
- **Provenance is restricted to the delta.** Only records for touched
  entities and changed resources travel, plus the transitive emit chain and
  the in-flight queue's emit records. `commit()` ingests them exactly like
  the full codec's; cross-machine `why()` works identically over the delta
  path.
- **Schema drift migrates on apply.** The delta embeds the schema of the
  types it ships; v1 rows arriving at a v2 receiver run its `migrate`
  block, exactly like `fork_from_bytes`. Field *patches* migrate too: a
  patched component whose shipped layout differs from the receiver's
  declared layout is patched by field name and then re-enters the `migrate`
  block, so derived fields (`shield = hp / 2`) stay coherent.
- **Wrong base and corruption are an `Err`.** The payload carries a blake3
  integrity digest plus a fingerprint of the base it was made against
  (allocator state, entity count, queue length); applying a delta to a
  world it doesn't describe is rejected, not fabricated. Base *identity* is
  the protocol's job — syncdesk keys served bases by the digest in the
  PULL payload's header, and `DPUSH <digest>\t<delta>` names its base by it.

At 10k entities with 200 touched, the delta is ~29 KB against ~1.5 MB for
the full payload (~54x), encodes in ~0.8 ms against ~45 ms, and applies in
~1.3 ms against ~71 ms (see [performance](performance.md)).

### Cross-machine `why()`

Because provenance rides the fork payload, `why()` answers for values this
machine never computed:

```text
Ticket of T-9 = { status: "open" }   (spawned in frame 0)   [via wire 738ec279, remote frame]
  <- by top-level code
```

The chain crosses the seam: a handler that fires locally for an event that
was emitted on another machine explains itself with the remote emit record
(`[via wire …]` on the emit line) and walks back to the remote cause.
Frames inside foreign records follow the *sender's* clock — the label says
so instead of pretending one timeline exists. Ledger ingestion is
component-granular: after a policy-resolved merge, the newest record for a
component is the sender's whole-component write even when the surviving
value mixes both sides field-by-field; the `commit() adopted a fork` note
discloses exactly that seam.

### Causality retention

The provenance ledger behind `why()` is a **window, not an archive**: it
retains the most recent 100,000 write and emit records each, evicting the
oldest. Long-running processes do not grow bookkeeping without bound. Emit
ids stay stable across eviction and commit seams keep their absolute
ordering; when a query reaches into evicted history, `why()` says
`older provenance was evicted by the retention window` instead of guessing.
Full history is always reconstructible by replaying a recorded trace.

### Streaming sessions (embedding API)

A host application (browser tab, game engine, editor) can keep one VM alive
as a **streaming session** instead of compiling per interaction. The
embedding API on `RadRuntime` (native and WASM, exported to JS by
`wasm-pack`):

| method | what it does |
|---|---|
| `runtime_features()` | JSON feature/version handshake for hosts before they enable advanced session features |
| `session_start(source)` | compile once, run top-level, fix the RNG seed (replicas converge) |
| `session_emit(event, fields_json)` | push one event; `fields_json` must be an object keyed by event fields, and `{"entity": "name"}` resolves handles |
| `session_pump()` | flush one frame through the declared handlers; returns that frame's prints |
| `session_render_delta()` | renderer-shaped JSON diff since the last render read: upserts, removes, and changed resources |
| `session_delta()` | the divergence since the last delta, as `fork_delta` bytes — one broadcast per flush |
| `session_apply(delta)` | apply a remote delta in order; wrong-lineage deltas are refused by the base fingerprint |
| `session_state()` / `session_load(state)` | full-state handshake for late joiners |
| `session_digest()` | state-only convergence receipt (`world_digest`) |
| `session_checkpoint()` | push the current world onto the capped undo ring before a user interaction |
| `session_undo()` / `session_redo()` | rewind or reapply a whole-world checkpoint; return `false` when empty |
| `session_why(entity, component)` | explain the live session's current value for a named entity/component |
| `session_preview(event, fields_json)` | emit and flush an event in a fork, return the preview world JSON, then roll back exactly |
| `run_traced(source)` | run a program with timeline tracing enabled and leave frames inspectable |
| `run_traced_with_patch(source, frame, entity, component, field, value_json)` | rerun with a field patch injected at a frame to preview the rewritten future |
| `timeline_len()` | number of captured timeline frames |
| `timeline_world(i)` | renderer-shaped JSON for captured frame `i` |
| `timeline_events()` | JSON event log sourced from the causality ledger |
| `why_at(frame, entity, component)` | causal explanation for a named entity/component as of a captured frame |

Host-pushed events get real causality records (`why()` answers for them),
and a session's frames are the same frame boundary record/replay counts.
Replicas never run handlers — they converge on state alone, which is what
makes a 3-tab browser demo agree byte-for-byte with the tab that did the
work. See `projects/playground/collab.html` for the wiring: BroadcastChannel
between same-browser tabs by default, or a real WebSocket relay
(`projects/playground/relay/relay.mjs`, `?relay=ws://host:8378`) so peers on other
machines join the same session — the relay is dumb fan-out; every
semantic stays in the VM.

`runtime_features()` reports `"causal_laws": 1` and
`"causal_constraints": 1` when the embedder can compile RFC-0001/RFC-0002
syntax. It also reports `"host_values": 1` and the active
`causal_value_limits` profile (`max_depth`, `max_nodes`,
`max_encoded_bytes`, and `max_collection_items`). WASM hosts opt in by
checking those markers before providing a Causal Laws program; the native CLI
uses `--experimental-laws`.

The `constraint_limits` object contains the version and fingerprint plus
per-invocation fuel/heap limits, the separately reserved aggregate fuel/heap
envelope, violation caps, and the exact canonical rejection byte cap. The
profile's value limits are the same limits shown in `causal_value_limits`;
setting either host profile updates the single transaction value domain.
Browser hosts can call `compile_and_run_result_json()` for a tagged
`settlement_rejected`, `runtime_error`, or `host_fault` result.

Rejection candidate values are frozen once per `(entity, component)` and
referenced by violations. Canonical bytes are produced through a bounded
writer. Capability rendering replaces hidden origins as a whole, including
law/resolver/intent identity and source metadata, instead of exposing a name
with only its payload removed.

Rust embedders exchange [`FrozenValue`](../../../core/vm/src/host_value.rs)
trees with the VM. A `ValueHandle<'vm>` may inspect one imported or global
value while its VM is borrowed, but cannot outlive that VM. The NaN-boxed raw
value and GC heap are deliberately crate-private. This prevents a heap pointer
from surviving its owner or being mutably aliased by copying a machine word.

Causal proposal and candidate capture uses the same limit profile. Cycles are
rejected. Shared acyclic subgraphs are serialized as trees and every repeated
edge is charged again, matching the canonical provenance representation. A
limit failure aborts the settlement without committing world or ledger state.

```js
const runtime = new RadRuntime()
JSON.parse(runtime.runtime_features())
runtime.session_start(source)
runtime.session_checkpoint()
runtime.session_emit("Click", JSON.stringify({ target: "button-1" }))
const printed = runtime.session_pump()
const render = JSON.parse(runtime.session_render_delta())
const why = runtime.session_why("button-1", "Style")
```

> **Backend note:** `core/vm` is the ground-truth implementation for
> speculative execution and event semantics. The historical C backend is frozen
> and should not be used as current feature-support evidence.

```rad
let future = fork()
let predicted = simulate(future, [system::Physics, system::AI], 10)

// Inspect the fork without committing
let predicted_hp = peek(predicted, hero, Health)?
if predicted_hp.hp > 0 {
    commit(predicted)
}
```
