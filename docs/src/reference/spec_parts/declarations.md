## 3. Declarations

### 3.1 Component Declaration

```
component <Name> {
    <field>: <default_value>,
    indexed <field>: <default_value>,
    ...
}
```

Declares a component type. Fields have names and default values. The type of each field is inferred from its default value.

**Indexed fields:** Prefixing a field with the `indexed` keyword creates a runtime index for O(1) entity lookup by that field's value via the `lookup()` builtin (see §6). Only fields with hashable types (`int`, `float`, `str`, `bool`, `entity`) may be indexed; function or compound types are rejected. The index is maintained automatically when components are added, removed, or modified via `set()`. Example:

```
component Username {
    indexed name: "",
}

let hero = spawn(Username { name: "Hero" })
let found = lookup(Username, "name", "Hero")  // Some(hero_entity_id)
```

**Plain-data rule (Law 1):** Component fields cannot have function or closure types (including nested list/map/tuple types that contain them). The checker rejects such fields so ECS storage stays data-only. See [Memory model](memory-model.md).

### 3.1.1 Resource Declaration

```
resource <Name> {
    <field>: <default_value>,
    ...
}
```

Declares a global singleton data type. Resources are structurally identical to components — named fields with default values — but they are **not attached to entities**. A resource is initialized once when the program starts and is accessed via `get_resource(Name)` (returns `Option`) and `set_resource(Name, value)`.

Resources can be injected into systems as parameters: `system Foo(r: mut MyResource) { ... }`. A resource-only system (no component parameters) runs exactly once per schedule invocation. A mixed system iterates entities while injecting the same resource instance on each iteration.

The checker enforces: a resource name cannot collide with a component name (and vice versa), duplicate `resource` declarations are rejected, and `spawn()` / `entities()` cannot accept resource types.

**Plain-data rule:** Like components, resource fields cannot have function or closure types.

### 3.2 Struct Declaration

```
struct <Name> {
    <field>: <default_value>,
    ...
}
```

Declares a plain data record type. Structs are structurally identical to components — they have named fields with default values and support the same field access and spread syntax — but they are **not eligible for ECS operations**. You cannot use a struct with `system` parameters, `entity` declarations, `get()`, `set()`, `has()`, `spawn()`, or `query`.

```
struct Point { x: 0.0, y: 0.0 }
let p = Point { x: 3.0, y: 4.0 }
print(p.x)                         // 3.0

let p2 = Point { x: 10.0, ..p }    // spread syntax
print(p2.y)                        // 4.0 (from p)
```

Use `struct` for general-purpose data records that don't need to participate in the ECS. Use `component` for data that will be attached to entities. The same **plain-data** restriction applies to `struct` fields (structs can nest inside components). Both `struct` and `component` instances share the same flat memory layout (`ComponentData` internally) providing O(1) field access when used as local variables. When components are inserted into the ECS, they are stripped apart into a highly optimized Structure-of-Arrays (SoA) layout; values written to the world are deep-copied into persistent ECS storage (see [Memory model](memory-model.md)).

### 3.3 Entity Declaration

```
entity <name> {
    <Component> { <field>: <value>, ... },
    ...
}
```

Creates a named entity with the specified components. The entity name is bound as a variable containing the entity ID.

#### Entity Literal Expression

```
entity {
    <Component> { <field>: <value>, ... },
    <expr>,
    ...
}

entity <name_expr> {
    <Component> { <field>: <value>, ... },
    <expr>,
    ...
}
```

When `entity` appears in expression position, it is parsed as an **entity literal expression**. It spawns a new entity, attaches the listed components, and evaluates to the entity ID. Because it is an expression, it can appear in let-bindings, function arguments, return values, and anywhere else an `entity`-typed value is expected.

An optional name expression between `entity` and `{` creates a **named** entity (retrievable via `get_entity()`). The name can be any expression that evaluates to a string — a string literal, variable, f-string, or function call. If omitted, the entity is anonymous.

Each entry inside the braces is a **component entry** — either a traditional component initializer (`Component { field: value }`) or an arbitrary expression that evaluates to a component value. The parser uses lookahead to disambiguate: `Ident {`, `Ident.path`, and `Ident::path` are parsed as component initializers; everything else is parsed as an expression. This allows variables, function calls, and other expressions to be used directly as component entries alongside traditional initializers.

```rad
// Anonymous (no name)
let hero = entity {
    Name { value: "Hero" },
    Health { hp: 100, max: 100 },
    Position { x: 0.0, y: 0.0 }
}

// Named with a string literal
let e = entity "player" { Health { hp: 100 }, Position { x: 0, y: 0 } }

// Named with a variable
let path = "assets/level.rad"
let file = entity path { FilePath { path: path }, Unparsed {} }

// Named with an f-string
let npc = entity f"npc_{id}" { Name { value: n } }

set_parent(entity { Child {} }, hero)

// Expression components: variables and function calls
let pos = Position { x: 1.0, y: 2.0 }
let e = entity { Name { value: "Hero" }, pos, make_health(100) }
```

**Disambiguation:** At statement level, `entity Ident` is a declaration (§3.3). In expression position, `entity {` is an anonymous literal; `entity <expr> {` is a named literal. The expression form has type `entity`.

### 3.4 State Machine Declaration

```
state <Name> {
    <StateName> {
        on <event_name> -> <TargetState>
        on <event_name> -> <TargetState> when <guard_expr>
        // optional comma separators are accepted
        // on <event_name> -> <TargetState>, on <event_name> -> <TargetState>
    }
    ...
}
```

Declares a finite state machine. Each state lists its valid transitions. Optional `when` clauses add guard expressions evaluated at transition time. Transition entries may be separated by newlines and/or optional commas.

### 3.5 System Declaration

```
system <Name>(<param>: [mut|accum] <ComponentType>, ...) [after <System> [, <System> ...]] [before <System> [, <System> ...]] {
    <body>
}
```

`after` and `before` clauses are optional and may be repeated. They declare **ordering constraints** relative to other systems:

- `after Physics` — this system must run **after** `Physics` when both appear in the same scheduled run.
- `before Render` — this system must run **before** `Render` when both appear in the same scheduled run.

Example:

```
system Render(p: Position) after Physics {
    ...
}
```

Declares a system that operates on entities and/or global resources. Parameters specify which component types to query and which resource types to inject. The `mut` modifier allows write access; without it, field assignment on that parameter is a compile-time error.

**Component parameters** query the entity world: the system iterates over all entities that have ALL the specified component types. **Resource parameters** inject global singletons declared with `resource`. A system may mix both: `system Tally(u: Unit, s: mut Stats) { ... }` iterates entities with `Unit` while injecting the `Stats` resource on each iteration. A **resource-only** system runs exactly once per schedule invocation.

Resource parameters participate in parallel conflict analysis: two systems that both hold a mutable reference to the same resource are serialized. The checker rejects `update(Resource)` and `set_resource(Resource, ...)` inside a system that already holds the same resource as a `mut` parameter to prevent writeback-overwrite bugs.

**`accum` resource parameters** (`d: accum DamageLog`) declare an **additive reduction**: the parameter is writable like `mut`, but when the system runs in a parallel batch, each worker's per-field **delta** against the batch's base snapshot is *folded into* the base (in schedule order — deterministic, floats included) instead of last-write-wins. Two `accum`-writers of the same resource therefore commute and may share a batch, while a plain reader or `mut`-writer of that resource still serializes against them. The contract is checked statically: `accum` is only valid on **resource** parameters, and every field of the resource must be `int` or `float` (folding is defined per numeric field). The fold is additive — `d.total = d.total + x` per entity aggregates exactly; non-additive updates (min/max/overwrite) belong in an event handler, which is serial by design.

The special variable `self` is bound to the current entity ID (unavailable in resource-only systems).

### 3.5.1 Phase Declaration

```
phase <Name> [<System>, <System>, ...]
serial phase <Name> [<System>, <System>, ...]
```

Declares a named group of systems. Phase names can be used anywhere a system name is accepted in `schedule` blocks:

```
phase Physics [Gravity, Collision, Movement]
phase Rendering [ClearScreen, DrawSprites, DrawUI]

schedule [Physics, Rendering]
```

The phase expands inline into its constituent systems. The checker validates that all listed systems exist and marks them as invoked (suppressing "unused system" warnings). Phases cannot nest other phases.

A **`serial phase`** additionally declares that its members must never share a parallel batch with each other, no matter how disjoint their data access is — "these systems are ordered and I do not want them raced", stated in the program instead of relied on implicitly. Members run in separate batches, in schedule order, in every schedule that includes them; systems outside the group may still run in parallel with them. The whole-schedule spelling is `schedule serial [...]` (§7.2).

### 3.6 Event Declaration

```
event <Name> { <field>, <field>, ... }
event <Name> { <field>: <Type>, ... }
```

Declares an event type with named fields. Field types are optional (unannotated fields are equivalent to `any`). **`pub` events** require an explicit type on every field (same rule as `pub` components / structs). Omit default values: events are instantiated only at `emit` sites.

### 3.7 Event Handler

```
on <EventName>(<param>) {
    <body>
}

on <EventName>(<param>) where <guard_expr> {
    <body>
}

on <EventName>(<param>) when <guard_expr> {
    <body>
}

on <EventName> once (<param>) {
    <body>
}

on <EventName> once (<param>) where <guard_expr> {
    <body>
}

on <EventName> once (<param>) when <guard_expr> {
    <body>
}
```

Registers a handler for an event type. When the event is emitted, all handlers are called in registration order. The parameter is bound to the event data (as a read-only ComponentData).

Multiple handlers can be registered for the same event.

The optional `where` or `when` clause adds a **guard expression**. The handler body only executes when the guard evaluates to truthy. `where` and `when` are interchangeable — use whichever reads more naturally. The guard is desugared to an `if` wrapper at parse time.

```
event Hit { target_id: str, amount: int }

on Hit(e) where e.amount > 10 {
    let target = lookup(Name, "value", e.target_id)?
    print("heavy hit on", target)
}
```

For **`once`** handlers that also have a guard, the guard desugaring is unchanged, but the runtime only marks the handler as **fired** (so later emissions skip it) after an invocation where the guard was truthy and the then-branch ran. If the guard is false, the handler is **not** consumed and remains eligible for future emissions.

The `once` form registers a one-shot handler: it runs at most **once per handler declaration for the lifetime of the program** (see §9). Ordinary handlers (without `once`) run on every emission.

### 3.8 Function Declaration

```
fn <name>(<param>, <param>, ...) {
    <body>
}

pure fn <name>(<param>, <param>, ...) {
    <body>
}
```

Optional type annotations are supported:

```
fn <name>(a: int, b: int) -> int {
    return a + b
}
```

Parameters can be marked as `mut` to allow in-place modification. When a parameter is marked as `mut`, it acts as an in-out reference. The caller must explicitly use the `&` operator to pass a mutable reference to the function. Inside the function body, the parameter is implicitly dereferenced, so you can use it like a normal variable:

```
fn do_something(mut tab: entity) {
    let t = require(tab, Tab)
    set(tab, Tab { count: t.count + 1 })
}

let tab = spawn()
do_something(&tab) // explicit mutation at call site
```

Declares a named function. Functions are first-class values and can be passed to other functions.

A **`pure fn`** declares a function that must not rely on impure effects in contexts that require purity (see §8). Non-`pure` functions are treated as impure for pipeline checking.

A **`readonly fn`** declares a function that may perform ECS read operations (`get`, `has`, `entities`, `query_*`, `with_field`, `peek`, `lookup`) but no world-mutating or I/O side effects. `readonly` functions are allowed inside pipeline expressions alongside `pure` functions (see §8). This lets you use ECS lookups in pipeline stages without having to extract them beforehand.

```
readonly fn get_hp(e: entity) -> int {
    let h = require(e, Health)
    return h.hp
}

let hps = entities(Health) |> map(get_hp)
```

### 3.9 Type (Sum Type) Declaration

```
type <Name> {
    <Variant> { <field>: <default>, ... }
    ...
}
```

Declares a sum type at top level. Variants are separated by whitespace (no comma between variants). Fields use `field: default` syntax — not the `field: Type = default` form used by components and structs (see §2.4 for details). A recursive or self-referential field is written with the bare type name in the default slot (`left: Expr`); see §2.4 for the tree example. Construction, `Option` / `Result`, and `match` are covered in §2.4 and §2.5.

### 3.10 Module Import

```
use "<relative_path>"
use "<relative_path>" as <alias>
use "<relative_path>" as <alias> : <Contract>
```

Imports top-level declarations from another `.rad` file. The path is relative to the directory of the importing file.

**Import Aliasing & Contracts:**

When `as <alias>` is specified, the imported module's `pub` declarations are scoped under the alias and accessed with dot notation (`alias.name`). Non-pub declarations are not accessible from outside; attempting to use them produces a compile-time error. Aliased declarations do **not** enter the flat namespace — they exist only behind their alias prefix.

When `: <Contract>` is added, the compiler verifies that the imported module satisfies the specified structural contract. The contract must be a `struct` type defining the required function signatures and types. If the imported module fails to provide the required `pub` exports with matching types, it is a compile-time error. This implements the **Ports & Adapters** pattern at the module boundary.

```
use "math.rad" as math

print(math.square(5))
let c = math.Color::Red { intensity: 42 }
```

Aliasing prevents name collisions: two modules may define identically named `pub` declarations without conflict, as long as they are imported under different aliases. If `use "path"` is used without `as`, behavior is unchanged — declarations merge into the flat namespace.

**Visibility:**

By default, all top-level declarations (`fn`, `component`, `struct`, `entity`, `state`, `event`, `type`) are **private** to the file they are defined in. To make a declaration accessible from other files, it must be prefixed with the `pub` keyword:

```
pub fn public_helper(x: int) -> int { return x }
pub component PublicComp { x: int = 0 }
```

**Strict Module Boundaries:** Any declaration marked `pub` requires explicit type annotations, regardless of whether `--strict-types` is enabled. For functions, this means parameter and return types must be specified. For components and structs, all fields must have type annotations. The compiler enforces this during the lowering phase and performs a reachability analysis to ensure public APIs do not leak private types. This ensures that the public API of a module is always strictly typed, preventing the ecosystem from fracturing into "typed" and "untyped" code while allowing fast, inferred iteration inside private module bodies.

If a file imports another file but attempts to use a private declaration from it, a compile-time error is raised.

**Resolution rules:**

- Paths are resolved relative to the importing file, then canonicalized.
- The module loader recursively processes `use` statements depth-first.
- Circular imports are safe: the loader tracks visited files and skips already-loaded modules.
- Duplicate top-level symbol names across files are rejected with an error that names both definition sites.

**Namespace:**

Bare `use` imports merge declarations into a single flat namespace where every top-level name must be unique. Aliased imports (`use "path" as name`) keep their declarations separate — accessible only through the alias prefix — so identical names in different aliased modules do not collide.

**Lockfile:**

Running with `--write-lock` produces a `forge.lock` file alongside the entry point. The lockfile records the path, byte size, FNV-1a checksum, and SHA-256 digest of every module in the graph and can be used to verify that dependencies have not changed unexpectedly.

**Source maps:**

When multiple files are loaded, error messages resolve to the original file and line number via an internal source map. Diagnostics always report the file-local position, not the merged offset.

**Scope:**

The current module system is file-based and local. All paths must point to files on disk. There is no remote package registry, no dependency resolution, and no `rad install` yet — these are planned for Q4 2026 (see §12).

**Example — multi-file project:**

```
// math.rad
fn square(x) { return x * x }

// main.rad
use "math.rad"
print(square(5))   // 25
```

---
