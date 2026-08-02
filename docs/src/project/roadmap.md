# Rad Language Roadmap

> Last updated: 2026-03-31
>
> This roadmap is a living document. Priorities shift based on community feedback.
> Open an [RFC](rfcs.md) or a Language Design Discussion on GitHub to propose changes.

## Status Key

| Icon | Meaning |
|------|---------|
| :white_circle: | Not started |
| :large_blue_circle: | In progress |
| :green_circle: | Shipped |
| :yellow_circle: | Blocked / needs RFC |

---

## Q2 2026 (Apr–Jun) — Stability & DX

**Theme:** Make the first 10 minutes frictionless. Ship v0.5 stable.

| Status | Item | Tracking |
|--------|------|----------|
| :large_blue_circle: | **v0.5 DX mode stable** — zero-field shorthand, match rest, compat warnings | CHANGELOG [Unreleased] |
| :large_blue_circle: | **Project templates** — `rad new --template workflow\|stream\|simulation` | #TBD |
| :green_circle: | **ECS resource singletons** — `resource` keyword, `get_resource`/`set_resource` builtins, `update(Resource)` sugar, system resource params, parallel conflict analysis, fork/snapshot support | Shipped |
| :green_circle: | **Indexed component fields** — `indexed` keyword, `lookup()` builtin, O(1) entity lookup by field value | Shipped |
| :green_circle: | **Readonly effect level** — `readonly fn`, ECS reads allowed in pipelines, `Effect::ReadECS` | Shipped |
| :green_circle: | **`--profile-copies` diagnostics** — CLI flag to surface hidden `Arc` deep clones with source lines | Shipped |
| :green_circle: | **`unique` binding keyword** — compile-time single-ownership, no aliasing, guaranteed in-place mutations | Shipped |
| :green_circle: | **String builtins** — `split`, `join`, `chars`, `trim`, `starts_with`, `ends_with`, `chr`, `ord`, `to_upper`, `to_lower` | Shipped |
| :green_circle: | **Collection builtins** — `values(map)`, `flat_map`, `group_by`, `append`, `zip` | Shipped |
| :green_circle: | **`pop` fix** — now returns just the last element (matches standard `pop` semantics) | Shipped |
| :green_circle: | **Heterogeneous maps** — mixed-value map literals infer `map<str, any>` | Shipped |
| :green_circle: | **Map iteration** — `for k, v in map { }` syntax | Shipped |
| :green_circle: | **Error recovery in parser** — continue after first error, report multiple diagnostics | Shipped |
| :green_circle: | **LSP completions** — component fields, system parameters, state variants, `system::` paths | Shipped |
| :green_circle: | **Criterion benchmarks** — lexer, parser, compiler, VM execution | Shipped in 0.4.0 |

## Q3 2026 (Jul–Sep) — Type System & Modules

**Theme:** Gradual typing becomes useful for large projects. Module system supports real codebases.

| Status | Item | Tracking |
|--------|------|----------|
| :green_circle: | **Generic functions** — `fn identity<T>(x: T) -> T` | Shipped |
| :green_circle: | **File-based module system** — `use "path.rad"`, recursive loading, cycle detection, duplicate symbol errors, source maps | Shipped |
| :green_circle: | **Module lockfile** — `--write-lock` produces `forge.lock` with per-module checksums | Shipped |
| :green_circle: | **Type aliases** — `type UserId = int` | Shipped |
| :green_circle: | **Module exports & Strict Boundaries** — `pub fn`, `pub component` visibility control, requiring strict types and leak analysis | Shipped |
| :green_circle: | **Import aliasing** — `use "path" as name` for scoped module access and collision avoidance | Shipped |
| :green_circle: | **Struct types** — non-ECS record types for general data modeling | Shipped |
| :white_circle: | **Playground v2** — shareable links, syntax highlighting, autocomplete | #TBD |
| :yellow_circle: | **WASM size optimization** — current bundle ~2MB, target <500KB | Blocked on tree-shaking |

## Q4 2026 (Oct–Dec) — Performance & Ecosystem

**Theme:** RAD is fast enough for production workloads. Package ecosystem bootstrapped.

| Status | Item | Tracking |
|--------|------|----------|
| :green_circle: | **NaN-boxing** — compact `u64` value representation (IEEE-754 NaN space); `Value` is `Copy` | Shipped (`core/vm/src/value.rs`) |
| :green_circle: | **First-Class Speculative Execution** — `fork()`, `simulate()`, `commit()`, `peek()` with static purity checking, Copy-on-Write snapshot architecture (497x fork speedup), C backend support | Shipped |
| :white_circle: | **Package registry** — `rad install`, `rad publish`, `rad.toml` `[dependencies]`, remote version resolution (local file-based `use` imports already shipped, see Q3) | Needs RFC |
| :white_circle: | **Standard library** — `std/collections`, `std/math`, `std/text` as RAD modules | Needs RFC |
| :green_circle: | **Async events** — `async on Event` with cooperative scheduling | Shipped |
| :white_circle: | **Debug adapter protocol** — step-through debugging in VS Code | #TBD |
| :white_circle: | **Property-based testing** — `rad test --fuzz` for system invariants | #TBD |

## 2027 H1 — Long-term Vision

| Item | Notes |
|------|-------|
| **FFI / host bindings** | Call Rust/C functions from RAD (Note: I/O, networking, and file access are now available as built-in functions) |
| **Conflict-aware system batching** | Shipped — `schedule` topologically orders systems, then groups them into batches with no conflicting `mut` component overlap (`core/vm/src/vm/parallel.rs`). Batches still run **sequentially** on one thread. |
| **Multithreaded parallel system runs** | Planned — execute conflict-free batches on multiple threads (not implemented today; no worker pool in the VM scheduler). |
| **Parallel fork simulation** | Evaluate multiple speculative futures concurrently |
| **Fork diff/merge** | Inspect what changed between forks without committing either |
| **AOT compilation** | Compile RAD to native binaries via LLVM or Cranelift |
| **Visual system graph** | IDE extension showing system dependency DAG |
| **Language server protocol v2** | Rename, find references, code actions |

---

## How to Influence This Roadmap

1. **Open an RFC** — for language-level changes (new syntax, type system, semantics)
2. **Open a Language Design Discussion** — for early-stage ideas and community debate
3. **Pick a `good-first-issue`** — string builtins, error messages, and test coverage are great starts
4. **Comment on existing issues** — upvotes and use-case descriptions help prioritize

We publish **monthly release notes** summarizing what shipped, what moved, and what's next.
