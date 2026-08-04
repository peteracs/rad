## Collections

| Function | Description |
|---|---|
| `len(col)` | Length of a list, string, tuple, or map |
| `range(end)` | List of integers `[0, 1, …, end-1]` |
| `range(start, end)` | List of integers `[start, start+1, …, end-1]` |
| `range(start, end, step)` | List of integers from `start` to `end` (exclusive) with `step` (can be negative) |
| `contains(col, val)` | Membership test — works on lists, strings (substring), and maps (key lookup) |
| `keys(map)` | Return list of map keys (sorted deterministically) |
| `values(map)` | Return list of map values (sorted deterministically by key) |
| `entries(map)` | Return list of `[key, value]` pairs (sorted deterministically by key) |
| `merge(map1, map2)` | Return new map where keys in `map2` override keys in `map1` |
| `remove_key(map, key)` | Return a new map with the specified key removed. If the map is uniquely owned, this performs an O(1) in-place deletion. |

## List Operations

| Function | Description |
|---|---|
| `push(list, val)` | Return new list with `val` appended. Statement form: `xs << val` (chains: `xs << a << b`). |
| `set_at(coll, key, val)` | Return new list/map with one element replaced. Lists bounds-check (no silent growth); maps insert-or-replace. The expression dual of `coll[key] = val`, and what `update(e, C) { field[i] = v }` lowers to. |
| `pop(list)` | Return the last element (errors on empty list). Alias for `pop_last`. |
| `pop_last(list)` | Return the last element (errors on empty list) |
| `drop_last(list)` | Return list without the last element (errors on empty list) |
| `drop_first(list)` | Return list without the first element (errors on empty list) — the queue-advance idiom: `queue = drop_first(queue)` |
| `sort(list)` | Return sorted copy (numbers, strings, bools, tuples — tuples compare lexicographically, same order as `sort_by`) |
| `sort_by(list, key_fn)` | Return sorted copy using `key_fn(element)` to extract comparison keys. **Tuple keys compare lexicographically** — multi-key sorting is `sort_by(fn(t) { return (-t.rung, t.dist) })`. Pure, pipeline-friendly. |
| `reverse(list)` | Return reversed copy (also works on strings) |
| `slice(list, start, end)` | Return sub-list from `start` to `end` (exclusive); also works on strings |
| `append(list1, list2)` | Concatenate two lists into a new list |
| `extend(list1, list2)` | Alias for `append` |
| `zip(list1, list2)` | Pair elements from two lists into `[[a₀, b₀], [a₁, b₁], …]` (stops at shorter). Pairs naturally with destructuring: `zip(xs, ys) \|> map(fn([a, b]) { ... })` |
| `enumerate(list)` | Return a list of `[index, element]` pairs: `enumerate(["a","b"])` → `[[0,"a"],[1,"b"]]`. Use with destructuring: `for [i, val] in enumerate(items) { ... }` |

## Functional

| Function | Description |
|---|---|
| `map(list, fn)` | Transform each element, return new list |
| `filter(list, fn)` | Keep elements where `fn` returns truthy, return new list |
| `reduce(list, init, fn)` | Fold list to single value: `fn(accumulator, element)` |
| `flat_map(list, fn)` | Map then flatten — `fn` must return a list per element |
| `group_by(list, fn)` | Group elements by the key returned from `fn(item)` (str, int, bool, entity, tuple — kept as real keys), return map of lists |
| `find(list, fn)` | Return `Some(element)` for the first element where `fn` returns truthy, or `None` |
| `any(list, fn)` | True if `fn` is truthy for at least one element; short-circuits. `any([])` is `false` |
| `all(list, fn)` | True if `fn` is truthy for every element; short-circuits. `all([])` is `true` |
| `max_by(list, fn)` | Return `Some(element)` with the largest key from `fn(element)`, or `None` for empty lists |
| `min_by(list, fn)` | Return `Some(element)` with the smallest key from `fn(element)`, or `None` for empty lists |
| `sum(list)` | Numeric fold: total of all elements. Ints stay int, any float promotes; `sum([])` is `0` |
| `product(list)` | Numeric fold: product of all elements; `product([])` is `1` |
| `get_or(coll, key, default)` | Map lookup or list index with a fallback instead of nil/bounds-error — the cooldown-table read: `cds \|> get_or("q", 0)` |
| `index_of(list, v)` | First index holding `v` (structural equality), or `-1`. An int rather than an Option because the consumer is slot arithmetic: `if at >= 0 { set_at(slots, at, nil) }` |

**Accessor shorthand:** anywhere a one-argument closure is expected, `.field`
projects that field — `mods |> map(.flat) |> sum` instead of
`mods |> map(fn(m) { return m.flat }) |> sum`. Chains reach through nesting:
`units |> map(.stats.hp)`.

**Readonly callbacks:** pipeline callbacks may READ the world — closures and
unannotated functions whose bodies only call readonly builtins
(`get`/`has`/`require`/`name_of`/queries) infer the readonly effect and
compose into `map`/`filter`/`sort_by`/`min_by` without a `readonly fn`
annotation: `units |> sort_by(fn(u) { return (-points_of(u), name_of(u)) })`.

**Filtered loops:** `for m in mods where m.stat == "ad" { ... }` is sugar for
wrapping the body in `if` — same truthy condition rules, reads like the query
it is.

## BitSet

A dynamically-growing bit set for O(1) integer membership testing. Ideal for
bookmarks, line flags, visited-node tracking, or any use case where you need
fast `contains` on integer keys without the overhead of a hash map.

Memory usage is ~1 bit per index: an 80,000-line file's bookmark set uses ~10 KB.

| Function | Description |
|---|---|
| `bitset_new()` | Create a new empty bit set |
| `bitset_set(bs, index)` | Return a new bitset with bit at `index` set (grows automatically) |
| `bitset_has(bs, index)` | Return `true` if bit at `index` is set, `false` otherwise — O(1) |
| `bitset_clear(bs, index)` | Return a new bitset with bit at `index` cleared |

```
let mut bookmarks = bitset_new()
bookmarks = bitset_set(bookmarks, 42)
bookmarks = bitset_set(bookmarks, 1337)

print(bitset_has(bookmarks, 42))    // true
print(bitset_has(bookmarks, 100))   // false

bookmarks = bitset_clear(bookmarks, 42)
print(bitset_has(bookmarks, 42))    // false
```

> **When to use BitSet vs list `contains`:** Use `bitset_has` when checking
> integer membership repeatedly — it is O(1) per lookup. `contains(list, val)`
> performs a linear scan and is O(n). For keyword sets or string membership,
> consider using a `map` with dummy values, which provides O(1) string-key lookup.
>
> **Note on Mutability:** `bitset` uses strict value semantics like `list` and `map`. `bitset_set` and `bitset_clear` are pure functions that return a new bitset. However, the compiler performs static escape analysis: if your bitset is uniquely owned (e.g. a local variable that never escapes), mutations are compiled to $O(1)$ in-place updates.

## String Buffers

Buffers are value-semantic string builders for tight append loops. The compiler
can optimize a non-escaping local reassignment pattern, but the surface model
stays functional: each append returns the next buffer value.

| Function | Description |
|---|---|
| `buffer_new()` | Create an empty string buffer |
| `buffer_append(buffer, str)` | Return a buffer with `str` appended |
| `buffer_to_str(buffer)` | Convert a buffer to a string |

```rad
let mut b = buffer_new()
b = buffer_append(b, "hp=")
b = buffer_append(b, str(42))
print(buffer_to_str(b))    // "hp=42"
```

## Byte Buffers

`bytebuf` is a native byte buffer for binary packet encode/decode. It has value
semantics at the language surface, and the compiler lowers non-escaping local
reassignment patterns to in-place writes.

| Function | Description |
|---|---|
| `bytebuf_new(size)` | Create a zero-filled byte buffer |
| `bytebuf_len(buf)` | Return the byte length |
| `bytebuf_get(buf, index)` | Read one byte as an int `0..255` |
| `bytebuf_set_u8(buf, index, value)` | Return a buffer with one byte written |
| `bytebuf_set_u32_le(buf, offset, value)` | Return a buffer with a little-endian unsigned 32-bit int written |
| `bytebuf_set_i32_le(buf, offset, value)` | Return a buffer with a little-endian signed 32-bit int written |
| `bytebuf_get_u32_le(buf, offset)` | Read a little-endian unsigned 32-bit int |
| `bytebuf_get_i32_le(buf, offset)` | Read a little-endian signed 32-bit int |
| `bytebuf_to_list(buf)` | Convert to `list<int>` for compatibility/tests |
| `bytebuf_from_list(bytes)` | Convert `list<int>` byte values into a byte buffer |

```rad
fn encode_move(client_seq: int, target_x: float, target_y: float) -> any {
    let mut packet = bytebuf_new(15)
    packet = bytebuf_set_u8(packet, 0, 77)
    packet = bytebuf_set_u8(packet, 1, 4)
    packet = bytebuf_set_u8(packet, 2, 2)
    packet = bytebuf_set_u32_le(packet, 3, client_seq)
    packet = bytebuf_set_i32_le(packet, 7, round(target_x * 1000.0))
    packet = bytebuf_set_i32_le(packet, 11, round(target_y * 1000.0))
    return packet
}
```

## ECS

| Function | Description |
|---|---|
| `spawn([name], components...)` | Create a new entity with optional name and components, return its ID |
| `despawn(id)` | Destroy an entity, clear its data, and recycle its ID |
| `get(id, Component)` | Get component — returns `Some(value)` or `None` |
| `require(id, Component)` | Get component and fail fast if missing (returns component directly) |
| `require_all(id, Component...)` | Get multiple required components, fail fast on first missing component |
| `set(id, Component{...})` | Set component on entity (use `..base` spread to avoid retyping unchanged fields) |
| `has(id, Component)` | Check if entity has component |
| `remove(id, Component)` | Remove component from entity |
| `entities([ComponentName...])` | Return all entity IDs, or only entities that have all listed component types |
| `name_of(id)` | Entity's declared name (empty string if unnamed). Readonly. |
| `get_entity(name)` | Lookup by name — returns `entity \| nil`; narrow with a guard (`if e == nil { return }`). Readonly. |
| `require_entity(name)` | Fail-fast lookup by name — returns `entity`, errors if missing (the get/require pairing, extended to names). Readonly. |
| `id_of(id)` | Entity's stable integer id. Pure — usable in `pure fn`. Entities also sort by ascending id: `query { C } \|> sort` is the canonical deterministic order. |
| `query_where(ComponentName..., fn)` | Filter entities having the given components using a predicate evaluated on the entity ID. The predicate may be **pure or read-only** — `get`/`res`/`has`/`readonly fn` calls are allowed (the entity list is snapshotted before the predicate runs), so filtering by component values is direct: `query_where(Hero, fn(id) { return (get(id, Hero) \|> unwrap).level >= 3 })`. Writes, IO, and events in the predicate are compile errors |
| `query_map(ComponentName..., fn)` | Map over entities having the given components using a function evaluated on the entity ID. Same contract as `query_where`: the mapper may be **pure or read-only** (world reads and `readonly fn` calls allowed); writes, IO, and events in the mapper are compile errors |
| `query_count(ComponentName...)` | Return the number of entities having the given components |
| `with_field(entities, ComponentName, FieldName, fn)` | Filter a list of entities by evaluating a predicate function on a specific component field |
| `lookup(ComponentName, field_name, value)` | O(1) indexed lookup: returns `Some(entity_id)` for the **lowest-id** entity whose `indexed` field matches `value`, or `None`. The field must be declared `indexed` in the component. |
| `lookup_all(ComponentName, field_name, value)` | Every entity whose `indexed` field matches `value`, ids ascending — the multi-match query ("all open tickets") as one hash probe instead of an O(world) scan. |
| `get_resource(ResourceType)` | Get global resource — returns `Some(value)` or `None`. Readonly. |
| `res(ResourceType)` | Direct resource access — returns the value itself, no Option. Declared resources auto-initialize from their field defaults, so `res(R)` never misses; the checker types `res(R).field` precisely and rejects components/unknown names. Readonly. |
| `set_resource(ResourceType, value)` | Set global resource value. Mutating. |

### `update` statement

**Component form:** `update(entity, Component) { field = expr, ... }` is syntactic sugar for reading the current component, overriding the listed fields, and writing back. The entity expression is evaluated exactly once. Field types are validated against the component's declaration.

```
component Score { points: 0, level: 1 }
let e = spawn(Score { points: 0, level: 1 })

update(e, Score) {
    points = 50,
    level = 2
}
```

This is equivalent to `set(e, Score { points: 50, level: 2, ..unwrap(get(e, Score)) })`, but shorter and less error-prone.

**Resource form:** `update(ResourceType) { field = expr, ... }` works the same way but for global resources declared with the `resource` keyword. No entity is needed.

```
resource Config { gravity: 9.81, debug: false }

update(Config) {
    debug = true
}
```

The checker rejects `update(entity, Resource)` (resources are not per-entity) and `update(Resource)` inside a system that holds the same resource as a `mut` parameter (the writeback would overwrite the update).

### Indexed lookup semantics

Declare a field `indexed` to maintain a runtime hash index
(`component Ticket { indexed status: "" }`). The index is maintained
through `spawn`/`set`/`update`/`remove`/`despawn`, survives `fork`/`commit`
rewinds, the wire codec (`fork_from_bytes` + `commit`), `save_world`/
`load_world`, `fork_apply`, `merge_forks`, and schema migration — the
program's `indexed` declarations are the source of truth, and `commit()`
reconciles any snapshot that arrived without index data (old saves,
foreign forks). Pinned semantics, chosen for determinism:

- With duplicate keys, `lookup` returns the **lowest entity id** and
  `lookup_all` returns ids **ascending** — both stable across save/load
  round trips and record/replay.
- Float keys are **bit-pattern** keys: `0.0` and `-0.0` are distinct
  buckets, and an int probe never matches a float key. Hashability costs
  exactness; the trade-off is documented rather than hidden.
- `lookup`/`lookup_all` on a field not declared `indexed` is a loud
  runtime error, never a silent scan.

**Effect classification:** The ECS read builtins (`get`, `has`, `entities`, `query_where`, `query_map`, `query_count`, `with_field`, `peek`, `lookup`, `lookup_all`, `get_resource`, `res`) carry the `readonly` effect — they read world state but never mutate it. This means they are **allowed inside pipeline expressions** (`|>`), unlike mutating builtins (`set`, `spawn`, `set_resource`, `load_world`, …) and IO builtins (`print`, `log`, `sleep_ms`, file/network access), which are rejected in pipelines — as direct stages and inside callbacks alike. User-defined functions that only call `readonly` builtins can be declared as `readonly fn` and also used in pipelines.

### Entity literal expressions

When all components are known up front, an **entity literal expression** can replace `spawn()` + multiple `set()` calls:

```
let hero = entity {
    Name { value: "Hero" },
    Health { hp: 100, max: 100 },
    Position { x: 0.0, y: 0.0 }
}
```

An optional **name expression** between `entity` and `{` creates a named entity, replacing the `spawn("name") + set()` pattern:

```
let e = entity "player" { Health { hp: 100 }, Position { x: 0, y: 0 } }
let found = require_entity("player")   // fail-fast lookup (entity)
let maybe = get_entity("player")       // fallible lookup (entity | nil)

// The name can be any string expression:
let file = entity path { FilePath { path: path }, Unparsed {} }
let npc = entity f"npc_{id}" { Name { value: n } }
```

The expression spawns an entity (named or anonymous), attaches every listed component, and evaluates to the entity ID (type `entity`). It works anywhere an expression is expected — let-bindings, function arguments, return values.

Component entries can be traditional initializers (`Component { field: value }`) or arbitrary **expressions** — variables, function calls, or any expression evaluating to a component value:

```
let pos = Position { x: 1.0, y: 2.0 }
let hero = entity { Name { value: "Hero" }, pos, make_health(100) }
```

See the [ECS guide](../guide/ecs.md#entities) and [language spec](spec.md) for details.

Use `spawn` + `set` when you need to add components conditionally or incrementally over time.

### Component spread syntax

When updating a component, use `..base` to copy unchanged fields from an existing value:

```
component Health { hp: 100 }
let hero = spawn(Health { hp: 100 })

let h = get(hero, Health)?
set(hero, Health { hp: h.hp - 10, ..h })
```

Explicit fields override the spread base. The type checker ensures the base is the same component type.

### Default-fill in literals

Fields whose declaration carries a usable default — `field: value` or
`field: Type = value` — may be **omitted from literals**; the constructor
fills them from the declaration:

```rad
component Incident { title: "", priority: 0, status: "open" }

let i = spawn(Incident { title: "disk full" })   // priority: 0, status: "open"
```

Bare type annotations (`x: float`) have no default and stay required. This
is *constructor* semantics: `Incident { title: "t" }` is always a complete
value with defaults filled — to change one field of an **existing** component
use `update(e, Incident) { status = "closed" }` or spread syntax, not `set`
with a partial literal (which would reset the other fields to defaults).

## State Machines

| Function | Description |
|---|---|
| `transition(state, event)` | Attempt transition — returns `Ok(value)` or `Err(message)` |

## Error Handling

**Postfix `?` (try):** After an expression of type `Option<T>` or `Result<T, str>`, `expr?` unwraps the success value or returns early from the **current function** with `None` / `Err`. The function’s return type must allow that propagation (for example `fn main() -> any`, `fn main() -> nil`, or an explicit `Option<...>` / `Result<...>` return type). `fn main() -> nil` is special-cased: `?` propagation exits the program cleanly instead of producing a type error. Prefer `?` over `unwrap` when missing data should propagate rather than panic.

| Function | Description |
|---|---|
| `unwrap(val)` | Extract value from `Some` or `Ok`; runtime error on `None` or `Err` |
| `unwrap_or(val, default)` | Extract value from `Some` or `Ok`; return `default` on `None`/`Err` (no runtime error) |
| `map_or(val, default, fn)` | If `val` is `Some`/`Ok`, return `fn(inner)`; otherwise return `default` |
| `expect(val, msg)` | Same as `unwrap` but uses `msg` in the error message on failure |
| `is_some(val)` | Return `true` if `Some` or `Ok`, `false` otherwise. Pure. |
| `is_none(val)` | Return `true` if `None` or `Err`, `false` otherwise. Pure. |

## Testing

| Function | Description |
|---|---|
| `assert(condition, msg)` | Assert condition is true; runtime error with `msg` on failure |
| `assert_eq(a, b)` | Assert two values are equal; runtime error on mismatch |

## Test Data Generation

| Function | Description |
|---|---|
| `gen_int()` | Generate a deterministic list of test integers (property-style generator) |
| `gen_float()` | Generate a deterministic list of test floats (property-style generator) |
| `gen_str()` | Generate a deterministic list of test strings (property-style generator) |
| `gen_bool()` | Generate a deterministic list of test booleans (property-style generator) |
| `gen_list(list)` | Generate a list of test lists from a seed list |

`gen_*` functions are for deterministic test input generation, **not randomness**.
Use `rand_*` functions when you need pseudo-random values at runtime.
