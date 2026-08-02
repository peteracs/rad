# Type Annotations

Rad uses gradual typing. Annotations are optional — the checker validates them when present and infers types otherwise.

## Function signatures

```
fn add(a: int, b: int) -> int {
    return a + b
}
```

The return type annotation (`-> int`) is optional. If omitted, the checker infers it.

## Variable annotations

```
let _total: int = add(1, 2)
let _name: str = "Rad"
let _ratio: float = 3.14
```

## String Literals

Rad supports standard string literals with double quotes, and line-based multi-line string literals using double backslashes (`\\`).

```
let single = "Hello, world!"

let multi = \\This is a multi-line string.
            \\It ignores indentation before the backslashes.
            \\Quotes like "this" don't need to be escaped!
```

### Escape sequences

All string types (plain, f-string, triple-quoted) support the same escape sequences:

| Escape | Character |
|--------|-----------|
| `\n`   | newline   |
| `\t`   | tab       |
| `\r`   | carriage return |
| `\\`   | literal backslash |
| `\"`   | literal double quote |
| `\0`   | null byte |

F-strings additionally support `\$`, `\{`, and `\}` to produce literal `$`, `{`, `}`.
Unknown escape sequences (e.g. `\a`) pass through as-is (backslash + character).

### F-strings and interpolation

Prefix a string with `f` to enable interpolation with `{expr}` or `${expr}`:

```
let name = "Rad"
print(f"Hello, {name}!")       // Hello, Rad!
print(f"Hello, ${name}!")      // Hello, Rad! (equivalent)
```

Regular (non-f) strings also support `${expr}` interpolation:

```
print("Hello, ${name}!")       // Hello, Rad!
```

### Triple-quoted f-strings

`f"""..."""` is a multi-line f-string designed for embedding code, JSON, or templates
where braces appear frequently.

**Important:** In triple-quoted f-strings, only `${expr}` interpolates. Bare `{` and `}`
are treated as literal text — no escaping needed.

| Syntax        | `{x}` means | `${x}` means | Bare `{` / `}` |
|---------------|-------------|--------------|-----------------|
| `f"..."`      | interpolation | interpolation | must escape (`{{`/`}}` or `\{`/`\}`) |
| `f"""..."""`  | literal text  | interpolation | literal text (no escaping needed) |

```
let n = 3
let code = f"""
    if (argc != ${n}) {
        fprintf(stderr, "error\n");
        exit(1);
    }
"""
```

## Available types

| Type | Examples | Notes |
|---|---|---|
| `int` | `0`, `42`, `-7` | Signed integer (`i64` range). Typical values are stored inline in the VM’s NaN-boxed word; very large magnitudes use a heap representation (see `core/vm/src/value.rs`). |
| `float` | `3.14`, `0.0`, `-1.5` | 64-bit IEEE 754 |
| `str` | `"hello"`, `""` | UTF-8 string. Indexing `s[i]` returns an integer byte. |
| `bool` | `true`, `false` | |
| `list` | `[1, 2, 3]` | Contiguous `Vec` with `Arc` copy-on-write |
| `tuple` | `(1, "hello")` | |
| `map` | `{ "key": "value" }` | Persistent HAMT |
| `fn` | `fn(int) -> str` | |
| `nil` | `nil` | |
| `any` | Top type, compatible with all types | |
| `entity` | Entity ID | |
| `bitset` | `bitset_new()` | O(1) integer membership set |
| `task` | Async task handle | |

## Tuples

Tuples are fixed-size ordered sequences of typed values. They are useful for grouping heterogeneous data without defining a struct.

```
let _pair: (int, str) = (42, "hello")
let _empty: () = ()
let _single: (int,) = (1,) // Note the trailing comma for single-element tuples
```

Tuples can be indexed using bracket notation (`pair[0]`), unpacked into function arguments using the spread operator (`..pair`), and destructured into multiple variables:

```
let (a, b) = (42, "hello")
print(a) // 42
```

*Note on Mutability:* When using `let mut (a, b) = ...`, the `mut` keyword applies to *all* bindings in the tuple. Granular mutability like `let (mut a, b) = ...` is not currently supported.

Closure parameters and for-loop bindings also support bracket destructuring for lists and tuples. This is especially useful in pipelines:

```
let rows = [(1, "a"), (2, "b")]
let names = rows |> map(fn([id, name]) { return name })

for [idx, val] in enumerate(items) {
    print(f"{idx}: {val}")
}
```

See [Pipelines — Destructuring in pipeline callbacks](./pipelines.md#destructuring-in-pipeline-callbacks) for details.

## Tuples as map keys

Tuples of valid key types (`int`, `str`, `bool`, `entity`, nested tuples) can key maps — coordinates index maps directly. Keys hash by value, iteration order is deterministic (lexicographic), and floats stay banned inside tuple keys:

```
let mut cost = {}
cost[(4, 2)] = 6
print(cost[(4, 2)])              // 6
let walls = { (1, 1): true }
```

## Maps with mixed value types

Map literals with different value types are inferred as `map<str, any>`:

```
let _record = {"name": "Alice", "age": 30, "active": true}
```

All map operations (`keys`, `values`, `entries`, `merge`, indexing) work naturally on these maps.

## Unique bindings

The `unique` keyword can be combined with `let` to enforce single-ownership at compile time:

```
let unique data = expensive_computation()
// data cannot be aliased — no let y = data, no passing to functions, no closure capture
```

This is primarily a performance tool: it guarantees that `Arc`-backed containers are never shared, so mutations are always in-place. See [Value Semantics](./value-semantics.md) for details.

## When to annotate

Annotations are optional for local variables and private functions, but they are **required for public exports**. Any declaration marked with `pub` (e.g., `pub fn`, `pub component`, `pub resource`) must have explicit type annotations. This ensures that the public API of a module is always strictly typed.

For local variables, the checker's inference handles most cases without annotation.

```
fn distance(x1: float, y1: float, x2: float, y2: float) -> float {
    let dx = x2 - x1
    let dy = y2 - y1
    return (dx * dx + dy * dy)
}
```

Here, `dx` and `dy` are inferred as `float` from their expressions. The parameter and return types are annotated for clarity.
