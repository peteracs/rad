# Rad Memory Model — Design Document

## Status: Implemented

## Overview

This document specifies Rad's memory management strategy for the compiled (Rust VM) version. The original Python prototype used Python's GC. The initial Rust VM used Reference Counting. The current VM uses a high-performance, custom-built architecture designed for C-level performance.

## Design Principles

1. **Simple mental model** — Developers should not think about memory.
2. **No GC pauses** — Rad targets game developers. Frame-time consistency matters.
3. **No borrow checker** — Rad's pitch is "simpler than Rust." Adding ownership/borrowing contradicts this.
4. **Predictable performance** — Memory operations should have bounded, predictable cost.

## Chosen Strategy: NaN-boxing + copy-on-write containers + four-layer runtime memory

### Value Types (NaN-Boxed)

Rad uses **NaN-boxing** to fit all runtime values into a single 64-bit word (`u64`). This makes the core `Value` type `Copy` in Rust — cloning a value is a trivial bit-copy with zero overhead.

| Type | Size | Notes |
|---|---|---|
| `float` | 8 bytes | 64-bit IEEE 754 (standard representation) |
| `int` | 8 bytes | Unboxed 48-bit signed integer stored inline in the NaN payload |
| `bool` | 8 bytes | Tagged NaN |
| `nil` | 8 bytes | Tagged NaN |
| Heap Pointers | 8 bytes | Tagged NaN containing a 48-bit raw pointer |

By unboxing 48-bit integers, the vast majority of integer arithmetic in Rad requires zero heap allocation.

### Lists and maps (copy-on-write vs persistent maps)

- **Lists** are backed by **`Arc<Vec<Value>>`**. Assignment is a cheap pointer copy (`Arc::clone`). Mutations use `Arc::make_mut`: if the list is uniquely owned, the vector is updated in place; if shared, the vector is cloned once and then mutated.
- **Maps** are backed by **HAMTs** (`im::HashMap`) for structural sharing and $O(\log n)$-style updates with sharing.

When you assign a list to a new variable or pass it to a function, it is a cheap pointer copy. When you `push` a new value, builtins return a new list value; the runtime avoids full deep copies of unrelated elements when the backing buffer can be updated in place.

```rad
let mut a = [1, 2, 3]
let b = a
a = push(a, 4)
// `a` is [1, 2, 3, 4]
// `b` is [1, 2, 3]
// Both share the memory for the first 3 elements.
```

### Component Data (Strict Structure-of-Arrays ECS with Copy-on-Write)

Components are the core data type in Rad. They have **value semantics** from the user's perspective, but under the hood, they are stored in a highly optimized **Structure-of-Arrays (SoA)** layout with **copy-on-write** semantics.

- When a component is stored in the ECS world via `set()`, its fields are deep-copied into persistent storage and pushed into flat, contiguous `ValueColumn` vectors (wrapped in `Arc` for copy-on-write).
- When a system iterates over entities, it iterates directly over these flat arrays. This provides maximum CPU cache utilization.
- `ComponentData` objects only exist as transient views when components are extracted into local variables.

**Copy-on-Write architecture:** Each per-field column vector is wrapped in `Arc<ValueColumn>` (a custom type with `Clone` that retains persistent `Arc<Object>` refs and `Drop` that releases them). The archetype's entity list and row index are `Arc<Vec<u32>>` and `Arc<HashMap<u32, usize>>` respectively. All top-level World bookkeeping maps (`entity_archetype`, `name_to_id`, `id_to_name`, `type_registry`, `archetype_map`) are `Arc`-wrapped.

When `fork()` creates a world snapshot, it performs `Arc::clone` on each column’s `Arc<ValueColumn>` and on world maps — **O(A)** shallow refcount bumps (A = archetype column `Arc`s), not a per-entity data copy. Actual **SoA data** cloning is deferred to the first mutation of a still-shared column via `Arc::make_mut()`. That path may clone an entire `ValueColumn`, which runs an **O(E)** scan over the entities in that column to `retain_persistent()` on heap-backed field values (strings, nested objects); **primitive-only columns** mostly hit a cheap bitwise `is_persistent_object` check with no atomic work. Untouched columns stay shared. This keeps fork nearly instant for typical worlds (~7µs at 10,000 entities), while the retain cost is paid only when a shared column is written.

```rad
component Position { x: 0.0, y: 0.0 }
component Velocity { dx: 0.0, dy: 0.0 }

// In a system with mut access, modifications are in-place in the SoA arrays.
// Arc::make_mut clones only the Pos columns (Vel stays shared).
system Move(pos: mut Position, vel: Velocity) {
    pos.x = pos.x + vel.dx
    pos.y = pos.y + vel.dy
}
```

### Runtime memory layers

Rad now uses a four-layer model:

1. **Ephemeral system arena (`BumpArena`)** for per-system temporaries. It resets between system runs.
2. **Static VM state** (builtins, chunk metadata, immutable config) allocated once per VM lifecycle.
3. **Persistent ECS storage** for world data written by `set` / `spawn` / `set_resource` escape paths. Global `resource` singletons are stored in a separate `Arc<HashMap<String, ComponentData>>` alongside entity-component archetypes, with the same CoW and air-gap semantics.
4. **Backup closure collector (`GcHeap`)** used for closure/capture-cell graphs and triggered explicitly by `gc_collect()`.

Important implications:

- `Value` stays NaN-boxed and `Copy`; inline ints/floats/bools avoid refcount traffic. Heap strings use **`Arc<str>`** inside `Object::Str`, so copying a string across the ECS “air gap” (e.g. `get` / `peek` deep-copying into the backup GC heap) is **O(1)** per string (Arc bump), not an O(n) byte copy.
- ECS data is no longer part of the tracing collector root set.
- Worst-case tracing work is bounded by live closure/capture graphs plus bytecode chunk constants, not by world size.

### Entity Lifetimes (Not GC'd)

It is a common misconception that entities are garbage collected when their `entity` ID variable goes out of scope. **This is false.**

1. **The World is the Owner:** The `World` owns component data in flat SoA arrays and persistent ECS storage.
2. **Entities are Data:** An entity is a concept, a relationship between arrays. It is not an object. The GC ignores entity IDs.
3. **Manual Despawn:** You must explicitly call `despawn(id)` to remove an entity from the ECS. 

For bulk cleanup (like unloading a level) or temporary entities (like particles), use **Data-Driven Lifetimes** (e.g., a `Scene` component or a `Lifetime` component managed by a system).

### Unique Bindings (`let unique`)

The `let unique` keyword provides an opt-in single-ownership guarantee at compile time. The checker ensures a `unique` binding is never aliased — it cannot be assigned to another variable, passed as a function argument, or captured by a closure. This means the runtime can guarantee that `Arc::make_mut` on a unique list will **never** trigger a deep clone, because the backing `Arc` always has a reference count of 1.

```rad
let unique mut xs = [1, 2, 3]
xs << 4       // guaranteed in-place: Arc refcount is always 1
xs << 5       // no clone, no allocation — just Vec::push
```

Without `unique`, assigning `let b = a` bumps the `Arc` refcount to 2. A subsequent mutation on `a` would clone the entire vector. With `unique`, the compiler rejects `let b = xs` at compile time, ensuring the refcount never rises above 1.

Use `unique` for hot-path accumulators, large lists that are built incrementally, or any binding where an accidental alias would cause a performance cliff.

### Copy Profiling (`--profile-copies`)

Run with `--profile-copies` to surface hidden O(n) deep clones at runtime. Whenever a list mutation (`push`, `set`, `extend`) triggers `Arc::make_mut` on a shared backing buffer (reference count > 1), the VM emits a diagnostic to stderr:

```
[copy-profile] line 42: deep clone of 10000-element list (Arc refcount was 2)
```

This helps identify performance bottlenecks where an unintended alias causes a full vector clone in a hot loop. The flag has negligible overhead when no copies occur.

### Immutability and Pipelines

Rad's `let` bindings are immutable. 

```rad
let data = [1, 2, 3]
// data[0] = 99  // ERROR: cannot mutate immutable binding

let mut data2 = [1, 2, 3]
data2[0] = 99  // OK
```

**Pipeline immutability:** Pipeline operations (`map`, `filter`, `reduce`) always return **new** lists. The fused pipeline path avoids allocating intermediate Rad lists between stages; list elements are still held in contiguous storage.

```rad
let original = [1, 2, 3, 4, 5]
let doubled = original |> map(fn(x) { return x * 2 })
// original is still [1, 2, 3, 4, 5]
// doubled is [2, 4, 6, 8, 10]
```

## Performance Characteristics

| Operation | Cost | Notes |
|---|---|---|
| Int/float/bool assignment | $O(1)$ | Stack copy (NaN-boxed) |
| Component field access (in System) | $O(1)$ | Direct pointer offset into contiguous `Arc<Vec<Value>>` |
| `get(entity, Component)` | $O(F)$ | Archetype lookup + **air-gap** deep-copy of **F** fields into the GC heap; string fields are **O(1)** each via shared `Arc<str>` |
| `get_resource(Type)` | $O(F)$ | Resource lookup + air-gap deep-copy of **F** fields; same semantics as `get` |
| `set_resource(Type, value)` | $O(F)$ | Deep-copy fields into persistent resource storage |
| `fork()` (snapshot) | $O(A)$ | A = shallow `Arc` refcount bumps on column handles/maps; no per-entity column clone |
| `peek(fork, entity, Component)` | $O(F)$ | Same air-gap deep-copy as `get` into the GC heap (safe if the entity is despawned later) |
| `commit(fork)` | $O(1)$ | Pointer swap of Arc-wrapped maps and column refs |
| First mutation after `fork()` | $O(E)$ | For a shared column, `Arc::make_mut` may clone the whole `ValueColumn` (E = rows); primitive-heavy columns stay cheap |
| String literal / new string | $O(n)$ | Allocate + copy into `Arc<str>` (or `String` before boxing) |
| String across ECS read (air gap) | $O(1)$ | New GC `Object` shell + `Arc::clone` of existing `Arc<str>` (no byte copy) |
| List/Map assignment | $O(1)$ | Pointer copy |
| List `push` (unique list) | $O(1)$ amortized | `Arc::make_mut` + `Vec::push` |
| List `push` (shared list) | $O(n)$ | Clone vector then push |
| Map `insert` | $O(\log n)$ | HAMT structural sharing |
| Arena allocation | $O(1)$ | Bump-pointer style allocation for system temporaries |
| Arena reset | $O(1)$ | Constant-time reset between system runs |
| Backup collector sweep | $O(\text{live closures} + \text{chunk constants})$ | Traces stack, globals, captures, and chunk constants (no ECS world scan) |
