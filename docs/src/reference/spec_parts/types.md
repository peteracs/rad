## 2. Type System

### 2.1 Primitive Types

| Type | Description | Default |
|---|---|---|
| `int` | Signed integer (`i64` range) | `0` |
| `float` | 64-bit floating point | `0.0` |
| `str` | UTF-8 string | `""` |
| `bool` | Boolean | `false` |
| `nil` | Null/unit | `nil` |

The VM stores most `int` values inline in the NaN-boxed `Value` word; values outside the fast inline range use a heap `BigInt`. Heap strings are stored as `Arc<str>` inside `Object::Str`, enabling O(1) sharing across the air gap between persistent ECS storage and the execution stack. See `core/vm/src/value.rs`.

### 2.2 Compound Types

| Type | Description |
|---|---|
| `list` | Ordered sequence of values |
| `tuple` | Fixed-size ordered sequence of typed values |
| `map` | Key-value store (persistent HAMT) |
| `bitset` | O(1) integer membership set |
| `component` | Named record with typed fields (ECS-eligible) |
| `struct` | Named record with typed fields (plain data, not ECS-eligible) |
| `state` | State machine instance |
| `system` | Reference type for a declared `system`; only created via `system::Name` expressions (see § World Forking) |
| `type` (sum type) | Tagged union of variants with optional fields |
| `fn` | Function (named or anonymous). Type annotations must specify parameters: `fn(int) -> str` or `fn()` |

### 2.3 Type Inference and Gradual Typing

Rad uses a **gradual typing** model. Type annotations are optional — unannotated bindings and parameters are inferred where possible, or treated as `any` (the top type).

#### 2.3.1 Inference Rules

Component field types are inferred from their default values:

```
component Health { hp: 100, max: 100 }
// hp: int, max: int (inferred from 100)
```

Variable bindings infer their type from the initializer expression:

```
let x = 42          // x: int (inferred)
let y: str = "hi"   // y: str (declared, validated against inferred)
let pair: (int, str) = (1, "a") // Tuple type annotation
```

Function parameter types are optional. Unannotated parameters are `any` (with a warning in non-strict mode):

```
fn add(a: int, b: int) -> int { return a + b }
fn identity(x) { return x }   // x: any, return: any
let callback: fn(int) -> str = fn(x) { to_str(x) }
```

Generic functions are supported:

```
fn identity<T>(x: T) -> T { return x }
fn pair<A, B>(a: A, b: B) -> list<any> { return [a, b] }
```

Type aliases are supported:

```
type UserId = int
type Boxed<T> = list<T>
```

#### 2.3.2 The `any` Type

`any` is the top type. It is compatible with all other types:

- A value of type `any` can be passed where any type is expected.
- A value of any type can be passed where `any` is expected.
- Operations on `any` values are checked at runtime, not compile time.
- `any` arises from: unannotated function parameters, empty list/map literals, builtins with polymorphic returns.

#### 2.3.3 Subtyping Rules

| From | To | Rule |
|---|---|---|
| `int` | `float` | Implicit numeric promotion |
| `any` | `T` | Always allowed (gradual) |
| `T` | `any` | Always allowed (gradual) |
| `list<T>` | `list<U>` | Covariant: allowed if `T` assignable to `U` |
| `map<V>` | `map<W>` | Covariant: allowed if `V` assignable to `W` |

All other type pairs are incompatible. Cross-type operations (e.g., `int + str`) produce a compile-time error.

#### 2.3.4 Division Semantics

The `/` operator performs **truncating integer division** when both operands are `int`: the result is always `int`, rounded toward zero. When either operand is `float`, the result is `float`. Division or modulo by zero with constant operands produces a compile-time error.

| Expression | Result Type | Example |
|---|---|---|
| `10 / 2` | `int` | `5` |
| `10 / 3` | `int` | `3` (truncated) |
| `-7 / 2` | `int` | `-3` (toward zero) |
| `10.0 / 3` | `float` | `3.3333…` |
| `10 / 3.0` | `float` | `3.3333…` |

The type checker infers `int / int` as `int`, consistent with runtime behavior.

The builtin `int_div(a, b)` provides the same truncating semantics as `/` for int operands, but as a named function — useful in pipelines and `map` calls where an operator cannot be used directly:

```
int_div(7, 2)    // 3
int_div(-7, 2)   // -3
[10, 20, 30] |> map(fn(x) { return int_div(x, 3) })  // [3, 6, 10]
```

#### 2.3.5 String Multiplication (Repeat)

`*` supports string repetition with an integer count:

| Expression | Result |
|---|---|
| `"=" * 5` | `"====="` |
| `3 * "ab"` | `"ababab"` |
| `"x" * 0` | `""` |
| `"x" * -2` | `""` |

This is useful for separators, progress bars, and text UI layout.

`int_div` is marked `pure` and can be used inside pipeline expressions.

#### 2.3.6 Annotation Validation

When a type annotation is present on a `let` binding or function parameter, the checker validates:

1. The annotation resolves to a known type (primitive, component, state machine, or sum type).
2. The inferred type of the initializer is assignable to the declared type.
3. If both types are concrete (non-`any`) and incompatible, a compile-time error is raised.

```
let x: int = 42       // OK: int assignable to int
let y: float = 10     // OK: int promotable to float
let z: int = "hello"  // Error: str not assignable to int
```

Mixed-type list literals infer to `list<any>` and emit a warning by default.
If a heterogeneous list is intentional, annotate the binding as `list<any>` to silence that warning:

```rad
let payload: list<any> = ["/users", 200, true]
```

#### 2.3.7 Logical Operators

The logical operators `and` and `or` require both operands to be of type `bool`. The type checker enforces this at compile time.

```
let a = true and false   // OK
let b = true and 42      // Error: Right operand of And must be bool, got int
```

#### 2.3.8 Runtime Type Checking

The runtime checks type consistency on component field assignment:

```
set(entity, Health { hp: "banana", max: 100 })
// Runtime error: Type error in 'Health.hp': expected int, got str
```

`int` is implicitly promotable to `float`:
```
component Position { x: 0.0, y: 0.0 }
set(entity, Position { x: 5, y: 10 })  // OK: int promoted to float
```

#### 2.3.9 Purity as a Type-Level Property

Functions declared with `pure` or used in pipeline stages are subject to additional constraints (see §6). The type checker verifies that pure functions do not:

- Mutate variables from enclosing scopes
- Call impure functions (emit, print, etc.)
- Access mutable global state

#### 2.3.10 Dead Code Detection

The compiler performs a reachability analysis pass to detect unused declarations. It traces execution starting from `main`, system bodies, tests, top-level statements, handlers for `pub` events, handlers reached by `emit`, module aliases, type annotations, and component/entity literals. It emits warnings for any of the following private declarations that are never referenced:

- Unused `fn` declarations
- Unused `component` declarations
- Unused `event` declarations
- Unused `struct` declarations

Additionally, the compiler tracks local variable usage and emits a warning if a `let` binding or function parameter is never read. Prefixing the name with `_` (e.g., `_unused`) silences this warning.

### 2.4 Sum Types

A sum type (tagged union) is declared with the `type` keyword. Each variant has a name and an optional set of fields with default values. The default value doubles as the type hint for inference:

```
type <Name> {
    <Variant1> { <field>: <default>, ... }
    <Variant2> { }
    ...
}
```

**Important:** Variant fields use `field: default` syntax (e.g. `radius: 0.0`), **not** the `field: Type = default` syntax used by `component`, `struct`, and `resource` declarations. Writing `radius: float = 0.0` inside a variant is a parse error — the parser emits a targeted diagnostic explaining the difference and naming the type-only spelling below.

**Recursive and self-referential fields.** A field whose type has no natural default value — the common case for tree-shaped data (ASTs, JSON, linked lists) — is declared with just the **type name** in the default slot: `left: Expr`. There is no value of type `Expr` to write as a default while `Expr` is still being declared, so the bare type name stands in and fixes the field's type. This is the canonical spelling for a recursive field:

```
type Expr {
    Num { value: 0 }
    Add { left: Expr, right: Expr }
    Mul { left: Expr, right: Expr }
}
```

`Expr::Add { left: Expr::Num { value: 2 }, right: Expr::Num { value: 40 } }` builds a real nested tree, and a `match` over the variants recurses into `left`/`right`. (The two spellings that do *not* work: `left: nil` types the field as `nil`, and `left: Expr = nil` is the `component`/`struct` form that the diagnostic above rejects.)

**Construction:** use `TypeName::VariantName { <field>: <expr>, ... }`. Fields omitted from the literal use the variant’s default values where defined. A variant with no fields uses `VariantName { }`.

**Experimental v0.5 DX compat mode:** when the runtime is started with `--compat-v0.5-dx`, zero-field variant shorthand `TypeName::VariantName` is accepted and treated as equivalent to `TypeName::VariantName { }`.

**Disambiguation in compat mode:** `Type::Variant` is resolved as a sum variant when `Type` names a sum type and `Variant` names one of its variants. In principle, if both a state machine and sum type shared the same name, compat diagnostics (`W2501`) would be emitted to encourage explicit syntax. In practice, RAD's flat top-level namespace prevents this overlap — `type X` and `state X` cannot coexist.

**Matching:** `match` works on sum type values as well as state machine instances. For a sum type, the match must be **exhaustive** — every variant of that type must appear as an arm. Each arm may **destructure** fields by listing their names in braces:

```
match x {
    Circle { radius } => { ... }
    Point { } => { ... }
}
```

For the built-in `Option` and `Result` types, you can use tuple-like and unit-like shorthand syntax:

```
match x {
    Some(value) => { ... }
    None => { ... }
}
```

An arm with no bindings uses `VariantName { } => { ... }`. If you only care about the variant tag, you can use a bare variant match: `VariantName => { ... }`. However, if you open braces to destructure a variant, you **must** bind all fields exhaustively (e.g., `{ field1, field2 }`) or use the rest operator (`..`) to explicitly ignore them.

**Experimental v0.5 DX compat mode:** when started with `--compat-v0.5-dx`, `match` supports rest-binding syntax in sum-type arms to ignore remaining fields: `Variant { field1, .. } => { ... }`. The `..` marker can appear at most once and must be the final entry. To ignore all fields, use `Variant { .. } => { ... }`.

### 2.5 Built-in Sum Types (`Option`, `Result`)

The language provides two predefined sum types:

- **`Option`** — variants `Some(value)` and `None`. Used when a value may be absent.
- **`Result`** — variants `Ok(value)` and `Err(message)`. Used for success or failure with a string message.

`get()` returns `Some(component)` when the entity has the component, and `None` when it does not (see §6).

`transition()` returns `Ok(new_state_instance)` on a successful transition, or `Err(message)` when the transition is invalid, guarded out, or missing (see §6).

**Try operator (`postfix ?`):** After an expression of type `Option<T>` or `Result<T, str>`, postfix `?` unwraps the success value (`Some` / `Ok`) or **propagates** failure: `None` or `Err` becomes the result of the **enclosing function** (the function exits early with that value). The enclosing function’s return type must be compatible with both the success path and the propagated `Option` / `Result` (see the type checker). Inside `nil`-returning `fn` bodies, propagation uses the same mechanism as for optional return types.

```rad
fn load_health(e: entity) -> Option<Health> {
    return Some(get(e, Health)?)
}

fn tick_door(d: Door) -> Result<Door, str> {
    let d2 = transition(d, "unlock")?
    return Ok(d2)
}
```

**`unwrap` and `expect`:** builtins for extracting the success payload when you intentionally **panic** on failure. `unwrap(x)` returns the inner `value` for `Some` or `Ok`, and errors on `None` or `Err`. `expect(x, "message")` does the same but uses the given string (or a default) in the error text on failure. Prefer `?` or `match` in production code; use `unwrap` only when failure is a bug (e.g. tests) or after a prior `has()` check.

Common patterns:

```
get(entity, "Health")?
match get(entity, "Health") {
    Some(value) => { ... }
    None => { ... }
}
```

---
