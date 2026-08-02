# Language Guarantees

This page is the single source of truth for the behavioral contracts Rad upholds.
If your program compiles and runs, every guarantee on this page holds.

---

## Maturity Labels

This reference uses three labels to separate hard guarantees from evolving ergonomics:

| Label | Meaning |
|------|---------|
| `stable` | Expected to remain behaviorally compatible; changes require changelog callouts and conformance updates |
| `compat` | Behavior gated by explicit compatibility flags (`--compat-v0.5-dx`) |
| `experimental` | Usability/documentation areas that may evolve quickly (for example, wording of diagnostics and guidance) |

Unless explicitly marked otherwise, sections below describe `stable` contracts.

---

## 1. Immutability by Default

All bindings are immutable unless explicitly declared `mut`.

```rad,ignore
let x = 10
x = 20        // ← compile error: x is not mutable

let mut y = 10
y = 20        // ← ok
```

Containers (lists, maps, components) follow the same rule.
Writing to an element of an immutable container is a compile-time error:

```rad,ignore
let xs = [1, 2, 3]
xs[0] = 99    // ← compile error
```

### What this means for you

- You can trust that any `let` binding will never change after initialization.
- Reading a binding in a different scope always yields the original value.
- No function can mutate a value you pass to it (see Value Semantics below).

---

## 2. Value Semantics

Every assignment, function argument pass, and return produces an **independent copy**.
Two variables never alias the same mutable state.

```
let mut a = [1, 2, 3]
let mut b = a          // b is a copy
b[0] = 99
print(a[0])            // 1 — a is unchanged
```

This applies uniformly to:

| Type | Copy behavior |
|------|---------------|
| `int`, `float`, `bool`, `str`, `nil` | Primitive values — always independent |
| Lists | Logical independent values (see note below) |
| Maps | Deep copy of key-value pairs |
| Components | Deep copy of fields |

**Lists (implementation):** User-visible semantics stay value-based, but the VM stores list payloads as `Arc<Vec<Value>>` with copy-on-write: assigning a list copies the handle; mutating may clone the backing vector when it is still shared. **Strings** use `Arc<str>` internally; copying a string across the air gap between persistent ECS storage and the execution stack is O(1).

**ECS reads (air-gap guarantee):** `get(entity, Component)` and `peek(fork, entity, Component)` always produce an independent copy of the component's fields, deep-copied from persistent ECS storage into the caller's heap. This guarantees that subsequent `despawn`, `commit`, or `set` operations on the entity cannot invalidate previously read values.

### Transforms return new values

All builtin list/map/string operations are non-destructive.
They take a value and **return a new value** — the input is never modified.

```
let xs = [3, 1, 2]
let sorted = sort(xs)
print(xs)       // [3, 1, 2] — unchanged
print(sorted)   // [1, 2, 3] — new list
```

To keep the result, reassign explicitly:

```
let mut xs2 = [3, 1, 2]
xs2 = sort(xs2)           // xs2 is now [1, 2, 3]
```

This holds for every builtin that transforms data: `push`, `pop`, `sort`, `sort_by`, `reverse`, `slice`, `map`, `filter`, `reduce`, `flat_map`, `find`, `max_by`, `min_by`, `enumerate`, `append`, `merge`, `replace`, `trim`, `split`, `join`, `chr`, `ord`, `chars`, `to_upper`, `to_lower`, `values`, and others.

---

## 3. Purity in Pipelines

Pipeline expressions (`|>`) enforce purity or readonly semantics.
Only pure or `readonly` functions may appear in a pipeline — side-effecting builtins are rejected at compile time:

```rad,ignore
// ✓ All stages are pure
let result = [1, 2, 3]
    |> filter(fn(x) { return x > 1 })
    |> map(fn(x) { return x * 2 })

// ✓ Readonly ECS reads are allowed in pipelines
readonly fn get_hp(e: entity) -> int { return require(e, Health).hp }
let hps = entities(Health) |> map(get_hp)

// ✗ Compile error: set() is a side-effecting builtin
[hero] |> map(fn(e) { set(e, Health { hp: 100 }) })
```

### Which builtins are side-effecting (forbidden in pipelines)?

| Builtin | Reason |
|---------|--------|
| `set` | Mutates world state (ECS) |
| `spawn` | Creates an entity |
| `remove` | Removes a component |
| `despawn` | Destroys an entity, clears its components, and recycles its ID |
| `set_resource` | Mutates a global resource |
| `flush_events` | Runs pending event handlers for the current flush phase |
| `transition` | Mutates state machine state |
| `print` | Side effect (I/O) — the builtin signature is not `pure`, but the pipeline checker does not block `print` the way it blocks ECS mutators (`set`, `spawn`, …), so `print` can appear in pipeline position for quick debugging |

### Which builtins are `readonly` (allowed in pipelines)?

| Builtin | What it reads |
|---------|---------------|
| `get` | Component value from entity |
| `has` | Component existence check |
| `entities` | Entity ID list |
| `query_where` | Filtered entity list |
| `query_map` | Mapped entity data |
| `query_count` | Entity count |
| `with_field` | Field-filtered entity list |
| `peek` | Component from fork |
| `lookup` | Indexed field lookup |
| `get_resource` | Global resource value |

These builtins carry the `readonly` effect — they read ECS state but never mutate it. User-defined `readonly fn` functions (which only call `readonly` builtins) are also allowed in pipelines.

The **`emit` statement** (not a builtin) queues an event for the next flush; it is a side effect and forbidden inside pipelines for the same reason as `set` / `spawn`.

All other builtins (`map`, `filter`, `sort`, `sort_by`, `find`, `max_by`, `min_by`, `enumerate`, `push`, `keys`, `values`, `format`, `chr`, `ord`, `chars`, `to_upper`, `to_lower`, `unwrap_or`, `is_some`, `is_none`, etc.) are **pure**: they compute a result from their inputs and nothing else.

---

## 4. Standard Library Semantics

### Consistent argument conventions

All collection builtins take the **collection as the first argument**, making them pipeline-friendly:

```
[1, 2, 3] |> push(4) |> sort |> reverse
```

| Pattern | Examples |
|---------|----------|
| `fn(collection, ...)` | `push(list, val)`, `contains(col, val)`, `slice(list, start, end)` |
| `fn(collection, callback)` | `map(list, fn)`, `filter(list, fn)`, `reduce(list, init, fn)` |
| `fn(string, ...)` | `split(str, sep)`, `replace(str, old, new)`, `starts_with(str, prefix)`, `chars(str)`, `to_upper(str)` |

### Return type contracts

| Builtin | Returns |
|---------|---------|
| `push(list, val)` | New list with `val` appended |
| `pop(list)` | Last element — errors on empty list (same as `pop_last`) |
| `pop_last(list)` | Last element — errors on empty list |
| `drop_last(list)` | Remaining list only — errors on empty list |
| `sort(list)` | New sorted list |
| `sort_by(list, key_fn)` | New list sorted by extracted keys |
| `find(list, fn)` | `Some(element)` or `None` |
| `max_by(list, fn)` | `Some(element)` with largest key, or `None` |
| `min_by(list, fn)` | `Some(element)` with smallest key, or `None` |
| `enumerate(list)` | New list of `[index, element]` pairs |
| `reverse(list_or_str)` | New reversed list or string |
| `slice(list_or_str, start, end)` | New sub-list or sub-string |
| `map(list, fn)` | New list of mapped values |
| `filter(list, fn)` | New list of kept values |
| `reduce(list, init, fn)` | Single accumulated value |
| `range(end)` / `range(start, end)` / `range(start, end, step)` | List of integers |
| `abs(n)` | Same numeric type as input (`int` → `int`, `float` → `float`) |
| `min(a, b)` / `max(a, b)` | Preserves type when both args match; promotes to `float` when mixed |
| `a / b` | Integer division when both operands are `int` (truncates toward zero); float division otherwise |
| `get(entity, Component)` | `Some(value)` or `None` — never bare `nil` |
| `transition(state, event)` | `Ok(value)` or `Err(message)` — never bare `nil` |
| `unwrap(option_or_result)` | Inner value, or runtime error on `None` / `Err` |
| `expect(option_or_result, msg)` | Inner value, or runtime error with custom message on `None` / `Err` |
| `try_int(val)` / `try_float(val)` | `Some(value)` on success, `None` on failure — never errors |
| `contains(col, val)` | `bool` — works on lists, strings, and maps |
| `keys(val)` | `list` of keys (sorted deterministically) — works on maps, components, and sum types |
| `values(map)` | `list` of values (sorted deterministically by key) |
| `chr(int)` | Single-character string from Unicode code point |
| `ord(str)` | Unicode code point (`int`) of first character |
| `chars(str)` | `list` of individual character strings |
| `to_upper(str)` / `to_lower(str)` | New string with case converted |

### Frozen builtin contracts (`stable`)

The contracts below are frozen for current major versions. Changes require explicit migration pathing (compat flags or major version bump), plus conformance and docs updates in the same change.

| Builtin | Maturity | Frozen contract |
|---------|----------|-----------------|
| `pop(list)` | `stable` | Returns the last element; `pop([])` must error |
| `pop_last(list)` | `stable` | Returns the last element; `pop_last([])` must error |
| `drop_last(list)` | `stable` | Returns list without its last element; `drop_last([])` must error |
| `reduce(list, init, fn)` | `stable` | Argument order is fixed as `(list, init, fn)` |

### Error behavior

Builtins never silently swallow errors. When a builtin cannot fulfill its contract, it produces a **runtime error** with a descriptive message:

| Situation | Error |
|-----------|-------|
| `pop([])` | `pop() on empty list` |
| `pop_last([])` | `pop_last() on empty list` |
| `drop_last([])` | `drop_last() on empty list` |
| `unwrap(Option::None)` | `unwrap() called on Option::None` |
| `unwrap(Result::Err)` | `unwrap() called on Result::Err: <message>` |
| `int("abc")` | `Cannot convert 'abc' to int` |
| `sort([1, "a"])` | `sort() cannot compare int and str` |
| `range(0, 10, 0)` | `range() step cannot be zero` |

For fallible conversions without errors, use `try_int` / `try_float` which return `Option` instead.

---

## 5. No Hidden Global State

Systems and event handlers interact with the world exclusively through the ECS API (`get`, `set`, `spawn`, `despawn`, `has`, `remove`, `entities`, `get_resource`, `set_resource`).
There is no mutable global variable or implicit shared state between systems. Global singletons are declared explicitly with the `resource` keyword and are tracked by the scheduler for conflict analysis.

Two systems that don't share component or resource types cannot interfere with each other.

---

## 6. Simulation Purity

Systems used inside `simulate()` are statically checked for side-effect safety. The compiler rejects any system that:

- Calls an IO builtin (`print`, `read_file`, `http_get`, etc.)
- Calls `commit()` (would corrupt the forked world)
- Calls a user function with IO or Event effects
- Calls unsafe event-effect builtins such as `transition`
- Emits an event whose transitive handler chain performs IO, calls `commit()`, or calls unsafe effectful functions

This guarantee ensures that **simulated futures cannot have observable side effects** on the real world. The forked world is completely isolated: events emitted during simulation dispatch on the fork's own event queue, no IO executes, and no state leaks back unless explicitly committed.

At runtime, nested `simulate()` calls save and restore the active event queues so recursive speculation cannot leak events into the live timeline.

---

## 7. Unique Binding Guarantee

`let unique` provides a compile-time single-ownership guarantee. When a binding is declared `unique`, the checker statically ensures it is never aliased:

- Cannot be assigned to another variable (`let y = x` is rejected)
- Cannot be passed as a function argument (`f(x)` is rejected)
- Cannot be captured by a closure (`fn() { ... x ... }` is rejected)
- **Can** be reassigned to the same name (`x = transform(x)` is allowed)

This guarantees that the value's `Arc` reference count never exceeds 1, so mutations via `push`, element assignment, or `extend` are always in-place — no hidden O(n) deep clones.

```rad,ignore
let unique mut xs = [1, 2, 3]
xs << 4           // guaranteed in-place
xs = sort(xs)     // OK — reassignment to same name

let ys = xs       // ← compile error: cannot alias unique binding 'xs'
```

Use `--profile-copies` to verify that `unique` eliminates all deep clones in practice.

---

## 8. Ghost effects (observational intrinsics)

Some builtins are treated as **pure** in the typechecker so they do not poison purity analysis, but they may still produce **observational** effects at runtime (for example logging to stderr). The canonical example is `debug_trace(value)`.

**Contract:** Ghost effects do not alter program state transitions or the values returned by pure code paths. The implementation reserves the right to **reorder, duplicate, or elide** ghost-effect calls entirely. In particular:

- The bytecode compiler may compile `debug_trace(x)` as a no-op when building with `--release` (the argument is still evaluated).
- The frozen historical C backend had a similar `RAD_RELEASE` macro path, but
  it is not part of the current runtime contract.

Do not rely on ghost effects for security boundaries, audit logs that must be complete, or optimization correctness beyond “the program’s dataflow result is unchanged when they are stripped.”

---

## 9. Component field type stability

A component or resource field declared `float` always stores a `float`, even
when the supplied value's runtime tag is `int`. The compiler coerces the value
at every construction and `update` site.

```rad,ignore
component Position { x: float = 0.0, y: float = 0.0 }

// `0` is an int literal, but the field is declared float:
update(avatar, Position) { x = 12.34, y = 0 }
// stored as { x: 12.34, y: 0.0 } — never { y: 0 }
```

**Why it matters:** values that cross an untyped boundary lose their float-ness.
The browser session decodes events from JSON, where a whole number (`y: 0`)
is indistinguishable from an int. Without coercion the field would hold an int,
strict float readers (e.g. the packed render buffer) would reject the whole
component, and a peer's snapshot (`0`) would diverge from the authority's
(`0.0`), breaking deterministic convergence. The coercion is `int -> float`
only (lossless); a `float -> int` write is lossy and surfaces as a real error.

---

## Summary

| Guarantee | Mechanism |
|-----------|-----------|
| Float fields stay float | `int` values are coerced to `float` at component construction/update; see §9 |
| Bindings don't change | `let` is immutable; `let mut` opts in explicitly |
| No aliasing bugs | Value semantics — assignment always copies |
| Transforms are non-destructive | All builtin operations return new values |
| Pipelines are pure/readonly | Side-effecting builtins rejected at compile time; `readonly` ECS reads allowed |
| Errors are explicit | Runtime errors with descriptive messages; `try_*` for safe conversion |
| No hidden state | ECS is the only channel for world mutation |
| Simulations are isolated | IO, `commit()`, unsafe event-effect calls, and unsafe handler chains are statically forbidden inside `simulate()` |
| Unique bindings are never aliased | `let unique` enforces single ownership at compile time; see §7 |
| Ghost effects are optional | Observational intrinsics may be elided in release builds; see §8 |
