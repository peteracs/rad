# Components, Systems (ECS), & Resources

The Entity Component System is Rad's core architecture. Per-entity data lives in **components**, global singleton data lives in **resources**, behavior lives in **systems**, and **entities** are IDs that tie components together.

## Components

A component is a named data bag with default values:

```
component Position { x: 0.0, y: 0.0 }
component Velocity { dx: 0.0, dy: 0.0 }
component Health { hp: 100, max: 100 }
component Name { value: "" }
```

Components are pure data — no methods, no behavior.

### Required fields

A field declared with a type but no default (`name: type`) is **required**:
every construction site must provide it, because no zero value would be
honest. The canonical case is an owner reference:

```
component Missile {
    source: entity,        // required — a missile without an owner is a bug
    speed: float = 40.0,   // defaulted — may be omitted
}

let m = entity "bolt" { Missile { source: caster } }   // ok
let m = entity "bolt" { Missile { speed: 50.0 } }      // error: missing 'source'
set(m, Missile { speed: 60.0, ..require(m, Missile) }) // ok: spread carries it
```

Resources reject required fields at declaration — they auto-initialize from
defaults, so there is no construction site to demand them at.

### Indexed fields

Prefix a field with `indexed` to create a runtime hash index for O(1) entity lookup by that field's value:

```
component Username { indexed name: "" }
component Email    { indexed address: "" }
```

Only hashable types (`int`, `float`, `str`, `bool`, `entity`) may be indexed. The index is maintained automatically when components are added, removed, or modified via `set()`.

Use the `lookup()` builtin to find an entity by an indexed field value:

```
let hero = spawn(Username { name: "Hero" })
let found = lookup(Username, "name", "Hero")

match found {
    Some(id) => { print("Found entity:", id) }
    None => { print("Not found") }
}
```

`lookup()` returns `Some(entity_id)` or `None`. It is an O(1) operation backed by a hash map, compared to `query_where` which scans all matching entities. Use indexed fields when you need fast singleton lookups (e.g., finding a player by username, a tile by coordinate).

With duplicate keys, `lookup()` returns the **lowest** entity id, and
`lookup_all()` returns every match with ids ascending — the multi-match
view ("all open tickets") as one hash probe. Measured at 50k entities,
`lookup_all` answered the same question ~675x faster than the scan
(0.2 ms vs 135 ms per query). Indexes survive `fork`/`commit`, the wire
codec, saves, deltas, merges, and schema migration; the battle tests in
`core/vm/src/index_tests.rs` pin all of it.

> **Note:** Calling `lookup()` on a non-indexed field produces a runtime error. The checker validates that indexed fields use hashable types at compile time.

## Entities

An entity is just an ID. You create one with `spawn` and attach components with `set`:

```
let hero = spawn()
set(hero, Name { value: "Hero" })
set(hero, Health { hp: 100, max: 100 })
set(hero, Position { x: 0.0, y: 0.0 })
```

When you know all the components up front, an **entity literal expression** is more concise — it spawns the entity, attaches every component, and returns the ID in a single expression:

```
let hero = entity {
    Name { value: "Hero" },
    Health { hp: 100, max: 100 },
    Position { x: 0.0, y: 0.0 }
}
```

To create a **named** entity (retrievable via `get_entity()`), place a name expression between `entity` and `{`:

```
// String literal name
let e = entity "player" { Health { hp: 100 }, Position { x: 0, y: 0 } }

// Variable name
let path = "assets/hero.rad"
let file = entity path { FilePath { path: path }, Unparsed {} }

// Computed name
let npc = entity f"npc_{id}" { Name { value: n } }

// Retrieve by name later
let found = get_entity("player")
```

Entity literals work anywhere an expression is expected — let-bindings, function arguments, return values:

```
register_npc(entity { Name { value: "Goblin" }, Health { hp: 30, max: 30 } })
```

Component entries can also be **expressions** — variables, function calls, or any expression that evaluates to a component value — alongside traditional `Component { ... }` initializers:

```
fn make_health(hp: int) -> Health {
    return Health { hp: hp, max: hp }
}

let pos = Position { x: 1.0, y: 2.0 }
let hero = entity { Name { value: "Hero" }, pos, make_health(100) }
```

Use `spawn` + `set` when you need to add components conditionally or over time. Use entity literals when the full set of components is known at the call site. Use named entity literals when you also need the entity to be retrievable by name.

Read components with `get` (safe/optional) or `require` (must exist), check existence with `has`, remove with `remove`:

```
let name = require(hero, Name)
print(name.value)

if has(hero, Health) {
    print("alive")
}

// remove(hero, Position)
// despawn(hero)
```

When a component is optional, `map_or` keeps the branch concise:

```
let hp = map_or(get(hero, Health), 0, fn(h) { return h.hp })
print(hp)
```

## Updating components with spread

When you only need to change one or two fields on a component, use **spread syntax** (`..base`) to copy the rest from an existing value:

```
let h = require(hero, Health)
set(hero, Health { hp: h.hp - 10, ..h })
```

The `..h` fills every field you didn't explicitly list — `max` keeps its current value. Without spread, you'd have to retype every field:

```
// Don't do this — verbose and error-prone
set(hero, Health { hp: h.hp - 10, max: h.max })
```

Spread works with any number of overrides and any component size:

```
component Stats { hp: 100, max_hp: 100, attack: 10, defense: 5, speed: 3, luck: 1 }

set(hero, Stats { hp: 100, max_hp: 100, attack: 10, defense: 5, speed: 3, luck: 1 })
let s = require(hero, Stats)
set(hero, Stats { hp: s.hp + 20, luck: s.luck + 1, ..s })
```

Rules:
- The spread base must be the **same component type** (the type checker enforces this)
- At most one `..base` per component literal
- Explicit fields override the spread — fields you list always win

## Updating components with `update`

When you only need to change a few fields, `update` is even shorter than spread:

```
update(hero, Health) {
    hp = 50
}
```

This reads the current `Health` from `hero`, overrides `hp` to `50`, and writes it back. It is equivalent to:

```
let h = require(hero, Health)
set(hero, Health { hp: 50, ..h })
```

`update` evaluates the entity expression exactly once. The checker validates that:
- The component exists
- All field names are valid
- Each assigned value matches the field's declared type

Multiple fields can be updated at once:

```
update(hero, Stats) {
    hp = 100,
    luck = s.luck + 1
}
```

List and map fields take **element-level updates** — one index level per
entry, applied in written order:

```
component Loadout { shields: list = [0, 0, 0], items: map<str, int> = {} }

update(hero, Loadout) {
    shields[1] = 250,          // list element (int index, bounds-checked)
    items["potion"] = 3,       // map entry (insert-or-replace)
}

// plain assignment seeds the base, later indexed entries patch it
update(hero, Loadout) { shields = [9, 9, 9], shields[1] = 0 }
```

Each indexed entry lowers to `set_at(...)` on the field's current value, so
the same operation is available as a pure expression: `set_at(xs, i, v)`.
For nested structures, fetch the component and assign the whole element:
`rows[i] = set_at(c.rows[i], j, v)`.

Use `update` for simple field patches. Use `set` with spread when you need computed spreads or conditional logic in the same expression.

## Entity Lifetimes & Best Practices

Entities are **not garbage collected**. The backup GC (`GcHeap`) traces closure/capture-cell graphs and bytecode chunk constants — it never scans ECS world columns, snapshots, or event timelines. If you spawn an entity and lose its ID, it will live in the `World` forever. You must explicitly call `despawn(id)` to remove an entity.

When an entity is despawned, its ID is pushed to a free list and will be recycled by a future `spawn()` call. This guarantees that long-running programs (like servers or simulations) will never exhaust the 2^32 entity ID limit. Furthermore, `despawn(id)` explicitly clears all of the entity's components: heap-backed field values (strings via `Arc<str>`, nested objects via `Arc<Object>`) have their persistent refcounts released, freeing the underlying data when no other column or fork shares it.

Here are the three best practices for managing data lifetimes in Rad:

### 1. Local Data: Use `struct` instead of `spawn()`
If you just need a temporary bundle of data that lives only as long as a function, **do not use the ECS**. Use a `struct`. Struct instances live in the per-system arena (`BumpArena`) or on the GC heap depending on context, and are cleaned up automatically — no manual `despawn` needed.

```rad,ignore
// BAD: Spawning an entity just to hold temporary data
let temp = spawn(MathParams { x: 10, y: 20 })
let result = calculate(temp)
despawn(temp) // Easy to forget!

// GOOD: Use a struct
struct MathParams { x: 0, y: 0 }
let temp = MathParams { x: 10, y: 20 }
let result = calculate(temp)
// Cleaned up automatically when the arena resets or GC collects
```

### 2. Temporary Entities: Data-Driven Lifetimes
For entities that need to exist in the world temporarily (like particles, projectiles, or temporary buffs), attach a `Lifetime` component and let a system clean them up. This avoids manually tracking IDs in arrays.

```rad
component Lifetime { timer: 0.0 }

fn despawn_expired(delta_time: float) {
    for (id, l) in query { mut Lifetime } {
        l.timer = l.timer - delta_time
        if l.timer <= 0.0 {
            despawn(id)
        }
    }
}
```

### 3. Bulk Cleanup: Tagging and Queries
For large groups of entities that share a lifecycle (like all enemies in a level, or all AST nodes in a compiler pass), tag them with a component and use a bulk query to despawn them all at once.

```rad
component Scene { name: "" }
component Enemy { hp: 0 }
component Obstacle { solid: false }

// Spawn entities with the scene tag
spawn(Scene { name: "level_1" }, Enemy { hp: 100 })
spawn(Scene { name: "level_1" }, Obstacle { solid: true })

// Later, clean up the entire scene in 3 lines:
for (id) in query { Scene } where Scene.name == "level_1" {
    despawn(id)
}
```

### 4. Optional Entities: Use `entity | nil`
When a component needs to reference another entity, but that reference is optional (e.g., a target that might not exist yet), do **not** use a magic number or a dummy entity. Instead, use Rad's union types to explicitly allow `nil`:

```rad
component Follow {
    // A default value of `nil` allows it to hold an entity ID or nil.
    target: entity | nil = nil
}
```

> **The Nil Inference Trap:** If you write `target: nil` without the `entity | nil` annotation, the compiler infers the field's type as *strictly* `nil`. Later, when you try to assign an entity to it, you will get a type error (`expected nil, got entity`). Always explicitly annotate optional fields!

Because you explicitly defined the union, the compiler forces you to check for `nil` before using the entity ID. Guard clauses count: an early exit on the nil branch narrows the binding for the rest of the scope:

```rad
let target = f.target
if target == nil { return }      // the early exit IS the else branch
walk_toward(require(target, Position))   // target: entity from here on
```

```rad
if f.target != nil {
    let target_pos = require(f.target, Position)
}
```

Because `nil` is a distinct runtime value and not an entity ID, the ECS world never accidentally tries to look up a "none" row, and you don't have to reserve `0` or `u32::MAX`.

## Resources

A resource is a global singleton — data that exists once, not per-entity. Use resources for game config, global counters, shared state that doesn't belong to any specific entity:

```
resource ClusterPool { free_workers: 2, finished: 0 }
resource GameConfig { gravity: 9.81, debug: false }
```

Resources are pure data, just like components — no methods, no behavior.

### Reading and writing resources

Declared resources auto-initialize from their field defaults, so the
shortest read is `res(R)` — the value itself, no Option:

```
print(res(ClusterPool).free_workers)
let cfg = res(GameConfig)
print(cfg.gravity)
```

The checker types `res(R).field` precisely (typos get "No field ... on
resource" with the field list) and rejects `res(SomeComponent)`. Like
`get()`, it carries the `readonly` effect: fine in `readonly fn`,
rejected in `pure fn`.

`get_resource` (returns `Option`) remains for when you want to
pattern-match presence explicitly, and `set_resource` replaces the whole
value:

```
let pool = get_resource(ClusterPool)?
set_resource(ClusterPool, ClusterPool { free_workers: 1, finished: pool.finished + 1 })
```

The `update` sugar works without an entity, and composes with `res()`
into a one-line read-modify-write:

```
update(ClusterPool) { finished = res(ClusterPool).finished + 1 }
```

### Resources in systems

Systems can accept resource parameters alongside component parameters. Mark resources `mut` for write access:

```
resource DamageLog { total: 0 }
component HP { value: 100 }

system Hit(h: mut HP, d: mut DamageLog) {
    let dmg = 25
    h.value = h.value - dmg
    d.total = d.total + dmg
}
```

This system iterates all entities with `HP`, injecting the same `DamageLog` resource on each iteration. The resource accumulates damage across all entities.

A **resource-only** system (no component parameters) runs exactly once per schedule invocation:

```
resource Counter { n: 0 }

system Tick(c: mut Counter) {
    c.n = c.n + 1
}
```

### Where to put shared-state mutation

A schedule may run its systems in parallel (see [Parallel scheduling](#parallel-scheduling)). That makes **where** you mutate state a design decision, not an afterthought:

- **A system mutates the components of the entity it is iterating.** `system Hit(h: mut HP, ...)` writes each entity's own `HP` — every entity is a disjoint write, so it parallelises cleanly and deterministically.
- **A handler mutates shared resources.** Handlers are **serial by design** (they run one at a time during event flush), so folding many contributions into one resource is well-defined there no matter how the systems that emitted the events were scheduled.

So the robust pattern for cross-entity aggregation is **systems detect and `emit`; handlers count and mutate the shared resource**:

```
resource Damage { total: 0 }
component HP { value: 100 }
event Hit { amount: 0 }

system Strike(h: mut HP) {
    let dmg = 25
    h.value = h.value - dmg
    emit Hit { amount: dmg }        // detect: write my own component, announce the rest
}

on Hit(e) {
    update(Damage) { total = res(Damage).total + e.amount }   // aggregate: serial, exact
}
```

Accumulating directly into a `mut` resource from inside an entity-iterating system (`d.total = d.total + dmg` on a `mut DamageLog` parameter) also works, but it couples your books to the scheduler's merge; routing the aggregation through a handler keeps it correct by construction because handler execution is serial.

**`accum` makes in-system accumulation first-class.** Declaring the parameter `d: accum DamageLog` puts the reduction in the signature: the parameter is writable like `mut`, and in a parallel batch each worker's per-field *delta* is folded into the base in schedule order — associative, order-independent in effect, and deterministic (floats included, because the fold order is fixed). Two `accum`-writers of the same resource may share a batch; a plain reader or writer of it still serializes against them. The fold is **additive**, and the checker enforces the contract: `accum` is only valid on resource parameters whose fields are all `int`/`float`. Non-additive aggregation (min/max, sets, logs) still belongs in a handler:

```
resource Damage { total: 0 }
component HP { value: 100 }

system Strike(h: mut HP, d: accum Damage) {
    h.value = h.value - 25
    d.total = d.total + 25          // folded exactly, even in a parallel batch
}
```

If you want ordering by declaration instead of by data analysis, three levers steer the scheduler directly — none of them affects explicit `simulate_par`/`simulate_many`:

- `schedule serial [A, B, C]` — this one call runs its systems one at a time in topological order (no worker snapshots, no merge).
- `serial phase Line [A, B]` — members of the phase never share a batch with each other, in any schedule that runs them.
- `rad run --serial-schedule` — the global flag: every schedule in the program runs serially, the one-command differential test against the parallel path.

### Resource vs component

The checker enforces a clean separation:
- `spawn(Resource)` and `entities(Resource)` are compile errors
- `get_resource(Component)` and `set_resource(Component, ...)` are compile errors
- A resource and a component cannot share the same name
- Duplicate `resource` declarations are rejected
- `update(Resource)` and `set_resource(Resource, ...)` are rejected inside a system that holds the same resource as a `mut` parameter (the writeback would silently overwrite the explicit mutation)

Resources participate in parallel conflict analysis: two systems that both hold a mutable reference to the same resource are serialized. Resources are included in `fork()`/`simulate()`/`commit()` snapshots.

### Transient resources

`transient resource` declares state that is excluded from the world's *identity*: `world_digest()` and `save_world()` skip it, while forks and `commit()` still carry its values. Use it for metadata and derived state — command tapes, caches, spatial indexes — anything that describes or accelerates the simulation without being part of it:

```
transient resource Tape { orders: list = [] }
```

A replay tape recorded into a transient resource leaves the match digest untouched, which is what makes tape-driven replay bit-exact by construction.

## Systems

A system is a function that runs on every entity matching a component signature:

```
system Physics(pos: mut Position, vel: Velocity) {
    pos.x = pos.x + vel.dx
    pos.y = pos.y + vel.dy
}
```

`Physics` runs once for every entity that has both `Position` and `Velocity`. The `mut` keyword marks which components the system can write to. That information lets the runtime partition non-conflicting systems into batches (see `core/vm/src/vm/parallel.rs`). Systems in the **same batch** can run **in parallel** when the schedule executes (see [Parallel scheduling](#parallel-scheduling) below).

Inside a system body, **`self` is the entity being visited** — use it to emit
events about the unit, read components outside the signature, or detach tags:

```
system age_buffs(b: mut Buffs) {
    emit BuffExpired { unit: self, buff: "ghost" }
    remove(self, StatsDirty)
}
```

A **bare component name** is a type-only filter param: the system only visits
entities carrying it, without binding data you'd never read — the
tag-component idiom:

```
component StatsDirty {}    // zero-field tag

system recompute_stats(StatsDirty, base: BaseStats, cs: mut CombatStats) {
    // runs only for dirty units; nothing named `d` left unused
}
```

## Runtime Queries

While systems execute across all matching entities automatically, sometimes you need to query entities manually in user space (e.g., finding the nearest enemy, counting active players). Rad provides a built-in `query` expression that operates directly on archetype storage for maximum performance:

```rad
// Find all entities with Health
let all_health = query { Health }

// Find all entities with Position and Health, filtered by a predicate
let alive_enemies = query { Position, Health } where Health.hp > 0
```

By default, `query` returns a list of entity IDs. If you want to extract the component data directly, you can use the `select` clause:

```rad
// Returns a list of Health components
let healths = query { Health, Position } select Health

// Returns a list of tuples containing (Health, Position)
let pairs = query { Health, Position } select Health, Position where Health.hp > 0
```

When selecting multiple components, the result is a list of tuples. Use bracket destructuring in for-loops or pipelines to name each element:

```rad
let pairs = query { Health, Position } select Health, Position

// Destructure in a for-loop
for [hp, pos] in pairs {
    print(f"HP: {hp.hp} at ({pos.x}, {pos.y})")
}

// Destructure in a pipeline
let low_hp = pairs
    |> filter(fn([hp, pos]) { return hp.hp < 50 })
    |> map(fn([hp, pos]) { return pos })
```

These queries are much faster than manually iterating over all entities because they leverage the ECS's internal Structure-of-Arrays (SoA) layout.

### Structural exclusion (`without`)

The ECS `query` API matches entities that have **all** listed components and **none** of a set of excluded types. For simple filters, the compiler **hoists** negations from `where` into that structural exclusion pass so fewer entities reach the predicate.

You can express exclusion with `!` on a component name inside `where`, combined with `&&` for multiple exclusions:

```rad
// Entities with A and B but not C (C checked structurally, not per-entity in the filter)
let xs = query { A, B } where !C

// Multiple exclusions: !Dead && !Paused
let alive = query { Position } where !Dead && !Paused
```

Only straightforward patterns are hoisted (e.g. `!ComponentName`, combined with `&&`). More complex `where` clauses still run as a follow-up filter on the candidate set returned by the archetype query.

## Mutable Query Loops

If you need to iterate over entities and mutate their components outside of a `system`, you can use a mutable query loop. This avoids the verbose read-modify-write cycle of `require` and `set`.

```rad
// Directly mutate Health for all entities
for (h) in query { mut Health } {
    h.hp = h.hp - 10
}

// You can also bind the entity ID (parentheses are optional but recommended for multiple bindings)
for (id, pos, vel) in query { mut Position, Velocity } {
    pos.x = pos.x + vel.dx
    pos.y = pos.y + vel.dy
    if pos.y < 0.0 {
        despawn(id)
    }
}
```

The runtime automatically writes back any changes to the `mut` bindings at the end of each loop iteration. Note that mutable queries are *only* allowed directly in `for` loops to prevent aliasing issues.

## System ordering

Systems can declare execution order with `before` and `after`:

```
system Render(p: Position) after Physics {
    print(p.x, p.y)
}
```

`Render` always runs after `Physics` has finished updating positions.

When you group systems in a `schedule [...]` block, the runtime topologically sorts them based on these dependencies, then partitions them into conflict-free batches. You can list systems as **`alias.Sys`** or **`system::path::Sys`** (same `system::` paths as in `simulate`).

## Parallel scheduling

Systems in the **same batch** do not conflict on reads/writes (see `core/vm/src/vm/parallel.rs`). The VM runs **multi-system batches in parallel**: each system executes against a **snapshot** of the world, then the runtime merges ECS commands and events back on the main thread.

**Determinism:** The order in which parallel workers finish is **not** fixed. If two systems in the same batch could write conflicting data for the same entity, the **final** world state can depend on merge order. The partitioner is designed so that **should not** happen for systems that share a batch; treat cross-batch ordering as the source of truth (`before` / `after`).

**Events:** Events emitted from parallel systems are collected and sorted deterministically (by trace id, then name) before being queued for the next frame.

**Memory:** Each worker VM uses a private scratch arena (`BumpArena`). ECS commands and deferred events are applied on the main thread; heap-backed payloads are **deep-copied** into persistent ECS storage (`Arc<Object>` with retain/release via `ValueColumn`). Strings use `Arc<str>`, so crossing the air gap is O(1) per string. This is separate from merging **compiler** or **WASM chunk** heaps into the VM (those still use `GcHeap::merge` so bytecode constants stay valid).

## World Forking (Speculative Execution)

Rad lets you fork the entire ECS world, run systems in isolation, inspect results, and optionally commit the changes back — all as first-class builtins.

```rad
component Health { hp: 100 }
component Attack { damage: 10 }

system Combat(h: mut Health, a: Attack) {
    h = Health { hp: h.hp - a.damage }
}

let hero = spawn("hero", Health { hp: 100 }, Attack { damage: 5 })
let boss = spawn("boss", Health { hp: 200 }, Attack { damage: 15 })

// Fork → simulate 5 ticks → inspect without committing
let future = simulate(fork(), [system::Combat], 5)

let hero_preview = peek(future, hero, Health)?
let boss_preview = peek(future, boss, Health)?

if hero_preview.hp > 0 {
    commit(future)  // Apply the simulated state
}
```

### How it works

| Function | What it does |
|---|---|
| `fork()` | Snapshot the ECS world. Uses copy-on-write (`Arc` refcount bumps) — O(A) cost regardless of entity count. |
| `simulate(fork, systems, ticks)` | Run named systems on the fork for N ticks. IO and `commit()` are statically forbidden; emitted event handler chains must also be simulation-safe. |
| `peek(fork, entity, Component)` | Read a component from the fork without committing. Returns `Option`. Values are deep-copied across the air gap (strings O(1) via `Arc<str>`). |
| `commit(fork)` | Replace the live world with the fork. **Clears all pending events.** |

### Events inside simulations

Systems passed to `simulate()`/`simulate_par()` **may emit events**. Emitted events dispatch *inside* the fork — the simulation flushes once per simulated tick, exactly like the live loop — so event-driven architectures (damage events, death events, cascades) simulate without rewrites. Nothing leaks: the live event queue and the live delayed (`emit … after`) queue are untouched by anything a simulation does.

Safety moves to the handlers: the checker walks every handler transitively reachable through a system's emits (handlers may emit further events), and rejects the system if any of them performs IO:

```
Error: System 's' cannot be used in simulate(): handler `on Pong` calls IO builtin 'print' (forbidden in simulation)
```

### Purity enforcement

The type checker statically prevents systems used in `simulate()` from performing IO or calling `commit()`, including through any handler chain reached by emitted events. This guarantees that simulated futures cannot have side effects on the real world.

### Use cases

- **AI decision-making**: Evaluate multiple strategies in parallel, peek at outcomes, commit the best one
- **Transaction preview**: Project financial or game state forward N steps before deciding to apply
- **Undo/rollback**: Fork before a risky operation, discard the fork if it goes wrong
- **What-if analysis**: Compare multiple futures without modifying any state

See the [Built-in Functions reference](../reference/builtins.md) for full API details.
