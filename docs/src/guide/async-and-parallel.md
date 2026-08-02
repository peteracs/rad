# Async and Parallel Execution

Rad supports cooperative async tasks and conflict-aware system scheduling.

## Async Basics

- `async fn` creates an async function.
- `async on Event(...)` creates an async event handler.
- `async callee(args)` starts a task and returns a `task` value — `async` must be followed immediately by a **call** (e.g. `async add_one(41)`), not a bare name.
- `await task` resolves the task and yields its inner value.

```rad
async fn add_one(x: int) -> int {
    return x + 1
}

let t = async add_one(41)
print(await t)
```

## Async I/O

When called inside an async function or async handler, blocking builtins are scheduled on the VM I/O pool and return a `task` instead of completing synchronously. Examples include `http_get`, `read_file`, `write_file`, `input`, `readline`, and the other file/HTTP helpers documented in [Built-in Functions](../reference/builtins.md).

Outside async context, the same builtins use their synchronous path (or are unavailable on WASM — see platform notes in the builtins reference).

## System Scheduling

`schedule [A, B, C]` works in two steps:

1. Topological ordering from `after` / `before`.
2. Conflict-aware batching by read/write component and resource sets.

Two systems conflict when they overlap on:

- write/write
- write/read
- read/write

The runtime partitions systems into conflict-free batches and runs each system **sequentially** (one after another) on the main thread. The static analysis that builds those batches is what would enable safe parallel execution later; multithreaded system execution is not turned on in the VM today (there is no Rayon-based or thread-pool system dispatcher in `core/vm/src/vm/exec.rs`).

**What is shipped today:** topological ordering, conflict-free batching (`core/vm/src/vm/parallel.rs`), and sequential execution of each system in order.

**What is not shipped:** running different systems in a batch on different CPU cores at the same time.
