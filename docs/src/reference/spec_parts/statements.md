## 4. Statements

### 4.1 Variable Binding

```
let <name> = <expr>
let mut <name> = <expr>
let unique <name> = <expr>
let rec <name> = <expr>
let <name>: <Type> = <expr>
let mut <name>: <Type> = <expr>
let (<name1>, <name2>, ...) = <expr>
let mut (<name1>, <name2>, ...) = <expr>
```

`let` creates an immutable binding. The variable cannot be reassigned.
`let mut` creates a mutable binding. The variable can be reassigned.
`let unique` creates a single-ownership binding. The compiler statically ensures the value is never aliased: it cannot be assigned to another variable (`let y = x`), passed as a function argument, or captured by a closure. Reassignment to the same name (`x = transform(x)`) is allowed. This guarantees that mutations on the binding are always in-place (no hidden O(n) deep clones from `Arc::make_mut`).
`let rec` creates a recursive binding. It is only valid for a single binding name, not tuple destructuring.
Type annotations are optional and constrain the initializer type during type-checking.

**Tuple Destructuring:** You can destructure a list or tuple directly into multiple variables using parentheses:

```
fn test_tuple() -> list<int> {
    return [42, 100]
}

let (a, b) = test_tuple()
print(a) // 42
print(b) // 100
```

*Note on Mutability:* When using `let mut (a, b) = ...`, the `mut` keyword applies to *all* bindings in the tuple. Granular mutability like `let (mut a, b) = ...` is not currently supported.

**Bracket Destructuring in Closures and For-Loops:** In addition to `let (a, b) = ...`, closure parameters and for-loop bindings support bracket destructuring syntax: `fn([a, b]) { ... }` and `for [a, b] in rows { ... }`. This is particularly useful in pipelines where `query ... select A, B` or `zip`/`enumerate` return lists of tuples or lists:

```
let rows = [("alice", 30), ("bob", 25)]
let names = rows |> filter(fn([name, age]) { return age > 26 })
                 |> map(fn([name, age]) { return name })

for [idx, val] in enumerate(items) {
    print(f"{idx}: {val}")
}
```

**Variable Shadowing:** Declaring a new variable with the same name as an existing variable in scope is allowed. However, if the new variable has the exact same type as the shadowed variable, the compiler will emit a warning to prevent accidental bugs (e.g., `let i = 0` inside an outer loop). Shadowing with a different type (e.g., unwrapping an `Option<T>` to a `T`) is considered an intentional type-state pattern and does not warn. Prefixing the variable name with `_` also silences the warning.

#### Optional binding (`let Some ... else` / `let Ok ... else`)

For `Option` and `Result` values, a shorthand binds the payload of `Some` or `Ok` and supplies a block for the `None` or `Err` case:

```
let Some(name) = <expr> else { <block> }
let Ok(name) = <expr> else { <block> }
let mut Some(name): <Type> = <expr> else { <block> }
```

Restrictions:

- Only `Some` and `Ok` patterns are allowed (not arbitrary sum-type variants).
- The pattern must introduce **exactly one** binding (for example `Some(x)`).
- The subject expression must have type `Option` or `Result` (or `any` in non-strict checking).
- The `else` block must either diverge (`return`, `break`, `continue`) or evaluate to a value that is compatible with the binding type.

The construct is compiled to a `match` expression with two arms (`Some`/`Ok` vs `None`/`Err`); the `else` block runs when the value is `None` or `Err`. The match expression’s value becomes the binding (the `else` arm should end with an expression if you need a concrete value there, matching ordinary `match` expression semantics).

### 4.2 Assignment

```
<target> = <expr>
```

Assigns a value to a mutable target. Valid targets:
- Mutable variable: `x = 5`
- Component field (when mutable): `pos.x = 1.0`
- List index: `list[0] = "hello"`

Assignment to an immutable binding is a compile-time error.

#### 4.2.1 Value Semantics

Rad enforces **value semantics** for all compound types (lists, maps, components, bitsets, buffers). This means bindings hold independent copies of data, not references.

**Copy-on-bind:** When a compound value is assigned to a new binding, a deep copy is made:

```
let mut a = [1, 2, 3]
let mut b = a       // b is an independent copy
b[0] = 99
print(a[0])         // 1  (a is unchanged)
```

**Copy-on-call:** Function arguments receive independent copies. Mutations inside a function do not affect the caller:

```
fn mutate(xs) {
    xs[0] = 999
}
let mut data = [1, 2, 3]
mutate(data)
print(data[0])      // 1  (data is unchanged)
```

**Nested write-back:** Compound assignment to nested containers writes back through the full access chain. The compiler handles this automatically:

```
let mut xs = [[1, 2], [3, 4]]
xs[0][1] = 99       // modifies xs in place
print(xs[0][1])     // 99
```

**Implementation note:** The Rust VM stores **lists** as `Arc<Vec<Value>>` (copy-on-write: unique bindings mutate in place; shared lists clone the vector on write). **Maps** use persistent `im::HashMap` (HAMT) for structural sharing. When a map value is uniquely owned, updates can reuse storage; when shared, structural sharing keeps copies $O(\log N)$ in the tree depth rather than always deep-copying the whole map. **Strings** use `Arc<str>` internally; crossing the air gap between persistent ECS storage and the execution stack (via `get`/`peek`) is O(1) per string field.

**Representation:** Runtime `Value` is NaN-boxed into a single `u64` (IEEE-754 quiet NaN space encodes non-float payloads; see `core/vm/src/value.rs`). 48-bit integers are stored unboxed directly inside the NaN payload. Heap objects (strings, lists, closures) are pointed to by tagged NaN pointers; persistent ECS objects carry a `PERSISTENT_PTR_TAG` bit to distinguish them from GC-managed objects.

**Closure exception:** Variables captured by closures via `let mut` are shared between the closure and the enclosing scope using a mutable cell. Reassignment inside the closure updates the outer binding:

```
let mut count = 0
let inc = fn() { count = count + 1 }
inc()
print(count)        // 1
```

This is the only case where two bindings can observe each other's mutations.

#### 4.2.2 Indexing Rules

- List/string indices must be non-negative integers.
- Indexing into a string `s[i]` returns an integer representing the byte value at that index (e.g. `97` for `"a"`), not a 1-character string.
- Negative indices are runtime errors (`Negative index`).
- Out-of-bounds list/string access is a runtime error (`List index N out of bounds`).
- Missing map keys evaluate to `nil`.

### 4.3 If Statement

```
if <condition> {
    <body>
} else if <condition> {
    <body>
} else {
    <body>
}
```

The type checker evaluates `<condition>` to ensure it is a boolean. If the condition evaluates to a constant boolean value (e.g., `true`, `false`, `!false`), the compiler emits a warning.

**Best Practice:** Deeply nested `else if` chains are considered an anti-pattern. Use **Guard Clauses** (early returns) for simple linear control flow, or **Pattern Matching** (`match`) for complex state evaluation and value assignment.

### 4.4 While Loop

```
while <condition> {
    <body>
}
```

`break` exits the innermost loop. Like `if` statements, constant boolean conditions emit a warning.

### 4.5 For Loop

```
for <var> in <iterable> {
    <body>
}

for <key>, <value> in <map> {
    <body>
}

for (<id>, <comp1>, <comp2>) in query { <Comp1>, <Comp2> } {
    <body>
}

for [<a>, <b>] in <iterable> {
    <body>
}

for <var> in <iterable> where <cond> {
    <body>
}
```

Iterates over a list, string, map, or ECS query. For lists, it binds the element to `<var>`. For strings, it binds the integer byte value of each character to `<var>`. For maps, a single variable binds the key, while two variables bind the key and value. For queries, it binds the entity ID and its components (parentheses around the bindings are optional but recommended for multiple bindings). The loop variables are mutable within the loop body. `break` exits the loop.

**List destructuring:** When the iterable yields lists or tuples, bracket syntax `for [a, b] in rows` unpacks each element positionally into named bindings. The bindings are immutable. Underscore `_` may appear multiple times as a discard. The checker validates that the element type is a list, tuple, or `any`; for tuples, the binding count must match the tuple arity. Destructuring cannot be combined with two-variable map iteration (`for [k, v] in map` is a type error — use `for k, v in map` instead).

**Filtered loops:** `for x in xs where cond { ... }` is parser sugar for wrapping the body in `if cond { ... }`. It is useful with query expressions, for example `for (id) in query { Scene } where Scene.name == "level_1" { ... }`.

### 4.6 Return

```
return <expr>
return
```

Returns a value from a function. `return` without an expression returns `nil`.

If a function declares a return type other than `any` or `nil`, the compiler verifies that all control flow paths return a value. Falling off the end of a branch (like an `if` without an `else`) implicitly returns `nil`, which will trigger a type error if the function is expected to return a specific type.

*Note:* Any statements following a diverging statement (`return`, `break`, or `continue`) in the same block are considered unreachable and will result in a compile-time error.

### 4.7 Emit

```
emit <EventName> { <field>: <expr>, ... }
emit <EventName> { <field>: <expr>, ... } after <ticks>
```

Emits an event. All registered handlers for the event are called.

**Event queuing:** Rad uses a strict double-buffered event architecture. Emitting an event pushes it to the next frame's queue. Events are only dispatched when the current frame ends (via `schedule`) or when `flush_events()` is explicitly called. This prevents stack overflow from circular events.

`emit ... after N` queues a delayed event that fires after `N` event-flush cycles. Delayed timers are part of program state: forks, simulation, commit, snapshot, and replay preserve them.

Delayed emits are not supported while a system is running inside a parallel
system batch. Use an immediate `emit`, emit the delayed event from a handler, or
place that system in a single-system schedule when it must arm a timer.

### 4.8 System Execution

```
<SystemName>()
schedule [ <SystemName>, <SystemName>, ... ]
```

Each target may be **`Alias.Sys`** (module alias and system name) or **`system::path::ToSys`** (same path rules as `system::…` expressions).

`<SystemName>()` executes a single system across all matching entities.

`schedule [ A, B, C ]` runs several systems in an order that respects all `after` / `before` constraints declared on those systems. The implementation **topologically sorts** the listed systems; if constraints contain a **cycle**, it is an **error**. Named phases (see §3.5.1) expand inline. After ordering, the native VM partitions conflict-free systems into parallel worker batches and merges writes plus emitted events deterministically; wasm runs the same isolated worker path sequentially.

### 4.9 Update Statement

**Component form** (requires an entity expression):

```
update(<entity_expr>, <ComponentName>) {
    <field> = <expr>,
    <list_or_map_field>[<index_expr>] = <expr>,
    ...
}
```

Syntactic sugar for reading a component, overriding specific fields, and writing it back. Equivalent to:

```
let __tmp = <entity_expr>
set(__tmp, ComponentName { field: expr, ..unwrap(get(__tmp, ComponentName)) })
```

The entity expression is evaluated exactly once. The checker validates that the component and all field names exist, and that each assigned value matches the field's declared type.

An update block may patch one level inside a list or map field with bracket syntax:

```rad
update(hero, Loadout) {
    shields[1] = 250,
    items["sword"] = 1
}
```

Nested indexed updates such as `rows[1][0] = v` are rejected; read the field first, compute the nested value, and assign the whole element.

**Resource form** (no entity — resources are global singletons):

```
update(<ResourceName>) {
    <field> = <expr>,
    ...
}
```

Desugars to a `get_resource` / `set_resource` round-trip. The checker rejects `update(entity, Resource)` (resources are not attached to entities) and `update(Resource)` inside a system that already holds the same resource as a `mut` parameter (the writeback would overwrite the update).

### 4.10 Match

```
match <expr> {
    <Name> => { <body> }
    <Name> { <field>, <field>, ... } => { <body> }
    has <ComponentName>(<binding>) => { <body> }
    <Literal> => { <body> }
    _ => { <body> }
}
```

`match` works as both a statement and an expression. In expression position (for example
`let x = match v { ... }`), each arm returns the value of the final expression in its
block; if an arm block has no trailing expression, that arm's value is `nil`.

**State machines:** when the subject is a state machine instance, each arm is a state name. The match must be **exhaustive** — every state in that machine must have an arm. Optional `{ }` destructuring is not used for plain state arms.

**Sum types:** when the subject is a sum type value, each arm is a **variant name**. The match must list **every variant** of that sum type. Use `Variant { field1, field2 } => { ... }` to bind fields; use `Variant { } => { ... }` for variants with no fields.

**Primitives (Strings, Integers, Floats, Booleans):** when the subject is a primitive type, arms can be exact literal values (e.g., `"hello" => { ... }`, `42 => { ... }`, `true => { ... }`). A wildcard arm (`_ => { ... }`) is **always required** since it's impossible to exhaustively match all possible values of an open set.

**Entity component patterns:** `has Component` matches an entity that carries `Component`. `has Component(c)` also binds the component value for that arm.

```rad
match target {
    has Health(h) => print(h.hp)
    _ => print("no health")
}
```

Arms may use a bare expression after `=>`; the parser wraps it as the arm body. This is especially useful in match expressions:

```rad
let label = match state {
    Open => "open"
    Closed => "closed"
}
```

**Experimental v0.5 DX compat mode:** `Variant { field1, .. } => { ... }` is supported for sum types and means "bind listed fields and ignore remaining fields".

Missing arms or variants for exhaustive types is an error.

---
