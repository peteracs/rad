### Causality queries: `why()` and `why_resource()`

```rad
print(why(hero, Gold))          // -> str: the causal chain of the value
print(why_resource(Treasury))
```

"Why does this value exist?" as a runtime primitive. The VM keeps a
provenance ledger of every main-timeline write — who wrote it (top-level
code, a system, or an event handler) — and every event emission. Handler
causes link to the exact emit record of the event *instance* they were
handling, so the chain is causal, not merely correlated:

```text
Gold of hero = { amount: 0 }   (set in frame 4)
  <- by `on Hit` handler
  <- Hit { amount: 10 } emitted in frame 3
  <- by top-level code
```

Chains cross as many event hops as it takes (`set_resource` <- `on Drained`
<- `Drained` emitted by `on Hit` <- `Hit` emitted by top-level), and cover
`set`, `spawn`, `remove`, `despawn`, system writebacks (sequential *and*
parallel batches), and resource writes. Writes inside `simulate()` forks and
sandbox guests are deliberately invisible — speculative values never become
"this value".

The same question works backwards in time: the `why` method on
`rad replay --serve` answers from the ledger rebuilt during the replay pass,
**at any frame** — `why {frame: 3, entity: "hero", component: "Gold"}` says
"spawned, top-level", while frame 4 returns the full drain chain. One call
replaces the whole `diff_frames` bisection loop, and it works on traces
recorded before the feature existed, because provenance is reconstructed
from deterministic re-execution rather than stored in the trace. See
`projects/dogfood/causality/main.rad` and `projects/dogfood/timetravel/why_session.jsonl`.

### Retroactive edits: `rad replay --with`

```text
rad replay trace.radr --with fixed.rad
```

Replay the recorded session's **inputs** against **modified code** — "what would my fix
have done in that exact production session?" Two passes run back to back:

1. **Faithful pass** — the trace's embedded (original) source replays strictly,
   producing the recorded final world.
2. **Retro pass** — the edited file runs against the same trace, with recorded io served
   from an **oracle keyed by `(builtin, args)`**, consumed FIFO per key. Same question →
   the same answer the recorded world gave, regardless of how the edit reordered,
   removed, or duplicated calls.

Oracle semantics, chosen deliberately:

- **Repeatable reads** — a key exhausted by extra calls serves its last recorded value:
  a file re-read returns the same content (it didn't change mid-session), an extra
  `clock()` freezes time at its last reading.
- **Holes are loud** — io the recorded session *never* performed halts the retro pass:
  `retroactive replay hole at frame N: …` — replay cannot fabricate answers from a world
  it never saw. (RNG needs no oracle: the seed travels in the header, so `rand_int`
  replays for free even when the edit consumes it differently.)

The deliverable is the **fix's blast radius** — a value-accurate diff of the two final
worlds:

```text
=== Retroactive replay: fixed.rad against the recorded session ===
Recorded io: 3 consumed, 1 repeated reads, 0 unused
The edit's blast radius (original vs edited final world):
  {Gold: 1}
```

`{Gold: 1}` reads as: *this fix restores the drained gold and touches nothing else* —
Health histories are byte-identical. A fix that reports `changes NOTHING` is equally
informative (e.g. an edit confined to `simulate()` forks never touches the real
timeline). See `projects/dogfood/timetravel/fixed.rad` and `main_v2.rad`.

### Schema migration: `migrate`, `save_world()`, `load_world()`

```rad
component Health { hp: 100, max_hp: 100 }       // v2 shape

migrate Health(old) {                            // v1 saves had only `hp`
    return Health { hp: old["hp"], max_hp: old["hp"] * 2 }
}

write_file("world.radw", save_world())           // persist: schema travels with the data
let n = load_world(read_file("world.radw"))      // replace world; migrate shape drift
```

Schema evolution as grammar. The world — entities, names, components,
resources — is a first-class value, so persisting it is one builtin, not a
serialization framework:

| builtin | type | effect | what it does |
|---|---|---|---|
| `save_world()` | `() -> str` | `read_ecs` | world → JSON, **schema embedded** (per-type field layout), authoritative relation assertions included, full-fidelity tagged values, wrapped in the `RADWORLD3` integrity envelope (blake3 digest) |
| `load_world(json)` | `(str) -> int` | `ecs` | JSON -> replacement world; returns entities loaded. **Aborts** on malformed/corrupt input |
| `try_load_world(json)` | `(str) -> Result<int, str>` | `ecs` | the fallible sibling: `Ok(entities_loaded)` or `Err(message)` instead of aborting. A failed load leaves the live world untouched, so an app can fall back to a prior backup |

`save_world()` output carries a blake3 **integrity envelope** (`RADWORLD3 <digest> <body>`, or a
compressed `RADPACK1` envelope for large saves), so `load_world`/`try_load_world` refuse a
corrupted or tampered save loudly instead of loading garbage. Unsupported pre-release save shapes
are rejected rather than retained as compatibility branches.
`load_world` is the fail-fast spelling;
`try_load_world` is the handle-it spelling — the same `get`/`require`, `to_int`/`try_int` pairing
used elsewhere.

Serialization is pure; persistence composes with ordinary io
(`write_file`/`read_file`, TCP, HTTP — anywhere a `str` goes). That one
decision means record & replay (#2) needs zero new machinery: the io
boundary is already recorded.

`load_world` replaces the current entity set with the saved one instead of
appending rows into the live world. Declared resources seed the replacement so
transient resources and resources omitted by older saves remain available; saved
resources then overwrite their declared rows. Each persisted shape is compared
against the declared one:

- **Identical field set** → loads as-is. Field *order* is normalized — reordering
  a component declaration is not a schema change.
- **Shape drift + `migrate X(old)` declared** → the block runs per instance. `old`
  binds the persisted fields as `map<str, any>` (the old shape no longer exists
  as a type); the body must `return` the new component. Renames, splits, computed
  defaults — it's ordinary code.
- **Shape drift, no migration** → a loud error naming exactly what drifted:
  `schema of 'Health' changed (added: [max_hp], removed: []) and no migration is
  declared — add migrate Health(old) { return Health { ... } }`. No silent nulls,
  no zero-filled fields, ever.

**Declared schema versions.** A component or resource may carry a version tag —
`component Incident v2 { ... }` — which `save_world()` embeds per type in the
save's schema section. A migrate block that declares a second parameter,
`migrate Incident(old, from_version)`, receives the **save's** version for that
type as an int (`0` for saves written without one), turning generation
detection from a shape sniff into a fact:

```rad
component Incident v3 { severity: 1, source: "" }

migrate Incident(old, from_version) {
    if from_version == 1 { return Incident { severity: old["sev"], source: "" } }
    if from_version == 2 { return Incident { severity: old["severity"], source: "" } }
    return Incident { severity: old["severity"], source: old["source"] }
}
```

Two generations that happen to share a field set are no longer indistinguishable,
and the sniff can no longer silently pick wrong. The tag is **load metadata,
not state**: re-tagging a component does not move `world_digest()`, and saves
from versionless programs are byte-identical to before. One-parameter
`migrate X(old)` blocks keep working, versioned save or not.

Migrations target components *and* resources by name, and compose with the
rest of the list: loaded entities carry spawn provenance, so `why(hero, Health)`
works immediately after a load, and a `load_world` inside a recorded session
replays deterministically. See `projects/dogfood/schema/v1.rad` / `v2.rad` for the full
v1 → v2 story (added field, renamed fields, migrated resource) in 40 lines.

### Convergence receipts: `world_digest()`

```rad
// after applying the server's down-delta and committing:
if world_digest() == rpc("DIGEST") { print("converged") }
```

| builtin | type | effect | what it does |
|---|---|---|---|
| `world_digest()` | `() -> str` | `read_ecs` | blake3 of the canonical **state-only** serialization, including authoritative relation fact content |
| `world_digest(fork)` | `(world_fork) -> str` | `read_ecs` | the same digest for a fork's state, **without committing it** |
| `schema_digest()` | `() -> str` | `read_ecs` | fingerprint of the program's declared component/resource/event layouts |

Fork bytes (`fork_to_bytes`) include in-flight events, provenance, frame
counters, and id free-lists — all of which legitimately differ between two
machines whose *worlds* agree, so fork digests cannot prove convergence.
`world_digest()` hashes entities (names, components, fields) and resources
only: two processes that merged to the same state print the same digest, no
matter how they got there. Unflushed events do not move it; a real field
change does.

`world_digest()` is an **integrity** receipt (these bytes are what I hashed),
not a **validity** receipt: it certifies whatever world it is handed, including
a type-corrupted one. Validity is enforced separately — the `load_world` field-
type boundary rejects a wrong-typed save, and the `RADWORLD3` envelope rejects a
tampered one — so a digest match means "same state", never "well-formed state".

**Across a schema migration**, raw digest comparison lies: the canonical
body embeds the schema, so a v1 world and its v2-migrated twin digest
differently *by construction* — exactly when a rolling upgrade needs the
receipt most. The protocol that stays honest:

1. Exchange `schema_digest()` first. Equal fingerprints → compare
   `world_digest()` directly, as before.
2. Different fingerprints → the newer side **certifies**: the older peer
   ships its full fork bytes; `fork_from_bytes` migrates them on ingest
   (running the declared `migrate` blocks), and `world_digest(fork)`
   hashes that migrated view. Both sides of the comparison now carry the
   same schema, so equality means *logical* convergence — and a real
   divergence still reports MISMATCH truthfully.

```rad
// the upgraded server's CERTIFY handler:
match fork_from_bytes(client_bytes) {
    Ok(theirs) => {
        if world_digest(theirs) == world_digest() { reply("MATCH") }
        else { reply("MISMATCH") }
    }
    Err(m) => { reply(f"ERROR {m}") }
}
```

See `projects/dogfood/radtrack/demo/run_rolling_demo.ps1` for the live receipt.

### World merge: `merge_forks()`

```rad
let base = fork()
// …branch A mutates the world… let ours = fork()
// …commit(base), branch B mutates… let theirs = fork()

match merge_forks(base, ours, theirs) {
    Ok(merged) => { commit(merged) }       // both futures, one timeline
    Err(conflicts) => {                    // conflicts are data, not prose
        for c in conflicts {
            match c {
                FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                    print(f"{name}: {comp}.{field} ours={ours} theirs={theirs}")
                }
                _ => { print("structural conflict") }
            }
        }
    }
}
```

Git merge for program state — with one move git cannot make, because the
language owns 100% of state and text owns none:

- **Field granularity.** A conflict is the *same field* of the same entity or
  resource diverging from base in both forks — never coarser. Two forks
  editing `Stats.atk` and `Stats.def` of the same component merge cleanly;
  two forks raising `Bank.gold` and upgrading `Bank.vault` both land.
  Convergent edits (both forks writing the same value) are not conflicts.
- **Entity ids are handles, not identity.** Two forks spawning different
  entities that collide on a runtime id is *not* a conflict: theirs is
  remapped to a fresh id and **every `EntityId` reference contributed by
  theirs is deep-rewritten** — through lists, tuples, sum types, nested
  components, and maps (keys included). The remap happens before any
  comparison, so a reference to a colliding spawn can never spuriously
  equal an ours-side reference.
- **Names are identity.** Two forks claiming the same name for different
  entities is a real conflict (`names are identity`). Renames three-way
  merge like any field.
- **Despawn rules.** Despawn vs. untouched → despawn wins. Despawn vs.
  modified → conflict. Component removal follows the same logic.
- **Conflicts are data, not prose.** `Err` carries a `list<Conflict>` — a
  built-in sum type whose variants carry the subject and all three diverging
  values. A resolution policy is a `match` in user code, never string
  parsing. The variants:

  | variant | fields | meaning |
  |---|---|---|
  | `FieldConflict` | `ent, name, comp, field, base, ours, theirs` | same field diverged in both forks (resolvable: a value) |
  | `ResourceFieldConflict` | `res, field, base, ours, theirs` | same resource field diverged (resolvable: a value) |
  | `ComponentConflict` | `ent, name, comp, detail` | removed-vs-modified, added-both, layout drift |
  | `DespawnConflict` | `ent, name, detail` | despawned in one fork, modified in the other |
  | `RenameConflict` | `ent, base, ours, theirs` | renamed differently in both forks (resolvable: the chosen name) |
  | `NameConflict` | `name, entities` | one name claimed by several entities (resolvable: a list of new names) |
  | `ResourceConflict` | `res, detail` | resource initialized in both forks, layout drift |
  | `EventConflict` | `detail, base, ours, theirs` | in-flight events consumed or reordered |

  The `ent` field is a live entity handle — a policy can `get()` other
  components off the conflicting entity to make its decision.
- **In-flight events merge too — never silently dropped.** Emission is
  append-only within a fork, so base's pending queue must be a prefix of
  each branch's queue; the merged fork carries base's events, then ours'
  post-fork emissions, then theirs' (with entity references in theirs'
  payloads rewritten through the id remap). `commit(merged)` +
  `flush_events()` fires all of them. If a branch *consumed* events the
  other still carries (it called `flush_events()` after the fork), there is
  no honest automatic answer to "did those handlers run?" — the merge
  refuses with an `in-flight events` conflict instead of guessing.

The merged world is rebuilt canonically (sorted entity/component order)
through the engine's own operations, so archetype, index, name-map, and
id-allocator invariants hold by construction, and `merge_forks(base, a, b)`
agrees with `merge_forks(base, b, a)` wherever no remap is involved.

When RFC-0003 authoritative relation state differs between `base`, `ours`,
and `theirs`, merge currently returns a typed relation conflict before
mutating anything. This is deliberately fail-closed: a correct future merge
must preserve assertion lifetimes, named unique constraints, generational
entity references, restrict/cascade behavior, and causal ancestry.

**Programmable resolution: `merge_forks_with()`.** Pass a list of
`(conflict, resolution)` pairs and the merge applies them instead of
refusing. What counts as a resolution depends on the conflict:

- `FieldConflict` / `ResourceFieldConflict` — the value the merged world
  should carry.
- `NameConflict` — a list of new names, one per claiming entity (in the
  conflict's `entities` order; `""` unnames). Names are semantic identity,
  so the machine never picks — but "keep both, as `T-5/a` and `T-5/b`" is a
  complete human answer. The merge **re-validates** after renaming: chosen
  names that still collide (with each other, or with an entity the forks
  never touched) come back as conflicts, so a rename can never steal a name
  unnoticed.
- `RenameConflict` — the one name the entity should carry.
- Despawns and event consumption have no honest "pick a side" and stay
  unresolvable.

The sync policy lives in user code:

```rad
fn rank(s: str) -> int {
    if s == "closed" { return 3 }
    if s == "escalated" { return 2 }
    return 1
}

match merge_forks(base, ours, theirs) {
    Ok(m) => { commit(m) }
    Err(conflicts) => {
        let mut decisions = []
        for c in conflicts {
            match c {
                FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                    if comp == "Ticket" and field == "status" {
                        let mut pick = ours              // precedence: closed > escalated > open
                        if rank(theirs) > rank(ours) { pick = theirs }
                        decisions = push(decisions, (c, pick))
                    }
                    if comp == "Ticket" and field == "assignee" {
                        decisions = push(decisions, (c, theirs))   // pusher wins
                    }
                }
                _ => {}
            }
        }
        let m = merge_forks_with(base, ours, theirs, decisions) |> unwrap
        commit(m)
    }
}
```

Name claims resolve the same way — two offline clients both minting `T-5`
is one rename away from a clean merge:

```rad
match c {
    NameConflict { name, entities } => {
        // keep both: first claimant (ours) and second (theirs, remapped)
        decisions = push(decisions, (c, [f"{name}/a", f"{name}/b"]))
    }
    _ => {}
}
```

Unnamed conflicts still come back as `Err` — a policy resolves exactly what
it names, nothing silently.

| builtin | type | effect |
|---|---|---|
| `merge_forks(base, ours, theirs)` | `(world_fork, world_fork, world_fork) -> Result<world_fork, list<Conflict>>` | `ecs` |
| `merge_forks_with(base, ours, theirs, resolutions)` | `(world_fork, world_fork, world_fork, list<(Conflict, any)>) -> Result<world_fork, list<Conflict>>` | `ecs` |

(`merge` remains the map-merge builtin; `merge_forks` is its world-scale
sibling.) This is the convergence point of the whole list: **fork** futures
(#1), **diff** them (#3), **merge** the ones you want (#7), `commit` the
result — speculative execution with reconciliation, as language primitives.
See `projects/dogfood/worldmerge/main.rad`, and `projects/dogfood/opsdesk/` for all seven
features running as one machine in one program (migrate a v1 save, forecast
with simulate, merge two shifts with an in-flight event, fence the merge
with `assert_only_changed`, audit with `why()`, record and replay the whole
session bit-for-bit).
