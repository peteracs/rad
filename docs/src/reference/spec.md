# Rad Language Specification

## Version: 0.4

---

This document describes stable v0.4 behavior plus an optional **v0.5 DX compatibility mode** enabled by CLI flags.

---

## 1. Lexical Structure

### 1.1 Keywords

```
component  resource  struct  entity  state  system  event  on  emit  fn  let  mut
if  else  while  for  in  return  break  continue  true  false  nil  schedule
and  or  not  match  when  where  use  as  type  pure  once  indexed  unique
async  await  rec  pub  readonly  phase  update
```

### 1.2 Operators

| Operator | Precedence | Associativity | Description |
|---|---|---|---|
| `\|>` | 1 (lowest) | Left | Pipeline |
| `or` | 2 | Left | Logical OR |
| `and` | 3 | Left | Logical AND |
| `==` `!=` `is` | 4 | Left | Equality / Variant Check |
| `<` `>` `<=` `>=` | 5 | Left | Comparison |
| `|` | 6 | Left | Bitwise OR |
| `^` | 7 | Left | Bitwise XOR |
| `&` | 8 | Left | Bitwise AND |
| `<<` `>>` | 9 | Left | Shifts |
| `+` `-` | 10 | Left | Addition |
| `*` `/` `%` | 11 | Left | Multiplication (`str * int` repeats strings) |
| `-` `not` `!` `~` `await` | 12 | Right (prefix) | Unary |
| `.` `()` `[]` `?` | 13 (highest) | Left (postfix) | Access, call, index, try |

In **v0.5 DX compatibility mode**, `..` is recognized in `match` binding lists as a rest-binding marker.

Note: some declaration keywords are accepted as contextual identifiers in identifier positions where parsing is unambiguous (for example, `entity`, `before`, `after`, and `update` can be used as variable names). `update` is a contextual keyword: `update(...)` starts an update statement only when followed by `(`; otherwise it parses as an identifier. The experimental Causal Laws words `intent`, `law`, `resolver`, `constraint`, `watches`, `settle`, `propose`, `next`, `require`, `else`, and `key` are also contextual rather than reserved.

### 1.3 Literals

- **Integer:** `0`, `42`, `-7`
- **Float:** `3.14`, `0.5`, `-2.0`
- **String:** `"hello"`, `"line\nbreak"`, `"tab\there"`
- **Multi-line String:** `\\line 1\n\\line 2` (Zig-style line-based literals)
- **Boolean:** `true`, `false`
- **Nil:** `nil`
- **List:** `[1, 2, 3]`, `[]`

### 1.4 Comments

```
// Single-line comment (to end of line)
```

### 1.5 Identifiers

Identifiers start with a letter or underscore, followed by letters, digits, or underscores. Keywords cannot be used as identifiers.

### 1.6 Formal Grammar (EBNF)

The following Extended Backus-Naur Form grammar defines the complete syntax of Rad. Terminals are in `"quotes"`, `{ X }` means zero-or-more, `[ X ]` means optional, and `|` separates alternatives.

```ebnf
program        = { declaration } ;

declaration    = [ "pub" ] ( component_decl | resource_decl | struct_decl | entity_decl | state_decl
               | system_decl | event_decl | fn_decl | type_decl | type_alias_decl
               | intent_decl | law_decl | resolver_decl )
               | on_handler | use_decl | test_decl | statement ;

(* Top-level declarations *)
component_decl = "component" IDENT [ VERSION ] "{" { [ "indexed" ] field_def [ "," ] } "}" ;
resource_decl  = [ "transient" ] "resource" IDENT [ VERSION ] "{" { field_def [ "," ] } "}" ;
(* VERSION is an identifier of exactly `v` + digits, e.g. `v2` — the type's
   declared schema version, embedded per type in save_world() output and
   handed to `migrate X(old, from_version)` on load. *)
struct_decl    = "struct" IDENT "{" { field_def [ "," ] } "}" ;
intent_decl    = "intent" IDENT "{" { [ "key" ] IDENT ":" type_expr [ "," ] } "}" ;
law_decl       = "law" IDENT "(" [ typed_params ] ")" block ;
resolver_decl  = "resolver" IDENT "for" IDENT "(" IDENT "," IDENT ")" block ;
constraint_decl = "constraint" IDENT "for" IDENT "(" IDENT "," IDENT ")"
                  [ "watches" IDENT { "," IDENT } ] block ;
entity_decl    = "entity" IDENT "{" { component_entry [ "," ] } "}" ;
entity_expr    = "entity" [ expr ] "{" { component_entry [ "," ] } "}" ;
component_entry = component_init | expr ;
component_init  = IDENT "{" { field_init [ "," ] } "}" ;
field_def      = IDENT ":" [ type_expr "=" ] expr ;
field_init     = IDENT ":" expr ;

state_decl     = "state" IDENT "{" { state_def } "}" ;
state_def      = IDENT "{" { "on" IDENT "->" IDENT [ "when" expr ] [ "," ] } "}" ;

system_decl    = "system" IDENT "(" [ sys_param { "," sys_param } ] ")"
                 { "after" IDENT { "," IDENT } | "before" IDENT { "," IDENT } }
                 block ;
sys_param      = IDENT [ ":" [ "mut" | "accum" ] IDENT ] ;

event_decl     = "event" IDENT "{" { event_field [ "," ] } "}" ;
event_field    = IDENT [ ":" type_expr ] ;
on_handler     = [ "async" ] "on" IDENT [ "once" ] "(" IDENT ")" [ ( "when" | "where" ) expr ] block ;

fn_decl        = [ "pure" | "async" | effect { effect } ] "fn" IDENT [ type_params ] "(" [ param { "," param } ] ")"
                 [ "->" type_expr ] block ;
effect         = "io" | "ecs" | "readonly" | "event" ;
param          = [ "mut" ] IDENT [ ":" type_expr ] ;

type_decl      = "type" IDENT [ type_params ] "{" { variant_def } "}" ;
type_alias_decl = "type" IDENT [ type_params ] "=" type_expr ;
type_params    = "<" IDENT { "," IDENT } ">" ;
type_expr      = single_type { "|" single_type } ;
single_type    = IDENT | "nil"
               | IDENT "<" type_expr { "," type_expr } ">"
               | "(" [ type_expr { "," type_expr } ] ")"
               | "fn" "(" [ type_expr { "," type_expr } ] ")" [ "->" type_expr ] ;
variant_def    = IDENT "{" { field_init [ "," ] } "}" ;

use_decl       = "use" STRING [ "as" IDENT ] [ ":" IDENT ] ;
migrate_decl   = "migrate" IDENT "(" IDENT [ "," IDENT ] ")" block ;
phase_decl     = [ "serial" ] "phase" IDENT ( "[" { IDENT [ "," ] } "]" | "{" { IDENT [ "," ] } "}" ) ;

test_decl      = "test" ( IDENT | STRING ) [ "for" IDENT "in" expr { "," IDENT "in" expr } ] block ;

(* Statements *)
block          = "{" { statement } "}" ;
statement      = let_stmt | let_else_stmt | assign_stmt | if_stmt | while_stmt | for_stmt
               | return_stmt | emit_stmt | schedule_stmt | match_stmt | settle_stmt
               | propose_stmt | next_stmt | constraint_require_stmt
               | "break" | "continue" | expr_stmt ;

let_stmt       = "let" [ "unique" ] [ "mut" ] [ "rec" ] ( IDENT | "(" IDENT { "," IDENT } ")" ) [ ":" type_expr ] "=" expr ;
let_else_stmt  = "let" [ "mut" ] IDENT ( "{" { IDENT [ ":" IDENT ] [ "," ] } "}" | "(" IDENT ")" ) [ ":" type_expr ] "=" expr "else" block ;
assign_stmt    = expr "=" expr ;
if_stmt        = "if" expr block [ "else" ( if_stmt | block ) ] ;
while_stmt     = "while" expr block ;
for_stmt       = "for" ( IDENT { "," IDENT } | "(" IDENT { "," IDENT } ")" | "[" IDENT { "," IDENT } "]" ) "in" expr [ "where" expr ] block ;
return_stmt    = "return" [ expr ] ;
emit_stmt      = "emit" IDENT "{" { field_init [ "," ] } "}" [ "after" expr ] ;
schedule_stmt  = "schedule" [ "serial" ] "[" schedule_target { "," schedule_target } "]" ;
schedule_target = IDENT [ "." IDENT ] | "system" "::" IDENT { "::" IDENT } ;
match_stmt     = "match" expr "{" { match_case } "}" ;
match_case     = match_pattern [ ( "when" | "if" ) expr ] "=>" ( block | expr ) ;
settle_stmt    = "settle" "{" { statement } "}" ;
propose_stmt   = "propose" IDENT "{" { field_init [ "," ] } "}" ;
next_stmt      = "next" "(" expr "," component_init ")" ;
constraint_require_stmt = "require" expr "else" STRING ;
match_pattern  = "_" | INT | FLOAT | STRING | "true" | "false"
               | "has" IDENT [ "(" IDENT ")" ]
               | IDENT [ "{" { IDENT [ ":" IDENT ] [ "," ] } [ ".." ] "}" | "(" IDENT ")" ] ;
expr_stmt      = expr ;

(* Expressions — listed in ascending precedence *)
expr           = pipe_expr ;
pipe_expr      = or_expr { "|>" or_expr } ;
or_expr        = and_expr { "or" and_expr } ;
and_expr       = eq_expr { "and" eq_expr } ;
eq_expr        = cmp_expr { ( "==" | "!=" | "is" ) cmp_expr } ;
cmp_expr       = bitor_expr { ( "<" | ">" | "<=" | ">=" ) bitor_expr } ;
bitor_expr     = bitxor_expr { "|" bitxor_expr } ;
bitxor_expr    = bitand_expr { "^" bitand_expr } ;
bitand_expr    = shift_expr { "&" shift_expr } ;
shift_expr     = add_expr { ( "<<" | ">>" ) add_expr } ;
add_expr       = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr       = unary_expr { ( "*" | "/" | "%" ) unary_expr } ;
unary_expr     = ( "-" | "not" | "!" | "~" | "await" ) unary_expr | "async" postfix_expr | postfix_expr ;
postfix_expr   = primary { "." IDENT | "(" [ call_arg { "," call_arg } ] ")" | "[" expr "]" | "?" } ;
call_arg       = expr | ".." expr ;

primary        = INT | FLOAT | STRING | FSTRING | "true" | "false" | "nil"
               | "[" [ expr { "," expr } ] "]"
               | "{" [ expr ":" expr { "," expr ":" expr } ] "}"
               | "(" expr ")"
               | match_expr
               | if_expr
               | entity_expr
               | system_ref
               | field_accessor_fn
               | IDENT "::" IDENT [ "{" { field_init [ "," ] } [ ".." expr ] "}" ]
               | IDENT "{" { field_init [ "," ] } [ ".." expr ] "}"
               | IDENT
               | fn_expr
               | query_expr ;

match_expr     = "match" expr "{" { match_case } "}" ;
if_expr        = "if" expr "{" expr "}" "else" ( if_expr | "{" expr "}" ) ;
system_ref     = "system" "::" IDENT { "::" IDENT } ;
field_accessor_fn = "." IDENT { "." IDENT } ;

fn_expr        = "fn" "(" [ param { "," param } ] ")" [ "->" type_expr ] block ;

query_expr     = "query" "{" [ [ "mut" ] IDENT { "," [ "mut" ] IDENT } ] "}" [ "select" IDENT { "," IDENT } ] [ "where" expr ] ;

(* Terminals *)
IDENT          = ( LETTER | "_" ) { LETTER | DIGIT | "_" } ;
INT            = DIGIT { DIGIT } ;
FLOAT          = DIGIT { DIGIT } "." DIGIT { DIGIT } ;
STRING         = '"' { CHAR } '"' ;
FSTRING        = 'f"' { CHAR | "{" expr [ ":" FORMAT_SPEC ] "}" | "${" expr [ ":" FORMAT_SPEC ] "}" } '"' ;
TRIPLE_FSTRING = 'f"""' { CHAR | "${" expr [ ":" FORMAT_SPEC ] "}" } '"""' ;   (* NB: only ${} interpolates; bare {} are literal *)

FORMAT_SPEC    = { CHAR } ;   (* Python-style mini-language: [[fill]align][sign][#][0][width][.precision][type] *)
```

**Compatibility extension (v0.5 DX mode only):**

```ebnf
match_case_compat_v05 = IDENT [ "{" { IDENT "," } [ IDENT ] [ "," ] [ ".." ] "}" ] "=>" block ;
primary_compat_v05    = ... | IDENT "::" IDENT ;
```

`match_case_compat_v05`: the `..` rest marker, if present, must be the final entry inside the braces. Named bindings after `..` are a parse error (`E2503`).

`primary_compat_v05` allows zero-field sum-variant shorthand (`Type::Variant`) when ambiguity can be resolved.

---

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

## 5. Expressions

### 5.1 Tuple Expression

```
(expr1, expr2, ...)
```

Creates a fixed-size, ordered sequence of values. Tuples are distinct from lists: they have a fixed length known at compile time, and their type signature `(T1, T2, ...)` captures the exact type of each element.

```
let pair: (int, str) = (10, "hello")
let empty: () = ()
let single: (int,) = (5,)
```

### 5.2 Function Calls and Spread Operator

Functions are called using parentheses `f(arg1, arg2)`.

You can use the spread operator `..` to unpack a tuple into individual function arguments. The spread operator is **only supported for tuples**, not lists, because the compiler needs to know the exact number of arguments and their types at compile time.

```
fn add3(a: int, b: int, c: int) -> int { return a + b + c }

let args = (1, 2, 3)
let sum = add3(..args) // Equivalent to add3(1, 2, 3)
```

### 5.3 Pipeline

```
<expr> |> <fn_or_call>
```

Passes the left expression as the first argument to the right function. Equivalent to `f(left, ...)` or `f(left)`.

```
[1, 2, 3] |> map(fn(x) { return x * 2 })
// Equivalent to: map([1, 2, 3], fn(x) { return x * 2 })
```

**Precedence:** `|>` has the **lowest** operator precedence (level 1 in §1.2). Arithmetic, comparison, and logical operators all bind tighter than `|>`. This means expressions like `list |> reduce(0, fn(a, x) { return a + x }) / len(list)` parse the `/ len(list)` as part of the pipeline's right-hand side, **not** as a division applied to the pipeline result. To apply arithmetic to a pipeline's output, bind the pipeline result to a variable first:

```
let total = scores |> reduce(0, fn(a, x) { return a + x })
let avg = total / len(scores)
```

Pipelines are restricted to **pure or readonly** computation (see §8): the right-hand side cannot call side-effecting builtins such as `set`, `spawn`, or use `emit`. User-defined callees must be known pure (`pure fn` or inferred pure) or `readonly fn` (performs only ECS reads). ECS read builtins (`get`, `has`, `entities`, `query_where`, `query_map`, `query_count`, `with_field`, `peek`, `lookup`) are classified as `readonly` and permitted in pipelines. Assignments to **outer** variables from code executed as part of the pipeline are also rejected by the static checker.

**Field accessor shorthand:** `.field` parses as a one-argument projection closure. Chains are allowed.

```rad
let total = mods |> map(.flat) |> sum
let hps = units |> map(.stats.hp)
```

### 5.4 Function Expression

```
fn(<param>, ...) { <body> }
fn([<a>, <b>], <param>, ...) { <body> }
```

Creates an anonymous function (closure). Captures the enclosing environment.

**List destructuring in parameters:** Bracket syntax `fn([name, phase])` unpacks a list or tuple argument into named bindings. Multiple parameters can be destructured: `fn([a, b], [c, d])`. Plain and destructured parameters can be mixed: `fn(acc, [key, val])`. Underscore `_` may appear multiple times as a discard: `fn([_, mid, _])`. An optional type annotation can follow the brackets: `fn([a, b]: (int, str))`. The `mut` keyword before the brackets makes all destructured bindings mutable: `fn(mut [a, b]) { a = a * 10 }`. The checker infers element types from pipeline context (e.g., `list<(int, str)>` flowing into `map` gives `a: int, b: str`) and reports arity mismatches for tuples.

Captured variables follow normal mutability rules:
- Captured `let` bindings are read-only in the closure.
- Captured `let mut` bindings are shared between the closure and outer scope, so reassignment inside the closure updates the outer value.

### 5.5 Variant Check (`is`)

```
<expr> is <VariantName>
```

The `is` operator checks if a sum type or state machine instance is currently a specific variant or state. It evaluates to `true` or `false`.

```
let door = DoorState::Locked
if door is Locked {
    print("Locked")
}

let result = Result::Ok { value: 42 }
let is_ok = result is Ok
```

The right-hand side must be an identifier corresponding to a valid variant or state for the type of the left-hand expression. The type checker statically verifies this.

### 5.6 If Expression

```
if <cond> { <expr> } else { <expr> }
```

In expression position, `if` returns a value and requires an `else` branch. `else if` chains are allowed.

```rad
let tier = if hp < 25 { "danger" } else if hp < 60 { "hurt" } else { "ok" }
```

Statement-position `if` keeps the block-oriented behavior described in §4.

### 5.7 String interpolation

RAD supports interpolation in both `f"..."` strings and regular strings:

```
let city = "Neo Arcadia"
let pop = 1200
let a = f"city={city}, pop={pop}"
let b = "city=${city}, pop=${pop}"
```

For regular strings, interpolation uses `${...}`. `f"..."` continues to support both
`{...}` and `${...}` forms.

#### Format specifiers

F-string interpolations support Python-style format specifiers after a colon:

```
f"{expr:spec}"
f"${expr:spec}"
```

The format spec follows the Python format mini-language: `[[fill]align][sign][#][0][width][.precision][type]`.

| Component | Values | Description |
|---|---|---|
| fill | any character | Padding character (default: space) |
| align | `<` `>` `^` | Left, right, or center alignment. Numbers default to right (`>`), strings to left (`<`). |
| sign | `+` `-` (space) | `+` shows sign for positive and negative; `-` shows sign only for negative (default); space adds a leading space for positive values. |
| `#` | | Alternate form: adds `0b`, `0o`, `0x`, or `0X` prefix for binary, octal, and hex. |
| `0` | | Zero-pad: fills with zeros between the sign/prefix and digits. |
| width | integer | Minimum field width. |
| .precision | integer | For floats: digits after decimal point. For strings: max characters (truncates). |
| type | `d` `f` `e` `E` `b` `o` `x` `X` `s` `%` | `d` decimal, `f` fixed-point, `e`/`E` scientific, `b` binary, `o` octal, `x`/`X` hex, `s` string, `%` percentage. |

Examples:

```
let pi = 3.14159
print(f"{pi:.2f}")           // "3.14"
print(f"{42:06d}")           // "000042"
print(f"{255:#x}")           // "0xff"
print(f"{'hi':>10}")         // "        hi"
print(f"{42:+d}")            // "+42"
print(f"{0.75:.1%}")         // "75.0%"
print(f"{12345.6789:.2e}")   // "1.23e+04"
```

Format specifiers are supported in both `f"..."` and `f"""..."""` f-strings.
Regular string interpolation (`"${expr}"`) does not support format specifiers.

The `format_value(value, spec)` builtin provides the same functionality as a standalone function call (see section 6).

#### Triple-quoted f-strings

`f"""..."""` is a multi-line f-string where **only `${expr}` triggers interpolation**.
Bare `{` and `}` are literal text — no escaping needed. This is designed for
generating code (C, JSON, etc.) where braces appear frequently.

> **Common pitfall:** `{expr}` does NOT interpolate inside `f"""..."""`. Use `${expr}` instead.

| Syntax        | `{x}` | `${x}` | Bare `{` / `}` |
|---------------|--------|---------|-----------------|
| `f"..."`      | interpolates | interpolates | must double (`{{`/`}}`) or escape (`\{`/`\}`) |
| `f"""..."""`  | literal text | interpolates | literal text (no escaping needed) |

```
let n = 3
let code = f"""
    if (__nargs != ${n}) {
        fprintf(stderr, "arity mismatch\n");
        exit(1);
    }
"""
```

Inner double-quotes do not need escaping since the delimiter is `"""`.
Use `\$` to produce a literal `$` when followed by `{`.

### 5.8 Multi-line Strings

RAD supports Zig-style line-based string literals. They are prefixed with `\\` and consume the rest of the line. Multiple consecutive `\\` lines are concatenated with newlines, ignoring any indentation before the `\\`.

```
let menu = \\Help Menu:
           \\  - Option 1
           \\  - Option 2
```

Quotes inside multi-line strings do not need to be escaped.

### 5.9 Component Expression

```
<ComponentName> { <field>: <expr>, ... }
```

Creates a component value. Fields not specified use the defaults from the component declaration.

Component updates may use spread-style base copying with `..base` as the final entry:

```rad
// Caller must allow Option propagation (e.g. `fn example() -> any`).
let old = get(hero, Stats)?
let next = Stats { hp: old.hp - 10, ..old }
set(hero, next)
```

Rules:
- `..base` can appear at most once.
- `..base` must be the final entry in the literal.
- Explicit fields always override fields copied from `base`.

### 5.10 Entity Literal Expression

```
entity [ <name_expr> ] {
    <Component> { <field>: <expr>, ... },
    <expr>,
    ...
}
```

Spawns a new entity, attaches the listed components, and returns the entity ID. This is the expression-level counterpart to the named `entity Name { ... }` declaration (§3.3). The type of the expression is `entity`.

An optional **name expression** between `entity` and `{` assigns a name to the entity, making it retrievable via `get_entity()`. The name can be any expression evaluating to a string. If omitted, the entity is anonymous.

Each entry inside the braces is a **component entry**: either a component initializer (`Component { field: value }`) or an expression that evaluates to a component value. The parser uses lookahead to disambiguate: tokens matching `Ident {`, `Ident.`, or `Ident::` are parsed as component initializers; all other tokens begin an expression. This allows variables, function calls, and other expressions to supply components alongside traditional initializers. Entity literal expressions may be nested.

```rad
// Anonymous
let hero = entity {
    Name { value: "Hero" },
    Health { hp: 100, max: 100 }
}

// Named (string literal)
let e = entity "player" { Health { hp: 100 } }
let found = get_entity("player")   // returns the same entity

// Named (variable)
fn load_file(path: str) -> entity {
    return entity path { FilePath { path: path }, Unparsed {} }
}

// As a function argument:
register_npc(entity f"npc_{id}" { Name { value: "Goblin" }, Health { hp: 30, max: 30 } })

// Expression components (variables, function calls)
let hp = Health { hp: 50, max: 50 }
let mob = entity { Name { value: "Rat" }, hp, make_position(0.0, 0.0) }
```

### 5.11 State Reference

```
<MachineName>::<StateName>
```

Creates a state machine instance in the specified state.

### 5.12 Sum Type Variant Expression

```
<TypeName>::<VariantName> { <field>: <expr>, ... }
```

Builds a value of the given sum type. If `{ ... }` is empty, the variant must have no fields (or only defaults). This syntax is disambiguated from state references: a following `{` begins field values for the variant, not a state literal.

In `--compat-v0.5-dx` mode, zero-field shorthand `TypeName::VariantName` is accepted for sum variants and may emit compatibility diagnostics when a name is also a state machine.

### 5.13 Compatibility Flags

The CLI supports compatibility and warning-policy flags for v0.5 DX rollout:

- `--compat-v0.5-dx` enables v0.5 DX compatibility syntax and behavior.
- `--warn-compat` enables compatibility warnings (default).
- `--no-warn-compat` disables compatibility warnings.
- `--deny-warnings` turns warnings into a non-zero process exit.
- `--profile-copies` enables runtime diagnostics for hidden `Arc` deep clones. When a list mutation (push, set, extend) triggers `Arc::make_mut` on a shared backing buffer, a diagnostic is emitted to stderr with the source line number and element count. Use this to find unexpected O(n) copies in hot loops. See [Memory Model](memory-model.md).

---

## 6. Built-in Functions

### General
| Function | Signature | Description |
|---|---|---|
| `print` | `print(args...)` | Print values to stdout |
| `str` | `str(val) -> str` | Convert to string |
| `int` | `int(val) -> int` | Convert to integer |
| `float` | `float(val) -> float` | Convert to float |
| `len` | `len(list\|str\|map) -> int` | Length of list, string, or map |
| `range` | `range(n)`, `range(a, b)`, `range(a, b, step)` | Generate list of integers |
| `typeof` | `typeof(val) -> str` | Type name as string |
| `variant_of` | `variant_of(val) -> str` | Returns the variant name if `val` is a sum type or state, else `nil` |
| `abs` | `abs(num) -> num` | Absolute value |
| `min` | `min(a, b) -> num` | Minimum |
| `max` | `max(a, b) -> num` | Maximum |
| `int_div` | `int_div(a: int, b: int) -> int` | Truncating integer division (rounds toward zero) |
| `rand_int` | `rand_int(min: int, max: int) -> int` | Random integer in inclusive range `[min, max]` |
| `rand_float` | `rand_float() -> float` | Random float in range `[0.0, 1.0)` |
| `rand_bool` | `rand_bool() -> bool` | Random boolean |
| `rand_seed` | `rand_seed(seed: int) -> nil` | Set PRNG seed for deterministic pseudo-random sequences |

For a small replayable game-style example, see `tests/conformance/rng_seeded_dungeon_reproducible.rad`.

### I/O, File, HTTP, and Networking
| Function | Signature | Description |
|---|---|---|
| `print` | `print(...) -> nil` | Print values to stdout (with newline) |
| `eprint` | `eprint(...) -> nil` | Print values to stderr (with newline) |
| `log` | `log(level: str, data: map) -> nil` | Print structured JSON log with trace context; only string-keyed map entries are emitted |
| `metric` | `metric(type: str, name: str, value: float, tags: map) -> nil` | Print structured JSON metric with trace context; only string-keyed tags are emitted |
| `trace_id` | `trace_id() -> int \| nil` | Get the current distributed trace ID inside event handling, or `nil` outside an event context |
| `write_stdout` | `write_stdout(str) -> nil` | Write string to stdout without newline |
| `write_stderr` | `write_stderr(str) -> nil` | Write string to stderr without newline |
| `flush_stdout` | `flush_stdout() -> nil` | Explicitly flush stdout |
| `input` | `input([prompt]) -> str` | Print optional prompt and read a line from stdin |
| `readline` | `readline() -> str` | Read a line from stdin |
| `read_stdin_all` | `read_stdin_all() -> str` | Read all of stdin until EOF |
| `read_file` | `read_file(path: str) -> str` | Read UTF-8 text file contents |
| `write_file` | `write_file(path: str, content: str) -> nil` | Write UTF-8 text file contents (overwrite) |
| `append_file` | `append_file(path: str, content: str) -> nil` | Append text to file |
| `read_file_bytes` | `read_file_bytes(path: str) -> list<int>` | Read file as bytes |
| `write_file_bytes` | `write_file_bytes(path: str, bytes: list<int>) -> nil` | Write bytes to file |
| `file_exists` | `file_exists(path: str) -> bool` | Check if a file or directory exists |
| `remove_file` | `remove_file(path: str) -> nil` | Delete a file |
| `list_dir` | `list_dir(path: str) -> list<str>` | List directory entries |
| `create_dir` | `create_dir(path: str) -> nil` | Create a directory (including parents) |
| `remove_dir` | `remove_dir(path: str) -> nil` | Recursively remove a directory |
| `http_get` | `http_get(url: str) -> str` | Blocking HTTP GET, returns response body text |
| `http_post` | `http_post(url: str, body: str) -> str` | Blocking HTTP POST, returns response body text |
| `http_post_json` | `http_post_json(url: str, body: str) -> str` | HTTP POST with JSON content-type |
| `http_request` | `http_request(method: str, url: str, headers: map, body: str) -> map` | Full HTTP request, returns map with status, headers, body |
| `tcp_connect` | `tcp_connect(host: str, port: int) -> int` | Open TCP connection, return handle |
| `tcp_listen` | `tcp_listen(host: str, port: int) -> int` | Bind TCP listener, return handle |
| `tcp_accept` | `tcp_accept(handle: int) -> int` | Accept TCP connection, return handle |
| `tcp_accept_timeout` | `tcp_accept_timeout(handle: int, timeout_ms: int) -> Option<int>` | Accept TCP connection with a deadline; `timeout_ms = 0` polls |
| `tcp_read` | `tcp_read(handle: int, max_bytes: int) -> str` | Read from TCP stream |
| `tcp_write` | `tcp_write(handle: int, data: str) -> nil` | Write to TCP stream |
| `tcp_close` | `tcp_close(handle: int) -> nil` | Close TCP handle |
| `udp_bind` | `udp_bind(host: str, port: int) -> int` | Bind UDP socket, return handle |
| `udp_recv_from` | `udp_recv_from(handle: int, max_bytes: int) -> (str, str, int)` | Receive one UDP datagram as `(data, host, port)` |
| `udp_recv_from_timeout` | `udp_recv_from_timeout(handle: int, max_bytes: int, timeout_ms: int) -> Option<(str, str, int)>` | Receive one UDP datagram with a deadline; `timeout_ms = 0` polls |
| `udp_recv_from_bytes` | `udp_recv_from_bytes(handle: int, max_bytes: int) -> (list<int>, str, int)` | Receive one UDP datagram as `(data_bytes, host, port)` |
| `udp_recv_from_bytes_timeout` | `udp_recv_from_bytes_timeout(handle: int, max_bytes: int, timeout_ms: int) -> Option<(list<int>, str, int)>` | Receive one UDP datagram as bytes with a deadline; `timeout_ms = 0` polls |
| `udp_recv_bytebuf` | `udp_recv_bytebuf(handle: int, max_bytes: int) -> (any, str, int)` | Receive one UDP datagram as `(bytebuf, host, port)` |
| `udp_recv_bytebuf_timeout` | `udp_recv_bytebuf_timeout(handle: int, max_bytes: int, timeout_ms: int) -> Option<(any, str, int)>` | Receive one UDP datagram as a native byte buffer with a deadline; `timeout_ms = 0` polls |
| `udp_send_to` | `udp_send_to(handle: int, host: str, port: int, data: str) -> int` | Send one UDP datagram, return bytes sent |
| `udp_send_to_bytes` | `udp_send_to_bytes(handle: int, host: str, port: int, data_bytes: list<int>) -> int` | Send one UDP byte datagram, return bytes sent |
| `udp_send_bytebuf` | `udp_send_bytebuf(handle: int, host: str, port: int, data: any) -> int` | Send one native byte-buffer UDP datagram, return bytes sent |
| `udp_close` | `udp_close(handle: int) -> nil` | Close UDP socket handle |

Browser clients cannot call these native socket builtins from WASM. Use a
host-owned WebTransport edge process for browser datagrams and keep RAD as the
UDP authority; see [WebTransport Edge Networking](../guide/webtransport-networking.md).

### Byte Buffers
| Function | Signature | Description |
|---|---|---|
| `bytebuf_new` | `bytebuf_new(size: int) -> any` | Create a zero-filled native byte buffer |
| `bytebuf_len` | `bytebuf_len(buf: any) -> int` | Return byte length |
| `bytebuf_get` | `bytebuf_get(buf: any, index: int) -> int` | Read one byte as `0..255` |
| `bytebuf_set_u8` | `bytebuf_set_u8(buf: any, index: int, value: int) -> any` | Write one byte |
| `bytebuf_set_u32_le` | `bytebuf_set_u32_le(buf: any, offset: int, value: int) -> any` | Write a little-endian unsigned 32-bit int |
| `bytebuf_set_i32_le` | `bytebuf_set_i32_le(buf: any, offset: int, value: int) -> any` | Write a little-endian signed 32-bit int |
| `bytebuf_get_u32_le` | `bytebuf_get_u32_le(buf: any, offset: int) -> int` | Read a little-endian unsigned 32-bit int |
| `bytebuf_get_i32_le` | `bytebuf_get_i32_le(buf: any, offset: int) -> int` | Read a little-endian signed 32-bit int |
| `bytebuf_to_list` | `bytebuf_to_list(buf: any) -> list<int>` | Convert to a byte list |
| `bytebuf_from_list` | `bytebuf_from_list(bytes: list<int>) -> any` | Convert a byte list to a native byte buffer |

### ECS
| Function | Signature | Description |
|---|---|---|
| `get` | `get(entity, ComponentName) -> Option` | `Some(component)` or `None` |
| `require` | `require(entity, ComponentName) -> component` | Returns component directly; runtime error if missing |
| `require_all` | `require_all(entity, ComponentName...) -> list` | Returns components in requested order; runtime error on first missing |
| `set` | `set(entity, component_value)` | Set/replace component on entity |
| `has` | `has(entity, ComponentName) -> bool` | Check if entity has component |
| `spawn` | `spawn([name], components...) -> entity_id` | Create a new entity |
| `remove` | `remove(entity, ComponentName) -> bool` | Remove a component from an entity |
| `despawn` | `despawn(entity) -> bool` | Destroy an entity |
| `entities` | `entities([ComponentName...]) -> list` | Return all entity IDs, or filter by entities having all listed components |
| `query_where` | `query_where(ComponentName..., fn) -> list` | Filter entities having components using a predicate function on entity ID |
| `query_map` | `query_map(ComponentName..., fn) -> list` | Map over entities having components using a function on entity ID |
| `query_count` | `query_count(ComponentName...) -> int` | Return the number of entities having the given components |
| `with_field` | `with_field(entities, ComponentName, FieldName, fn) -> list` | Filter a list of entities by evaluating a predicate function on a specific component field |
| `lookup` | `lookup(ComponentName, field_name: str, value) -> Option<entity>` | O(1) lookup: returns `Some(entity_id)` for the first entity whose `indexed` field matches `value`, or `None` |

> **Note:** `lookup()` requires the field to be declared as `indexed` in the component declaration. Non-indexed fields produce a runtime error.

> **Note:** The `query { ... } select ... where ...` expression is the preferred, fastest way to query entities and project component data. The builtin functions above are maintained for dynamic use cases.

> **See also:** Entity literal expressions (§5.9) provide a declarative alternative to `spawn()` + `set()` when creating entities with known components inline. Named entity literals (`entity "name" { ... }`) also replace the `spawn("name") + set()` pattern.

### World Forking (Speculative Execution)
| Function | Signature | Description |
|---|---|---|
| `fork` | `fork() -> world_fork` | Copy-on-write snapshot of the entire ECS world (entities, components, archetypes). O(A) shallow `Arc` refcount bumps on column handles/maps; actual data cloning deferred to first mutation via `Arc::make_mut` (see [Memory Model](memory-model.md)). |
| `simulate` | `simulate(fork, systems, ticks) -> world_fork` | Run the listed systems on a fork for N ticks, returning the updated fork. Ground-truth semantics live in the Rust VM. |
| `commit` | `commit(fork) -> nil` | Atomically replace the live ECS world with the fork's state. **Clears all pending events.** |
| `peek` | `peek(fork, entity, Component) -> Option<Component>` | Read a component from the fork without committing. Returns `None` if the entity or component does not exist in the fork. Values are deep-copied across the air gap (O(F) for F fields; string fields are O(1) via `Arc<str>`). |

**Static system list.** The second argument to `simulate` must be a **list literal** whose elements are **`system::…` references** naming declared `system`s — for example `simulate(f, [system::Decay, system::Physics], 3)`. String literals in that list are rejected at compile time (use `system::Name` instead). Multi-segment paths use additional `::` (for example `system::alias::Sys` → the same qualified-name rules as `schedule [alias.Sys, …]`). Unknown system names are compile-time errors.

As a convenience for sharing one schedule across call sites, a reference to a **top-level immutable binding** whose value is such a list literal is also accepted and const-folds to it — `let ROLLOUT = [system::Decay, system::Physics]` then `simulate(f, ROLLOUT, 3)`. The binding must be top-level, immutable (`let`, not `let mut`), and a plain list literal of `system::…` references; an arbitrary variable, a `let mut`, or a computed expression (a function call, concatenation, etc.) is still rejected, because the checker resolves the schedule statically with no dataflow analysis.

**Syntax.** `system::Name` is an expression of type `system`. At runtime, values in the schedule list are **`system` references** (`Value`/`Object::SystemRef`), not strings.

**Design note.** `simulate` is exposed as a builtin call for parser and composability, but the second argument is checked like a dedicated syntactic form (as if it were a macro or keyword), not like a normal function parameter.

`fork()` creates a copy-on-write snapshot of the ECS world (O(A) `Arc` refcount bumps, not a full deep copy). `simulate()` runs systems on the fork in isolation: IO, `commit()`, unsafe event-effect calls such as `transition`, and unsafe handler chains are statically forbidden. `emit` statements are allowed; emitted events dispatch on the fork's own event queue and any events still pending at the end stay with the returned fork. `commit()` replaces the real world (O(1) pointer swap) and **discards all pending events** in the main timeline, since they reference pre-commit state that no longer exists. `peek()` reads from a fork without modifying any state; values are deep-copied across the air gap into the caller's heap (string fields are O(1) via shared `Arc<str>`).

### State Machines
| Function | Signature | Description |
|---|---|---|
| `transition` | `transition(state_inst, event_name) -> Result` | `Ok(state_inst)` or `Err(message)`; transition guards are declared in `state` with `when ...` |

### Option / Result helpers
| Function | Signature | Description |
|---|---|---|
| `unwrap` | `unwrap(option_or_result) -> value` | Unwraps `Some` / `Ok`; errors on `None` / `Err` |
| `unwrap_or` | `unwrap_or(option_or_result, default) -> value` | Unwraps `Some` / `Ok`; returns `default` on `None` / `Err` |
| `expect` | `expect(option_or_result, [msg]) -> value` | Like `unwrap` with a custom failure message |
| `map_or` | `map_or(option_or_result, default, fn) -> value` | Returns `fn(inner)` for `Some/Ok`, otherwise `default` |
| `is_some` | `is_some(option_or_result) -> bool` | Returns `true` if `Some` / `Ok` |
| `is_none` | `is_none(option_or_result) -> bool` | Returns `true` if `None` / `Err` |

### List & Pipeline
| Function | Signature | Description |
|---|---|---|
| `map` | `map(list, fn) -> list` | Transform each element |
| `filter` | `filter(list, fn) -> list` | Keep elements where fn returns truthy |
| `reduce` | `reduce(list, init, fn) -> value` | Fold list to single value |
| `flat_map` | `flat_map(list, fn) -> list` | Map then flatten (callback returns list) |
| `group_by` | `group_by(list, fn) -> map` | Group elements by key function |
| `push` | `push(list, val) -> list` | Return new list with val appended (use `list << val` for in-place mutation) |
| `pop` | `pop(list) -> value` | Return the last element (same as `pop_last`) |
| `pop_last` | `pop_last(list) -> value` | Return the last element |
| `drop_last` | `drop_last(list) -> list` | Return list without the last element |
| `sort` | `sort(list) -> list` | Return sorted copy |
| `sort_by` | `sort_by(list, fn) -> list` | Return sorted copy using key function |
| `reverse` | `reverse(list\|str) -> list\|str` | Return reversed copy |
| `slice` | `slice(list, start, end) -> list` | Return sub-list (also works on strings) |
| `append` | `append(list, list) -> list` | Concatenate two lists |
| `extend` | `extend(list, list) -> list` | Alias for `append` |
| `zip` | `zip(list, list) -> list` | Pair elements into `[[a, b], ...]` |
| `enumerate` | `enumerate(list) -> list` | Return `[[0, elem₀], [1, elem₁], ...]` index-element pairs |
| `find` | `find(list, fn) -> Option` | First element where `fn` returns truthy, or `None` |
| `max_by` | `max_by(list, fn) -> Option` | Element with largest key from `fn`, or `None` if empty |
| `min_by` | `min_by(list, fn) -> Option` | Element with smallest key from `fn`, or `None` if empty |
| `contains` | `contains(list\|str\|map, val) -> bool` | Check membership |

### Map / Collection
| Function | Signature | Description |
|---|---|---|
| `keys` | `keys(map\|component) -> list` | Sorted key names (deterministic) |
| `values` | `values(map) -> list` | List of map values (sorted deterministically by key) |
| `entries` | `entries(map) -> list` | List of `[key, value]` pairs (sorted deterministically by key) |
| `merge` | `merge(map, map) -> map` | Merge maps (second wins) |
| `remove_key` | `remove_key(map, key) -> map` | Return new map with key removed |

### String
| Function | Signature | Description |
|---|---|---|
| `split` | `split(str, delim) -> list<str>` | Split string by delimiter |
| `join` | `join(list, sep) -> str` | Join list elements with separator |
| `trim` | `trim(str) -> str` | Strip whitespace |
| `replace` | `replace(str, old, new) -> str` | Replace all occurrences |
| `starts_with` | `starts_with(str, prefix) -> bool` | Prefix check |
| `ends_with` | `ends_with(str, suffix) -> bool` | Suffix check |
| `regex_is_match` | `regex_is_match(pattern: str, text: str) -> bool` | True when regex pattern matches text |
| `regex_find` | `regex_find(pattern: str, text: str) -> Option[str]` | First regex match as `Some(value)` or `None` |
| `chr` | `chr(int) -> str` | Unicode code point to character |
| `ord` | `ord(str) -> int` | First character to code point |
| `chars` | `chars(str) -> list<str>` | Split into character list |
| `to_upper` | `to_upper(str) -> str` | Uppercase conversion |
| `to_lower` | `to_lower(str) -> str` | Lowercase conversion |
| `format` | `format(template, args...) -> str` | Replace `{}` placeholders with arguments in order |
| `format_value` | `format_value(value, spec: str) -> str` | Format a single value using a Python-style format specifier (see §5.6) |

### Date / Time
| Function | Signature | Description |
|---|---|---|
| `now_unix_s` | `now_unix_s() -> int` | Current UNIX timestamp (seconds) |
| `now_unix_ms` | `now_unix_ms() -> int` | Current UNIX timestamp (milliseconds) |

### Safe Conversion
| Function | Signature | Description |
|---|---|---|
| `try_int` | `try_int(val) -> Option` | Safe int conversion (no error) |
| `try_float` | `try_float(val) -> Option` | Safe float conversion (no error) |

### Testing
| Function | Signature | Description |
|---|---|---|
| `assert` | `assert(condition: bool, msg: str)` | Assert condition is true; runtime error with `msg` on failure |
| `assert_eq` | `assert_eq(a, b)` | Assert two values are equal; runtime error on mismatch |

### Test Data Generation
| Function | Signature | Description |
|---|---|---|
| `gen_int` | `gen_int() -> list<int>` | Generate a list of test integers |
| `gen_float` | `gen_float() -> list<float>` | Generate a list of test floats |
| `gen_str` | `gen_str() -> list<str>` | Generate a list of test strings |
| `gen_bool` | `gen_bool() -> list<bool>` | Generate a list of test booleans |
| `gen_list` | `gen_list(list) -> list<list<any>>` | Generate a list of test lists |

`gen_*` builtins are deterministic test generators, not runtime random-number APIs.
Use `rand_*` for pseudo-random values.

List-transforming builtins return new lists instead of mutating in place.
`pop`/`pop_last` return the removed element; use `drop_last` to get the remaining list.
Rebind explicitly:

```rad
let mut xs = [1, 2, 3]
xs = push(xs, 4)
xs = sort(xs)
let popped = pop(xs)
xs = drop_last(xs)
```

Calling `push(xs, v)` as a standalone statement has no effect on `xs` unless the returned list is assigned/rebound.

---

## 7. Execution Model

### 7.1 Program Structure

A Rad program consists of top-level declarations and statements. Execution proceeds in two passes:

**Pass 1 (Registration):**
1. `use` imports are processed
2. `component`, `event`, `state`, `type` declarations are registered
3. `fn`, `pure fn`, `system`, `on` declarations are registered

**Pass 2 (Execution):**
1. `entity` declarations create entities
2. Top-level statements execute in order
3. If a `main()` function exists, it is called after all top-level statements

### 7.1.1 Entry `main` — explicit return type (project convention)

In gradual mode, omitting a return type on a function is allowed but the checker may **warn** that the return type defaults to inferred/`any`. For the zero-parameter entry function `main`, this repository standardizes on an **explicit** return type:

- Use **`fn main() -> nil { ... }`** when the program does not return a meaningful value (the usual case for scripts and examples). The checker special-cases `main() -> nil`: the `?` operator is allowed, and propagation of `None`/`Err` exits the program cleanly rather than producing a type error.
- Use **`fn main() -> T { ... }`** when `main` returns a value that callers or tooling care about.

This aligns with `--strict-types` (which requires explicit return types on private functions), keeps `rad ... --deny-warnings` usable on the example tree, and matches the expectation that **public and entry-point APIs state their contracts**. Tests or local snippets that deliberately exercise the “missing return type” warning are exempt.

### 7.2 System Scheduling

Systems run when explicitly invoked via `<SystemName>()` or `schedule [ S1, S2, ... ]`.

- A single system call executes one system immediately.
- `schedule [ ... ]` first computes a deterministic **topological sort** from each system’s `after` / `before` constraints.
- Circular dependency among the listed systems is an error.
- After ordering, the runtime partitions systems into conflict-free batches using each system parameter’s mutability:
  - `mut` parameters are writes
  - non-`mut` parameters are reads
  - systems conflict on write/write or write/read overlap
  - `accum` resource parameters are **reductions**, not plain writes: two `accum`-writers of the same resource do **not** conflict (their per-field deltas fold), but an `accum`-writer conflicts with any plain reader or writer of that resource
  - members of the same `serial phase` always conflict with each other

The native VM runs conflict-free batches in parallel worker VMs using the same
snapshot as input. Each worker buffers ECS writes and next-frame events; the
main VM then merges writes in schedule order and sorts parallel-emitted events
by trace id, then event name. `accum` resources merge by summing each worker's
per-field delta against the batch's base snapshot, in schedule order. The wasm
VM uses the same worker isolation and merge rules but executes the batch
sequentially.

**Serial execution levers.** `schedule serial [ ... ]` runs the listed systems
one at a time in topological order on the main VM — no worker snapshots, no
merge — the per-call spelling of the global `rad run --serial-schedule` flag,
and the one-keyword differential test against the parallel scheduler. A
`serial phase` scopes the same intent to a named group (§3.5.1). Explicit
speculation (`simulate_par`, `simulate_many`) is unaffected by all three.

### 7.3 Event Ordering

1. Within a single flush, pending events are dispatched in **enqueue order** (oldest `emit` first).
2. Handlers for a single event run in registration order.
3. Events emitted during a handler are pushed to the **next frame's queue**, not dispatched until the next flush.
4. A flush runs after each `schedule` block or when `flush_events()` is called explicitly.

This prevents:
- Stack overflow from circular event chains
- Non-deterministic handler ordering
- Re-entrant handler bugs

`on ... once` handlers still follow declaration order. Each fires at most once for the program lifetime, except that `once` handlers **with** a guard are skipped only after a dispatch where the guard passed (see §9).

### 7.4 Mutability Rules

| Context | `let` | `let mut` |
|---|---|---|
| Reassignment | Error | OK |
| Field write | Error | OK |
| Index write | Error | OK |

| Context | System param (no `mut`) | System param (`mut`) |
|---|---|---|
| Field read | OK | OK |
| Field write | Error | OK |

| Context | Event handler param |
|---|---|
| Field read | OK |
| Field write | Error |

### 7.5 Async/Await

Rad supports cooperative async tasks:

- `async fn` declares an async function.
- `async on Event(...) { ... }` declares an async event handler.
- `async call(args)` spawns a task.
- `await task` waits for task completion and yields its inner value.

Example:

```rad
async fn fetch_name(id: int) -> str {
    return await http_get("https://example.com/user/" + str(id))
}

let t = async fetch_name(42)
let name = await t
print(name)
```

Notes:

- `await` is rejected for non-task values.
- `await` is rejected inside pipeline chains (`|>`).
- In async context, blocking I/O builtins (`http_get`, `read_file`, `write_file`, `input`, `readline`) are executed on an I/O thread pool and return tasks.

### 7.6 Task Errors

- Async task failures propagate at the `await` site.
- Awaiting a failed task raises a runtime error with task context.

### 7.7 Causal Settlements (experimental)

When `--experimental-laws` is enabled, a `settle` statement captures one
immutable base-world snapshot. Laws invoked by its body may read that snapshot
and create typed proposals. Proposals are grouped by intent and entity key and
canonically ordered by typed payload, not by producer invocation order.

Every proposed intent has exactly one same-module resolver. Each resolver reads
the original base snapshot and stages replacements in an isolated sparse patch
with `next`. Resolvers never observe proposals as world state or read any
candidate patch. Resolver declaration or execution order is unobservable and
cannot be configured.

After all resolvers finish, two candidate writes to the same `(entity,
component type)` are a settlement error. A conflict or any producer/resolver
failure discards all transient proposals and patches without changing the live
world or provenance ledger. A conflict-free patch is applied to a copy-on-write
world and adopted atomically.

After resolver conflict checks, constraints attached to staged components (or
their explicitly watched same-entity components) run once per constraint and
subject. Every constraint reads the original base through `base(subject,
Component)` and the complete candidate through `candidate(subject, Component)`.
It can report stable-code violations with `require condition else "code"`, but
cannot write, propose, emit, perform I/O, use nondeterminism, call native code,
or observe another constraint outcome. Reads of non-attached components require
an explicit `watches` declaration.

All selected invocations run under isolated deterministic fuel, heap, value,
and output limits. Their violations and evaluation failures are canonically
ordered. Zero outcomes permit the atomic commit; any outcome rejects the patch
without changing the live world or durable provenance. Constraints have no
ordering, priorities, projection, correction, or first-error semantics.

The complete v0 syntax, static restrictions, interoperability rules, and
non-goals are specified by [RFC-0001](https://github.com/peteracs/rad/blob/main/docs/rfcs/0001-causal-settlements.md)
and [RFC-0002](https://github.com/peteracs/rad/blob/main/docs/rfcs/0002-candidate-constraints.md),
and summarized in the [Causal Laws](../guide/causal-laws.md) and
[Candidate Constraints](../guide/candidate-constraints.md) guides.

---

## 8. Purity and Effects

### 8.1 Pure functions

```
pure fn <name>(<params>) {
    <body>
}
```

A function declared with `pure fn` is marked as **pure** for static analysis. The checker also performs conservative purity inference for unannotated functions; functions proven effect-free are treated as pure for pipeline validation.

When purity inference fails for a function used in a pipeline, the compiler traces the exact call chain to find the source of the impurity (e.g., an impure builtin like `set` or `print`, or an event emission). It then reports the full chain in the error message and suggests which functions need to be explicitly annotated with `pure fn`.

### 8.2 Readonly functions

```
readonly fn <name>(<params>) {
    <body>
}
```

A function declared with `readonly fn` may perform ECS **read** operations (`get`, `has`, `entities`, `query_where`, `query_map`, `query_count`, `with_field`, `peek`, `lookup`) but must not perform world-mutating operations (`set`, `spawn`, `remove`, `despawn`, `commit`), I/O, or event emission. The `readonly` effect is distinct from `ecs` (which permits both reads and writes).

`readonly` functions are allowed inside pipeline expressions, alongside `pure` functions. This enables a common pattern where pipeline stages need to look up ECS data:

```
readonly fn enemy_hp(id: entity) -> int {
    return require(id, Health).hp
}

let weakest = entities(Health)
    |> filter(fn(id) { return enemy_hp(id) < 50 })
```

### 8.3 Effect levels

Rad uses a lightweight effect system to classify function side effects:

| Effect | Keyword | What it permits |
|--------|---------|-----------------|
| (none) | `pure fn` | No side effects — only local computation |
| `readonly` | `readonly fn` | ECS reads (`get`, `has`, `entities`, `query_*`, `peek`, `lookup`) |
| `ecs` | `ecs fn` | ECS reads **and** writes (`set`, `spawn`, `remove`, `despawn`, `commit`, `fork`) |
| `io` | `io fn` | I/O operations (`print`, `read_file`, `http_get`, etc.) |
| `event` | `event fn` | Event operations (`emit`, `flush_events`) |

Effects can be combined: `io ecs fn` permits both I/O and ECS operations.

**Function types carry a purity rank.** A fn type annotation may be written
`pure fn(...) -> T`, `readonly fn(...) -> T`, or bare `fn(...) -> T`. The
ranks order `pure < readonly < impure` (a bare `fn(...)` promises nothing),
and an argument must rank at most as effectful as the parameter: a pure
function value is accepted everywhere, a readonly value satisfies readonly or
bare parameters, and an unannotated/impure value satisfies only bare ones.
Named `pure fn`s and `readonly fn`s used as values, closures the checker can
vouch for, and the readonly read builtins (`res`, `get`, `get_resource`, …)
carry their real rank.

**Function-typed parameters of effect-annotated functions are promoted.**
Inside an effect-restricted body the annotation is the only contract the
checker can trust, so a BARE `fn(...)` parameter of an explicitly
effect-annotated function is promoted to the strongest callback type its row
can call: `readonly fn(...)` when the row includes the `readonly` effect,
`pure fn(...)` otherwise. Callers must then pass a conforming function, and
in exchange the body may call the parameter without violating its effect row.
Explicit `pure fn(...)` / `readonly fn(...)` annotations are left as written,
and parameters of unannotated functions are never promoted.

```
pure fn apply(f: fn(int) -> int, v: int) -> int {
    return f(v)          // allowed: `f` is a pure fn type here
}

readonly fn scan(pred: readonly fn(entity) -> bool) -> list<entity> {
    return query_where(Hero, pred)   // readonly callback, readonly context
}

let a = apply(fn(x: int) -> int { return x * 2 }, 21)  // ok: pure closure
let b = apply(writes_a_resource, 21)                   // error: impure argument
```

**Unverifiable or under-ranked callees are not callable in restricted
contexts.** A pure function value is callable anywhere; a readonly value
requires a context that allows the `readonly` effect; anything else — an
impure `fn(...)` value, or a callee typed `any` — is treated exactly like
calling a named function that requires unrestricted effects, and is an
effect violation in any restricted context. Module-qualified calls
(`alias.helper(...)`) are checked against the callee's declared effect row
like any other named call.

### 8.4 Pipeline restrictions (`|>`)

The pipeline operator evaluates its left-hand side, then evaluates the right-hand side in a **pipeline context** where stricter rules apply (enforced by the static checker):

- World-mutating builtins (**`set`**, **`spawn`**, `set_resource`, and the fork/simulate/persistence write family) and **IO builtins** (`print`, `log`, `sleep_ms`, file and network access) are not allowed inside a pipeline — neither as a direct stage (`x |> print`) nor inside a callback. **`emit`** is likewise banned.
- Calls to user-defined functions that are not known pure or `readonly` are not allowed on the pipeline RHS (when the callee is resolved as a named function).
- ECS **read** builtins (`get`, `has`, `entities`, `query_where`, `query_map`, `query_count`, `with_field`, `peek`, `lookup`) are classified as `readonly` and are permitted in pipelines.
- **Assignment** to variables that are not introduced inside the pipeline-evaluated code (outer assignments) is rejected — pipelines must not mutate enclosing state through assignments.

These rules keep pipeline chains referentially transparent (modulo ECS reads, which are observationally stable within a single pipeline evaluation) and safe to reorder or optimize in future versions.

---

## 9. One-Shot Event Handlers

### 9.1 `once` handlers

```
on <EventName> once (<param>) {
    <body>
}
```

For each `once` handler declaration:

- **Without a guard:** the handler body runs **at most one time** — the first time the event is dispatched to it. Later emissions skip it.
- **With `where` / `when`:** the handler is retired only after a dispatch where the guard is truthy and the body runs. Emissions where the guard is false do **not** consume the `once` slot; the handler can run on a later emission when the guard passes.

Later emissions of the same event type skip handlers that are already fired.

Nested event dispatch (e.g. a handler that causes `flush_events` while another handler is running) restores this bookkeeping so an outer guarded `once` handler is not marked fired solely because an inner dispatch set the guard flag.

### 9.2 Normal handlers

Normal handlers (`on` without `once`) run for **every** emission, in registration order, as described in §7.3.

`once` handlers share the same dispatch order but are skipped after they are fired (including guarded `once` handlers only after a successful guarded run).

---

## 10. Error Handling

Rad does not have exceptions or try/catch. The compiler is designed for a robust Developer Experience (DX) and uses **error recovery** to report multiple syntax and type errors in a single run, rather than bailing on the first error.

Errors are reported with:

1. The exact line and column number
2. The source line with a caret pointing to the error
3. A plain-English explanation
4. A suggested fix (when applicable)

```
  Error: Cannot reassign 'x' — declared with 'let' (immutable)
   help: use 'let mut x = ...' for a mutable binding

  --> path/to/file.rad:12:5
   |
11 |     let x = 1
12 |     x = 2
   |     ^
13 | }
```

### 10.1 Parser Error Recovery

The Rad parser implements synchronization strategies to recover from syntax errors. When the parser encounters malformed code (e.g., a missing brace or unexpected token), it will:
1. Record the syntax error.
2. Skip tokens until it finds a safe synchronization point (like the start of the next statement or top-level declaration).
3. Continue parsing the rest of the file.

This allows the compiler to build a partial Abstract Syntax Tree (AST) containing `Error` nodes, which enables the Type Checker to run and find semantic errors even when the syntax is not perfect. You will see all syntax and type errors across your entire project in one go.
  --> game.rad:24:5
   |
     23 |     let x = 10
>>   24 |     x = 20
              ^
     25 | }
```

---

## 11. Shipped Since Initial Spec

The following items from earlier drafts are now implemented:

- **Static type system** — compile-time type checking with gradual typing, generic functions, type aliases, and sum types (see §2, §3)
- **Module system** — `use` imports with recursive loading, cycle detection, duplicate symbol errors, source maps, and lockfile support (see §3.9)
- **Import aliasing** — `use "path" as name` for scoped module access and collision avoidance (see §3.10)
- **FFI / native plugins** — C-ABI plugin interface via `rad_extension_init`, with value marshalling and dynamic library loading (see `ffi.rs`)
- **v0.5 DX improvements** — zero-field variant shorthand, match rest bindings, implicit tail return, improved diagnostics, and compat mode

## 12. Future Additions

- **Package registry** — `rad install`, `rad publish`, `rad.toml` `[dependencies]` section (struct scaffolding exists, no resolution or fetching logic yet)
- **Module exports** — `pub fn`, `pub component` visibility control
- **Standard library** — `std/collections`, `std/math`, `std/text` as distributable RAD modules
- **AOT compilation** — compile RAD to native binaries via LLVM or Cranelift
