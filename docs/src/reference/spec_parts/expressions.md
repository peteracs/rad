<a id="expressions"></a>
## 5. Expressions

Expressions produce values. Unless a rule below says otherwise, operands are
evaluated from left to right exactly once. The precedence table in
[Lexical structure](#operators-and-precedence) determines grouping.

<a id="expr-literals"></a>
### 5.1 Literals and aggregate values

```text
42
3.5
true
false
nil
"zone-a"
[1, 2, 3]
{"north": 1, "south": 2}
(7, "ready")
```

List elements have one compatible element type. Map keys satisfy the map-key
rules in the type system and values have one compatible value type. Parentheses
with a comma form tuples; parentheses without a comma only group an expression.

Formatted strings use `f"..."`. `${expression}` is the canonical interpolation
form. Single-line formatted strings also accept `{expression}`. Triple-quoted
formatted strings use `${expression}` exclusively, so ordinary braces remain
literal. Formatting evaluates each interpolation once in source order.

<a id="expr-names-access"></a>
### 5.2 Names, fields, and indexes

```text
current
packet.temperature
rows[index]
matrix[row][column]
module.public_name
```

Names resolve lexically, then through the current canonical module graph.
Fields require a declared record, component, resource, event, state, or sum
shape. Indexing supports lists, maps, strings, byte buffers, and APIs whose
declared type defines indexing. An invalid index or missing map key is a runtime
error unless the API explicitly returns `Option` or `Result`.

<a id="expr-operators"></a>
### 5.3 Unary and binary operators

RAD has three unary operations:

```text
-number
not condition
!condition
~mask
```

`not` and `!` are the same logical operation. Binary operators are:

```text
* / %
+ -
<< >>
& ^ |
< <= > >=
== != is
and or
|>
```

Arithmetic is checked against the operand's numeric representation. `/` and
`%` reject a zero divisor. Shifts reject an invalid count. Logical `and` and
`or` short-circuit. `is` checks a state/sum variant without extracting fields.
All binary levels associate left-to-right.

At expression level `<<` is a shift. The statement parser recognizes a
top-level `container << value` as collection append; nesting it in another
expression retains shift semantics.

<a id="expr-call"></a>
### 5.4 Calls, system references, and spread arguments

```text
dispatch(asset, mission)
module.dispatch(asset, mission)
system::AdvanceGrid
system::operations::AdvanceGrid
sum_three(..(1, 2, 3))
```

Arguments are evaluated left-to-right. Arity, generic substitution, parameter
types, callback purity, and inferred effects are checked before lowering.
`..tuple_or_list` in argument position expands its elements in order. A
qualified `system::` expression is a statically resolved system value; it is
not a dynamically looked-up string.

<a id="expr-pipeline"></a>
### 5.5 Pipelines

```text
records |> filter(fn(row) { return row.active }) |> len
value |> clamp(0, 100)
records |> fn(rows) { return len(rows) }
records |> .priority
```

`left |> function` calls `function(left)`. A call on the right inserts `left`
as its first argument. A closure consumes `left` as its first parameter. A
field projector maps a list to that field. Pipeline lowering preserves the
same type, authority, callback, allocation, and error rules as the equivalent
ordinary calls; it cannot conceal an effect.

<a id="expr-function"></a>
### 5.6 Function expressions and capture

```text
fn(value: int) -> int { return value + 1 }
fn(mut total: int) -> int { total = total + 1 return total }
fn([index, value]: (int, int)) -> int { return index + value }
```

Parameters may be annotated, mutable, or list-destructured. Return annotations
are optional for private inferred closures and required where strict boundaries
demand them. Closures capture lexical bindings. Mutable capture writes back to
the captured binding. A recursive closure requires `let rec`; ordinary
self-reference in an initializer is rejected. Capture and callback effects are
included transitively in system authority and query-cost analysis.

<a id="expr-records"></a>
### 5.7 Records, variants, states, and entities

```text
Position { x: 10, y: 20 }
Position { x: 11, ..old_position }
Decision::Review { reason: "high-value" }
MissionState::Active
entity "asset-17" {
    Identity { id: AssetId(u64(17)) }
    Position { x: 10, y: 20 }
}
```

Component/resource/struct construction must initialize every required field
exactly once. `..base` copies unspecified fields from a compatible value and
may appear once. Sum variants and state references are nominal and qualified.
An entity literal creates one entity with an optional unique name and the
listed components; duplicate component types or names are errors.

<a id="expr-query"></a>
### 5.8 Query expressions

```text
query { Position, Velocity }
query { mut Position, Velocity }
query { Position } where not Destroyed
query { Position, Health } where Health.current > 0
query { Position, Health } select Position, Health
```

The query takes a stable entity-membership snapshot in ascending entity-ID
order. Included components are required. A top-level `not Component` conjunct
in `where` is lowered to an exclusion query, so that component must be absent.
`mut` produces writeback-capable component bindings when the query is consumed
by a loop. `where` is evaluated per row after component binding. `select`
returns selected component values instead of entity IDs/row tuples.

Query syntax never implies an index. Whole-population queries are `O(n)` and
are visible to transitive cost contracts. Named materialized views and indexed
builtins are the explicit bounded alternatives.

<a id="expr-branching"></a>
### 5.9 If and match expressions

```text
let label = if ready { "ready" } else { "waiting" }

let value = match result {
    Ok { value } => { value }
    Err { error } => { panic(error) }
}
```

An `if` expression requires an `else`; all branches must produce compatible
types. A match expression uses the same pattern, guard, ordering, binding, and
exhaustiveness rules as a match statement, but every reachable arm must produce
a compatible value.

<a id="patterns"></a>
### 5.10 Patterns

Canonical source patterns are:

```text
_                                  // wildcard
0                                  // primitive literal
Decision::Allow {}                 // qualified variant
Review { reason }                  // variant shorthand in known context
Review { reason: message }         // explicit binding name
has Position(component)            // component-presence pattern
```

Variant fields support shorthand and `field: binding`; nested paths are
destructured recursively. Guards use `when expression`, run only after the pattern matches, and
may refer to its bindings. Arms are tested top-to-bottom. A guarded arm does not
make later arms unreachable and cannot alone establish exhaustiveness.

<a id="expr-async"></a>
### 5.11 Async calls and await

```text
let pending: task<int> = async compute_score(input)
let score: int = await pending
```

`async` must be followed by a call and returns `task<T>`. `await` consumes a
task and yields its result or propagates its task failure. Async reachability is
part of inferred authority; a system requires `async true` even when the call
is hidden behind helpers or callbacks.

<a id="expr-try"></a>
### 5.12 Try propagation

Postfix `?` unwraps `Option<T>` or `Result<T, E>`. `None` or `Err` returns early
from the current function, whose return type must admit the propagated value.
The success payload becomes the expression result. The operator does not catch
runtime panics or host failures that are not represented by the sum value.

`Expr::Error` is a parser-recovery node and has no source spelling.
