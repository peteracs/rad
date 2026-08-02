# Pipelines

The pipeline operator `|>` chains transformations left-to-right. It takes the result of the left expression and passes it as the first argument to the function on the right.

## Basic usage

```
let result = [1, 2, 3, 4, 5]
    |> filter(fn(x) { return x > 2 })
    |> map(fn(x) { return x * 10 })

print(result)
```

Output: `[30, 40, 50]`

This is equivalent to `map(filter([1,2,3,4,5], ...), ...)`, but reads in the order data actually flows.

## Pipeline functions

Rad provides built-in functions designed for pipelines. All take the collection as the first argument:

| Function | Signature | Description |
|---|---|---|
| `map` | `map(list, fn)` | Transform each element |
| `filter` | `filter(list, fn)` | Keep elements where `fn` returns true |
| `reduce` | `reduce(list, init, fn)` | Fold list to a single value |
| `flat_map` | `flat_map(list, fn)` | Map then flatten (callback returns list) |
| `group_by` | `group_by(list, fn)` | Group elements by key function, return map |
| `sort` | `sort(list)` | Return a sorted copy |
| `reverse` | `reverse(list)` | Return a reversed copy |
| `slice` | `slice(list, start, end)` | Return a sub-list |
| `push` | `push(list, val)` | Return list with `val` appended |
| `pop` | `pop(list)` | Return the last element (same as `pop_last`) |
| `pop_last` | `pop_last(list)` | Return the last element |
| `drop_last` | `drop_last(list)` | Return only the remaining list |
| `append` | `append(list, list)` | Concatenate two lists |
| `zip` | `zip(list, list)` | Pair elements into `[[a, b], ...]` |
| `enumerate` | `enumerate(list)` | Return `[[0, a], [1, b], ...]` index-element pairs |
| `find` | `find(list, fn)` | First element where `fn` returns truthy, or `None` |
| `max_by` | `max_by(list, fn)` | Element with largest key from `fn`, or `None` |
| `min_by` | `min_by(list, fn)` | Element with smallest key from `fn`, or `None` |

### String functions

| Function | Signature | Description |
|---|---|---|
| `split` | `split(str, delim)` | Split string by delimiter |
| `join` | `join(list, sep)` | Join list into string with separator |
| `trim` | `trim(str)` | Strip leading/trailing whitespace |
| `replace` | `replace(str, old, new)` | Replace all occurrences |
| `chars` | `chars(str)` | Split into list of characters |
| `to_upper` | `to_upper(str)` | Convert to uppercase |
| `to_lower` | `to_lower(str)` | Convert to lowercase |
| `chr` | `chr(int)` | Code point to character |
| `ord` | `ord(str)` | Character to code point |

## Chaining with `?` and `unwrap`

For `Option` / `Result` from `get` or similar, postfix `?` propagates failure from the enclosing function. Use `unwrap` / `expect` only when you want a runtime error on failure:

```
component Health { hp: 100 }

fn main() -> nil {
    let hero = spawn()
    set(hero, Health { hp: 100 })

    let hp = get(hero, Health)?
    print(hp.hp)
}
```

`fn main() -> nil` is special-cased to allow `?` — propagation of `None`/`Err` exits cleanly. You can also use `-> any` for the same behavior.

If you use `unwrap` and `get` returns `None`, the program errors at that point. Use `expect` for a custom message:

```
let _hp2 = expect(get(hero, Health), "hero has no Health component")
```

## Pure and readonly functions

Mark functions as `pure` to guarantee they have no side effects:

```
pure fn double(x: int) -> int { return x * 2 }

let _result2 = [1, 2, 3] |> map(double)
```

Mark functions as `readonly` to indicate they read ECS state but don't mutate it:

```
readonly fn get_hp(e: entity) -> int {
    return require(e, Health).hp
}

let low_hp = entities(Health) |> filter(fn(e) { return get_hp(e) < 50 })
```

Both `pure` and `readonly` functions are allowed in pipelines. The compiler enforces that pipeline stages cannot call side-effecting builtins (`set`, `spawn`, `emit`) or user functions with those effects. ECS **read** builtins (`get`, `has`, `entities`, `query_where`, `query_map`, `query_count`, `with_field`, `peek`, `lookup`, `get_resource`) are classified as `readonly` and permitted in pipelines. (`print` is not blocked by the same path as `set`; see [Language Guarantees](../reference/guarantees.md) §3.)

If you forget to mark a function as `pure` or `readonly`, the compiler infers purity where it can; otherwise it traces the call chain and points at the annotations you need.

## Destructuring in pipeline callbacks

When pipeline data contains lists or tuples (e.g., from `query ... select`, `zip`, or `enumerate`), bracket destructuring lets you name each element instead of using positional `r[0]`/`r[1]` indexing:

```
let task_rows = [("build", 2, 0.75), ("test", 1, 0.0), ("deploy", 2, 0.50)]

let running = task_rows
    |> filter(fn([name, phase, progress]) { return phase == 2 })
    |> map(fn([name, phase, progress]) { return f"{name}: {progress}" })
```

This works with all pipeline functions — `map`, `filter`, `reduce`, `flat_map`, `find`, `max_by`, `min_by`. For `reduce`, both the accumulator and the element can be destructured:

```
let totals = task_rows |> reduce((0, 0.0), fn([sum_p, sum_pr], [name, phase, progress]) {
    return (sum_p + phase, sum_pr + progress)
})
```

Use underscore `_` to discard unused positions: `fn([_, _, progress])`.

Destructured closures pair naturally with `enumerate` and `zip`:

```
["a", "b", "c"] |> enumerate |> map(fn([idx, val]) { return f"{idx}: {val}" })
// ["0: a", "1: b", "2: c"]

let keys = ["x", "y"]
let vals = [10, 20]
zip(keys, vals) |> map(fn([k, v]) { return f"{k}={v}" })
// ["x=10", "y=20"]
```

> **Note:** Pipeline fusion (vectorization) is currently disabled for destructured closures. They fall back to the standard per-element call path, which is correct but does not benefit from the `VecBroadcast`/`VecFilter` optimization described below.

## Pipeline Fusion Optimization

Rad's compiler automatically optimizes chains of `map`, `filter`, and `flat_map` operations. When you write a pipeline like:

```
let _result3 = [1, 2, 3, 4, 5]
    |> filter(fn(x) { return x % 2 == 0 })
    |> map(fn(x) { return x * 10 })
```

Instead of allocating a temporary list for the result of `filter`, the compiler **lowers** the chain to a **single loop** over one vector slot: the fused body runs per element, and only the **final** list is retained. The VM uses dedicated vector ops (`VecBroadcast`, `VecFilter`, and related opcodes) so intermediate steps stay in registers/stack traffic rather than allocating full intermediate lists.

**Branches in the fused lambda:** If the mapped expression is a conditional (e.g. `if` / `else` with single-expression arms), the compiler emits `VecSelect`, which blends per-element results from the condition, then-branch, and else-branch lists—again without falling back to a non-fused interpreter loop.

Purity rules ([Language Guarantees](../reference/guarantees.md)) still apply: impure calls are rejected in fused pipelines.

## Operator precedence

The `|>` operator has the **lowest** precedence of all operators. Arithmetic, comparison, and logical operators bind tighter. This means post-pipeline arithmetic must use a temporary variable:

```
let scores = [100, 90, 80]
// Won't work as expected — the / is parsed as part of the pipeline RHS:
// scores |> reduce(0, fn(a, x) { return a + x }) / len(scores)

// Instead, bind the pipeline result first:
let total = scores |> reduce(0, fn(a, x) { return a + x })
let _avg = total / len(scores)
```
