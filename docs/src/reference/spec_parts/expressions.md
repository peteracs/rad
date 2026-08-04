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
