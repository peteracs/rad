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
