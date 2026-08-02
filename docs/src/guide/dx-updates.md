# DX Updates (v0.5 Ergonomics)

This page summarizes newer language and tooling behavior introduced around the v0.5 DX track. **Not everything here requires a compatibility flag** — many features (tuples, map helpers, `query` `select`, auto-`main`, etc.) are part of the normal VM. Features that *do* need the opt-in parser/checker mode are summarized in [v0.5 Compatibility Mode](../reference/compat-v05.md).

> **See also:** [Developer Tools](./developer-tools.md) for the full tooling guide — LSP, formatter, lint presets, and snapshot testing.

## CLI flags

On `rad <file.rad>`, the bytecode VM defaults to **`--no-compat-v0.5-dx` semantics** (the `compat_v0_5_dx` flag starts `false` in `main.rs`). Turn on the v0.5 compatibility layer with:

- **`--compat-v0.5-dx`**: Enable v0.5 DX parsing and checker rules (zero-field variant shorthand, `match` rest patterns where gated, etc. — see the compat reference).
- **`--no-compat-v0.5-dx`**: Force the flag off (useful if a wrapper or script enabled it).

Other common flags:

- `--strict-types`: Enable full type-coverage checks. In strict mode, Rad requires explicit annotations for:
  - `let` bindings
  - function parameters and return types
  - component field declarations
  *(Note: `pub` exports always require strict types and are checked for private type leaks, even without this flag)*
- `--write-lock`: Write a `forge.lock` file next to the entry file after module loading. The lock captures module paths, byte counts, checksums, and SHA-256 pins for reproducible runs.
- `--profile-copies`: Enable runtime diagnostics for hidden `Arc` deep clones. When a list mutation triggers `Arc::make_mut` on a shared backing buffer, the VM emits a diagnostic to stderr with the source line and element count. Combine with `let unique` for guaranteed zero-copy mutations. See [Value Semantics](./value-semantics.md).

Example:

```
rad app/main.rad --compat-v0.5-dx --strict-types --write-lock
```

## If-expressions

`if` works in expression position: `if cond { a } else { b }` produces a value. The `else` branch is mandatory (every branch must produce a value), branches hold single expressions, chains continue with `else if`, and both branches must agree on type:

```rad
let aim = if on_footprint { player - click } else { unit - click }

pure fn sign(v: int) -> int {
    return if v > 0 { 1 } else if v < 0 { -1 } else { 0 }
}

// nests anywhere an expression goes
print(max(if hp > 50 { hp } else { 0 }, 10))
let labels = xs |> map(fn(v) { return if v >= 0 { "pos" } else { "neg" } })
```

## Tuple map keys

Tuples of valid key types (`int`, `str`, `bool`, `entity` — nested tuples too) can key maps. Keys hash by value and sort lexicographically, so iteration order stays deterministic. Floats remain banned, inside tuples included. This is the A* score-map shape — coordinates index maps directly:

```rad
let mut cost = {}
cost[(0, 0)] = 0
cost[(4, 2)] = 6
print(get_or(cost, (9, 9), -1))   // -1
let walls = { (1, 1): true, (2, 1): true }
```

Tuple-keyed maps round-trip through snapshots, deltas, and replays like every other value. `sort()` also gained the same lexicographic tuple ordering that `sort_by`/`min_by`/`max_by` already used.

## `rad fmt` matches the written style

The formatter was rebuilt against the dogfood corpus as its style spec: 4-space indents, token-lexed bracket tracking (multi-line closure arguments indent one step and close back exactly; braces inside strings never miscount), trailing-comment alignment preserved verbatim, CRLF and BOM kept. It is idempotent, and `fmt --check` runs clean across the repository — usable as a CI gate.

## Order-independent effect inference

Where a helper sits in the file no longer changes what the checker believes about it: purity/effect inference now runs to a fixpoint over the complete function table, so a function calling helpers declared *later* still infers `pure` or `readonly`. (Previously a forward call degraded the caller to unrestricted, producing spurious "has IO effects" rejections from `simulate()`.) Explicit annotations remain contracts and are never overridden.

## `sign`, `peek_resource`

`sign(n)` completes the `abs`/`clamp` family (-1/0/1, int-preserving, `0`/`NaN` → 0). `peek_resource(fork, R)` is the resource dual of `peek` — read a simulated score or clock straight off a result fork without committing it.

## Honest entity lookups

`get_entity(name)` returns `entity | nil` (it was `any`) — unguarded use where an entity is required is now a compile error, and a nil-guard narrows it back to `entity`. For lookups that must succeed, `require_entity(name)` is the fail-fast dual (the same `get`/`require` pairing components have):

```rad
let me = require_entity("player")          // entity, errors if missing
let p = get_entity(name)                   // entity | nil
if p == nil { return false }
return require(p, Vitals).alive            // p narrowed to entity
```

## Delayed timers travel with snapshots

`emit … after N` timers are program state: `fork()` captures them, `commit()` restores them (and drops the abandoned timeline's), `simulate()` results carry sim-scheduled timers, and the fork wire format ships them (old payloads still decode). A rewound match no longer loses its scheduled respawns.

## Events inside `simulate()`

Systems passed to `simulate()`/`simulate_par()` may emit events: they dispatch inside the fork, flushed once per simulated tick, with full isolation from the live event and delayed-event queues. The checker walks every handler transitively reachable through a system's emits and rejects IO anywhere in the chain (with the offending handler named). Event-driven game logic — damage events, cascades, death handling — now simulates without rewrites. See [ECS — Events inside simulations](./ecs.md#events-inside-simulations).

## For-loop tuple destructuring with parens

Parenthesized for-loop bindings over a **list** destructure tuples of any arity — symmetric with `let (a, b) = t`. Composes with `where`, supports `_` discards; map two-binding iteration and query unpacking keep their existing meanings:

```rad
for (due, who, x, z) in tape where due == t {
    emit Order { who: get_entity(who), to: (x, z) }
}
for (_, name, _) in rows { print(name) }
```

## Transient resources

`transient resource` declares runtime state that is **not part of the world's identity**: `world_digest()` and `save_world()` skip it, while forks and `commit()` still carry its values. For command tapes, derived caches, spatial indexes — anything that describes or accelerates the simulation without being simulation state:

```rad
transient resource Tape { orders: list = [] }

// recording a match no longer changes its digest:
update(Tape) { orders = push(res(Tape).orders, (t, who, x, z)) }
print(world_digest() == before)   // true
```

This is what makes tape-driven replay architecturally bit-exact: record orders into a transient tape during live play, `commit()` back to the t0 fork, re-feed the tape, and `world_digest()` must match at every checkpoint.

## Bucket-fill append: `m[k] << v`

Appending through a map index auto-vivifies a missing key with `[]` — the bucket-fill idiom every spatial grid and adjacency list wants. Chains append in order:

```rad
let mut buckets = {}
buckets[(0, 0)] << eid          // missing key starts as []
buckets[(0, 0)] << other        // present key appends
rows["r"] << 1 << 2 << 3        // [1, 2, 3]
```

## `group_by` with real keys

`group_by`'s key function can return any valid map key — `str`, `int`, `bool`, `entity`, or tuples of those — and the result map is keyed by that type (keys are no longer stringified). Invalid key types (floats, nil) error:

```rad
let by_parity = [1, 2, 3, 4, 5] |> group_by(fn(v) { return v % 2 })
print(by_parity[1])             // [1, 3, 5] — int key, not "1"
let census = units |> group_by(fn(e) { return cell_of(pos(e)) })  // tuple keys
```

## `id_of` and entity ordering

`id_of(e)` returns an entity's stable integer id (pure — usable anywhere). Entities also order by ascending id under the standard total order, so `entities |> sort` is the canonical determinism idiom:

```rad
let canon = query { Body } |> sort      // ascending eid, always
let ids = canon |> map(fn(e) { return id_of(e) })
```

## Tuple±scalar broadcast everywhere

Scalar broadcast now covers all four arithmetic ops with the tuple on the left (`center - reach` inflates a point on every axis), and the commutative ops with the scalar on the left:

```rad
let p = (3.0, 4.0)
print(p - 1.0)      // (2.0, 3.0)
print(2.0 + p)      // (5.0, 6.0)
```

## Matching on Primitives

You can now match directly on primitive literals (strings, integers, floats, and booleans). This eliminates the need for long `if/else-if` chains when dispatching on these values. Note that when matching on primitives, an unconditional wildcard arm (`_ => { ... }`) is **always required** for exhaustiveness.

```rad
let cmd = "start"

match cmd {
    "start" => { print("Starting...") }
    "stop" => { print("Stopping...") }
    _ => { print("Unknown command") }
}

let code = 404

match code {
    200 => { print("OK") }
    404 => { print("Not Found") }
    500 => { print("Server Error") }
    _ => { print("Other Error") }
}
```

## Match guards

Match arms can now include a guard expression:

```
type Event { Alarm { level: 0 } }
let evt = Event::Alarm { level: 3 }

match evt {
    Alarm { level } when level > 2 => { print("high") }
    Alarm { .. } => { print("normal") }
}
```

`if` is also accepted as a guard keyword:

```
type Event2 { Alarm { level: 0 } }
let evt2 = Event2::Alarm { level: 3 }

match evt2 {
    Alarm { level } if level > 2 => { print("high") }
    Alarm { .. } => { print("normal") }
}
```

## Nested destructuring in match patterns

Patterns support nested field extraction:

```
type Meta { Meta { code: "" } }
type Ev { Alarm { meta: Meta::Meta { code: "" }, level: 0 } }
let ev = Ev::Alarm { meta: Meta::Meta { code: "123" }, level: 3 }

match ev {
    Alarm { meta: { code }, level: sev } when sev > 2 => {
        print(code)
    }
    Alarm { .. } => { print("fallback") }
}
```

## Built-in Sum Type Ergonomics

The `Option` and `Result` types now support tuple-like and unit-like syntax for construction and matching, removing the need for verbose braced records:

```
// Old way
let ok_val = Result::Ok { value: 42 }
let none_val = Option::None {}

match ok_val {
    Ok { value } => { print(value) }
    Err { message } => { print(message) }
}

// New way
let ok_val = Ok(42)
let some_val = Some(1) // or just None

match ok_val {
    Ok(v) => { print(v) }
    Err(e) => { print(e) }
}
```

This also applies to `let else` bindings:

```
let Ok(_x) = ok_val else { return }
let Some(_y) = some_val else { return }
```

## Auto-invoked `main()`

If a top-level function named `main` is defined and takes no arguments, the VM will automatically invoke it after executing all top-level statements. This provides a clean entry point for scripts and applications without needing an explicit `main()` call at the bottom of the file.

```
fn main() {
    print("Hello from main!")
}
// No need to call main() here
```

## Tuple Literals

Rad now fully supports tuple literals at runtime. Tuples are fixed-size ordered sequences of typed values.

```
let t = (1, "hello", true)
print(t[0]) // 1
print(len(t)) // 3
print(typeof(t)) // "tuple"
```

Tuples can be unpacked using the spread operator `..`:

```
fn add(a: int, b: int) -> int { return a + b }
let args = (5, 10)
print(add(..args)) // 15
```

You can also destructure tuples (and lists) directly into multiple variables:

```
let (a, b) = (42, "hello")
print(a) // 42
```

## `simulate` and `schedule`: `system::…` references

The second argument to **`simulate(fork, systems, ticks)`** must be a **list literal** whose elements are **`system::Name`** (or qualified **`system::alias::Name`**) references to declared `system`s — for example `simulate(f, [system::Physics, system::AI], 10)`. **`schedule [system::A, …]`** uses the same syntax.

String literals such as `["Physics"]` in that position are **rejected at compile time** with a migration diagnostic; replace each string with the corresponding `system::…` reference so the checker, bytecode, and LSP (hover, go-to-definition) all resolve the same system identity.

See the **World Forking (Speculative Execution)** section in [Language Spec](../reference/spec.md) and the checker classification in `core/vm/src/simulate_syntax.rs` at the repository root.

## Query Projection (`select`)

The `query` expression now supports a `select` clause to project specific components from the matched entities, returning a list of components or tuples instead of just entity IDs.

```
component Health { hp: 100 }
component Position { x: 0.0, y: 0.0 }

// Returns a list of Health components
let _healths = query { Health, Position } select Health

// Returns a list of tuples containing (Health, Position)
let _pairs = query { Health, Position } select Health, Position where Health.hp > 0
```

## `main() -> nil` tolerates `?`

The `?` operator is now allowed inside `fn main() -> nil`. When `?` propagates `None` or `Err`, the program exits cleanly without a type error. This removes the common friction of choosing between `-> nil` and `-> any` just to use `?`:

```
fn main() -> nil {
    let data = read_file("config.txt")?
    print(data)
}
```

## List destructuring in closures and for-loops

Closure parameters and for-loop bindings now support bracket destructuring, eliminating magic-number indexing in pipelines:

```
// Before: positional indexing
let names = rows |> filter(fn(r) { return r[1] == 2 })
                 |> map(fn(r) { return r[0] })

// After: named destructuring
let names = rows |> filter(fn([name, phase]) { return phase == 2 })
                 |> map(fn([name, phase]) { return name })
```

For-loops support the same syntax:

```
for [a, b] in [[1, 2], [3, 4]] {
    print(a + b)
}
```

Multiple parameters can be destructured (`fn([a, b], [c, d])`), underscore `_` discards positions (`fn([_, mid, _])`), and `mut` makes bindings mutable (`fn(mut [a, b])`). The checker infers element types from pipeline context and validates arity for tuples.

## `enumerate`, `find`, `max_by`, `min_by` builtins

Four new collection builtins, all pipeline-friendly:

```
["a", "b", "c"] |> enumerate |> map(fn([idx, val]) { return f"{idx}: {val}" })
// ["0: a", "1: b", "2: c"]

let first_even = [1, 3, 4, 7] |> find(fn(x) { return x % 2 == 0 })
// Some(4)

let longest = ["hi", "hello", "greetings"] |> max_by(fn(s) { return len(s) })
// Some("greetings")

let shortest = ["hi", "hello", "greetings"] |> min_by(fn(s) { return len(s) })
// Some("hi")
```

## Named system phases

Group related systems into named phases for cleaner scheduling:

```
phase Physics [Gravity, Collision, Movement]
phase Rendering [ClearScreen, DrawSprites, DrawUI]

schedule [Physics, Rendering]
```

Phase names expand inline in `schedule` blocks. The checker validates that all listed systems exist.

## `update` syntax sugar

A shorter way to patch individual fields on a component:

```
update(hero, Health) {
    hp = 50
}
```

This reads the current `Health`, overrides `hp`, and writes it back. The entity expression is evaluated exactly once. Field types are validated at compile time.

The same form works for **resources** (no entity needed):

```
resource GameConfig { gravity: 9.81, debug: false }

update(GameConfig) {
    debug = true
}
```

## Guarded event handlers

Event handlers now support `where` or `when` guard clauses:

```
on Hit(e) where e.amount > 10 {
    print("heavy hit!")
}
```

The guard is desugared to an `if` wrapper at parse time. For handlers that combine **`once`** with a guard, the VM defers marking the handler as fired until the guard is truthy and the body runs (a false guard leaves the `once` handler eligible for a later emission). Ordinary guarded handlers do not need this extra step.

## New map built-ins

- `entries(map)`: Returns a list of `[key, value]` pairs.
- `merge(left, right)`: Returns a new map where keys in `right` override keys in `left`.
- `group_by(list, fn)`: Groups list items by the string key returned from `fn(item)`.

Example:

```
fn parity(n: int) -> str {
    if n % 2 == 0 { return "even" }
    return "odd"
}

let merged = merge({"x": 1}, {"x": 3, "y": 2})
print(merged["x"])            // 3
print(len(entries(merged)))   // 2

let grouped = group_by([1, 2, 3, 4], parity)
print(grouped["even"])        // [2, 4]
print(grouped["odd"])         // [1, 3]
```
