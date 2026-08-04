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

Per-invocation fuel and heap meters are independent. A separate aggregate host
envelope is reserved before validation, so one constraint cannot consume the
semantic contract promised to another. Proposal, candidate, and rejection
values share one versioned value-limit domain. Candidate details are retained
once per rejected `(entity, component)` and canonical rejection bytes are
emitted through an exact bounded writer. Host-budget and encoding failures use
the typed host-fault path and expose no partial constraint result.

The complete v0 syntax, static restrictions, interoperability rules, and
non-goals are specified by [RFC-0001](https://github.com/peteracs/rad/blob/main/docs/rfcs/0001-causal-settlements.md)
and [RFC-0002](https://github.com/peteracs/rad/blob/main/docs/rfcs/0002-candidate-constraints.md),
and summarized in the [Causal Laws](../guide/causal-laws.md) and
[Candidate Constraints](../guide/candidate-constraints.md) guides.

---
