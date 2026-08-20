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

On native targets, the runtime executes each multi-system conflict-free batch
concurrently through Rayon worker VMs. Every worker starts from the same world
snapshot; writes and events merge deterministically in schedule order. A
single-system batch runs directly, and `--serial-schedule` or `schedule serial`
provides a differential serial mode. WASM uses the same isolated worker path
sequentially because the target has no native thread pool.

The batcher consumes each system's exact synchronous transitive authority set,
including ordinary and imported helper calls. Queued event handlers belong to
the later event-drain boundary; a system that explicitly calls `flush_events()`
includes the reached handlers in its synchronous set.
