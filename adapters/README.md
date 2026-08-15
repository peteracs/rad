# Host adapters

Adapters expose the authoritative RAD engine to a particular host surface.
They may depend on `core/vm`; the core must never depend on them.

```text
cli ─────┐
lsp ─────┼──> core/vm
webgpu ──┘
```

- `cli/` owns command parsing, process I/O, and composition of the `rad`
  executable.
- `lsp/` owns Language Server Protocol transport and editor-facing requests.
- `webgpu/` owns disposable browser/GPU resources and materializes bounded
  presentation packets emitted by the runtime.

WASM bindings remain in `core/vm` for now because they directly translate
private VM values, GC-owned state, and session internals. Extract them only
after a stable public session API exists; moving the files alone would create
a reverse dependency or widen unsafe internals.
