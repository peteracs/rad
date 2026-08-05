# Enterprise Expectations

This guide is for teams coming from Rust, C++, Java, TypeScript, or Python who want to understand where RAD intentionally behaves differently.

## 1) Value Semantics, Not Aliasing

In RAD, assignment and argument passing always copy values (`list`, `map`, `str`, `bitset`, etc.). Mutating one binding never mutates another binding that was derived from it.

```rad
let mut a = [1, 2, 3]
let mut b = a
b[0] = 99
print(a[0]) // 1
```

*(Note: The compiler uses static escape analysis to optimize unique variables into $O(1)$ in-place mutations under the hood, but the language semantics are strictly value-based).*

## 2) Pipelines Enforce Purity (or Readonly)

Pipeline stages (`|>`) must be pure or `readonly`. Functions whose body only uses local variables, arithmetic, and pure builtins are automatically inferred as pure — no `pure fn` annotation needed. Functions that perform ECS reads (e.g., `get`, `has`, `entities`, `lookup`, `get_resource`) can be declared `readonly fn` and are also allowed in pipelines. If a function is used as a direct pipeline stage or callback and performs side effects (ECS mutation, non-pure/non-readonly calls, `print`, or other I/O), the checker rejects it ([Language Guarantees](../reference/guarantees.md) §3).

```rad
fn plus_one(x) { return x + 1 }
let out = [1, 2, 3] |> map(plus_one)

readonly fn hp(e: entity) -> int { return require(e, Health).hp }
let low = entities(Health) |> filter(fn(e) { return hp(e) < 50 })
```

## 3) Forward Function References Are Supported

Top-level functions can call later-declared functions in the same module. This includes mutual recursion.

## 4) Maps Use Deterministic Data Keys

Map keys can be `str`, `int`, `bool`, `entity`, or tuples made from those key types. Keys hash by value and iterate deterministically. Mixed value types are supported (`map<K, any>` behavior), which is convenient but less rigid than strict structural records.

For integer membership tests (bookmarks, visited sets, line flags), use `bitset_new()` / `bitset_has()` instead of `contains(list, val)`. BitSet provides O(1) lookup and ~1 bit per index, while list `contains` is O(n).

## 5) Components Are the “Record” Primitive

For stable schemas and typed fields, prefer components over ad-hoc maps.

## 6) Option/Result Are First-Class

`get(...)` returns `Option`, `transition(...)` returns `Result`, and `try_int`/`try_float` return `Option`. Use postfix `?` in functions that can propagate the same `Option`/`Result` (for example `fn main() -> any`), `match` for explicit control flow, or `unwrap_or(val, default)` for concise defaults. Check without unwrapping via `is_some(val)` / `is_none(val)`.

## 7) Commit `forge.lock` for production CI (modules)

If your program uses `use` with **remote HTTP(S) URLs**, production and CI must rely on a committed **`forge.lock`** next to the entry `.rad` file. The lockfile pins each module path (including URLs) with byte size, checksum, and **SHA-256** of file contents. Unpinned remote imports (no lockfile, or URL not listed with a sha256 line) are a **supply chain risk**: builds are not reproducible and content is not cryptographically pinned. Run `rad your_main.rad --write-lock` after your module graph is stable, then commit `forge.lock` and verify it in CI.

## 8) ECS Is the Mutation Boundary

World mutation happens via ECS operations (`set`, `spawn`, `remove`, `despawn`, `set_resource`). This keeps side effects explicit and auditable.

## 9) `let` Is Immutable by Default

Use `let mut` when rebinding is intended. Container element writes also require mutable bindings.

## 10) Builtins Are Non-Destructive Transforms

Collection/string operations return new values instead of mutating in place. Reassign intentionally when you want to keep transformed state. Note that string indexing `s[i]` returns an integer representing the byte value, not a 1-character string.

## 11) Diagnostics Are Part of the Workflow

RAD leans on checker diagnostics and hints to teach idioms (`pure fn`, type conversions, pattern exhaustiveness). Treat compile-time feedback as design guidance, not only error reporting.

## 12) Architecture Enforcement via Linter

The `rad lint` tool provides presets that enforce architectural patterns:
- **CQRS / Read-Write Separation**: The `enterprise` preset requires explicit effect keywords (`io`, `ecs`, `event`) before `fn` on functions that cause side effects (for example `io fn`, `ecs fn`, or combinations like `io ecs fn` as needed).
- **Strict Module Boundaries**: The `enterprise` preset requires aliased imports (`use "foo.rad" as foo`) and can enforce dependency DAGs via `--boundary` flags.
- **Observability**: The `enterprise` preset warns against bare `print()` calls, nudging teams toward structured `log()` and `metric()` builtins.
- **Declarative Code**: The linter warns when imperative loops are used solely to build collections, suggesting pipeline equivalents.
- **Scheduler-Visible System Contracts**: The `enterprise` and `strict` presets flag system bodies that directly read or write component/resource types missing from the system's signature (`RAD-L015`/`RAD-L016`) — such accesses are invisible to the scheduler's parallel conflict analysis. Direct accesses only: helper functions the system calls are not analyzed.

## 13) Compatibility Features Are Flag-Gated

Some syntax/ergonomic behaviors are controlled by compatibility flags (for example `--compat-v0.5-dx`). Keep CI flags explicit to avoid environment drift.

## 14) Historical C Backend String Interning

The frozen C backend (`emit_c.rad`) historically interned all strings at runtime via an FNV-1a hash table. This meant:
- String equality is a pointer comparison in the common case (O(1) instead of O(n) `memcmp`)
- Duplicate substrings extracted during lexing or parsing allocate only once
- Keyword checks and map key comparisons hit the fast path automatically

This section is historical. `core/vm` is the ground-truth runtime.

This is transparent to user code — no API changes are needed to benefit from interning.

## 15) Indexed Fields for O(1) Lookups

When you need to find an entity by a field value (e.g., looking up a user by username), declare the field as `indexed` and use `lookup()`. This provides O(1) hash-based lookup instead of O(n) query scans:

```rad
component Username { indexed name: "" }
let found = lookup(Username, "name", "alice")
```

Only hashable types (`int`, `float`, `str`, `bool`, `entity`) can be indexed. Use indexed fields for singleton lookups; use `query` for bulk operations.

## 16) Unique Bindings for Zero-Copy Guarantees

In performance-critical code, use `let unique` to guarantee that a value is never aliased and mutations are always in-place:

```rad
let unique mut buffer = []
for item in large_dataset {
    buffer << transform(item)  // never clones
}
```

The compiler rejects aliasing at compile time, eliminating an entire class of performance bugs.

## 17) Copy Profiling for Performance Audits

Use `--profile-copies` to surface hidden `Arc` deep clones in hot paths:

```bash
rad main.rad --profile-copies
```

This prints diagnostics to stderr whenever a list mutation triggers a deep clone due to shared `Arc` references. Combine with `let unique` to achieve zero-copy guarantees.

## 18) Conformance Is the Source of Truth

When docs and behavior disagree, trust conformance tests first and then update docs. Production teams should pin language versions and run the full conformance suite in CI.

## 19) Events Use Domain IDs, Not Entity Pointers

Do not pass raw ECS `entity` IDs in event payloads. Events are domain-level messages. Passing raw entity pointers couples your event bus to the ECS memory layout, making it impossible to serialize events over a network or safely process them across world forks. Instead, use the `indexed` keyword on a component field (like a string ID) and pass that string in the event. The handler can use `lookup()` to resolve the entity in O(1) time.

## 20) Nil is a Concrete Type

In Rad, `nil` is a concrete unit type, not a universal bottom type or implicit null state. If you define a component field as `target: nil`, the compiler infers its type as strictly `nil` and will reject any attempt to assign an entity to it. If a field needs to hold an entity or nil, you must explicitly declare the union: `target: entity | nil = nil`. This forces compile-time null checks (`if target != nil`) before ECS access.
