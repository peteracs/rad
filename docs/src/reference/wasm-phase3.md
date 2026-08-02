# Phase 3: Stateful WASM compiler cutover

## ABI

See [`core/vm/src/compiler_abi.rs`](../../../core/vm/src/compiler_abi.rs): packed `u64` pointer/length returns, `WasmDiagnosticRecord` blob layout, import `env.vfs_read`.

## Rust host

- [`core/vm/src/wasm_binary_emit.rs`](../../../core/vm/src/wasm_binary_emit.rs): builds the reactor stub module with `wasm-encoder` when `native-wasm-phase3` is enabled.
- [`core/vm/src/wasm_compiler_host.rs`](../../../core/vm/src/wasm_compiler_host.rs): wasmtime `Store`, `vfs_read` host import, guest exports `rad_init`, `rad_update_buffer`, `rad_check`, `rad_query_lsp`.
- **`compiler_wasm_bytes_from_env()`** (same module): resolves WASM bytes for the guest — if **`RAD_COMPILER_WASM`** is set, read that file; otherwise use the in-tree stub from `wasm_binary_emit`.
- **`VfsState`**: in-memory overlay (`files`), plus optional **`fallback_dir`**. The host’s `vfs_read` resolves a path in order: **overlay → `std::fs::read(path)` as absolute/relative → `fallback_dir.join(path)`**.

## Self-hosted Rad

- Historical frozen helpers live under
  [`core/c-backend/src/wasm_encode.rad`](../../../core/c-backend/src/wasm_encode.rad)
  and [`core/c-backend/src/emit_wasm.rad`](../../../core/c-backend/src/emit_wasm.rad).
  They are not the active WASM source of truth.

The stateful reactor ABI is documented here and implemented on the Rust host
side by `core/vm/src/wasm_compiler_host.rs` plus the current stub emitter in
`core/vm/src/wasm_binary_emit.rs`. There is no separate source note file under
`core/c-backend/`; that directory is frozen legacy code.

## `rad build`

```text
rad build [--target wasm] <entry.rad> <out.wasm>
```

- **Type-checks** the program (same module graph and checker defaults as a normal `rad` run; no VM execution).
- **Writes** `<out.wasm>`:
  - If **`RAD_COMPILER_WASM`** is set: copies bytes from that path (useful to install a prebuilt guest).
  - Otherwise: writes the **Phase 3 reactor stub** from `emit_compiler_reactor_stub_module()`.
- **`--target`** currently only supports **`wasm`**; it must appear **before** the two positional paths (not after the first file argument).

## `rad lsp` (WASM diagnostics)

Requires a native `rad` built with the **`native-wasm-phase3`** Cargo feature (default for in-tree `rad-vm`).

| Environment variable | Effect |
|----------------------|--------|
| **`RAD_WASM_PHASE3=1`** | Use the WASM reactor for **publishDiagnostics** (loads stub or file per table below). |
| **`RAD_COMPILER_WASM`** | Path to a `compiler.wasm` guest; **also** turns on the WASM diagnostic path (even without `RAD_WASM_PHASE3`). |
| **`RAD_VFS_ROOT`** | Optional workspace root: host sets `VfsState.fallback_dir` so `vfs_read` can load imports from disk when paths are not in the overlay. If unset, the LSP uses the active document’s parent directory as fallback. |

The LSP seeds the overlay with **all open file buffers** (by filesystem path) plus the document being checked. Hover, completion, go-to-definition, and formatting still use the **Rust** implementation unless extended separately. The **stub** guest returns **no** diagnostics until a real `compiler.wasm` implements checking inside `rad_check`.

## Cargo feature

- **`native-wasm-phase3`**: enables wasmtime, stub emission, and the LSP/CLI integration above. Disable with `--no-default-features` if you need a slimmer `rad` without the Phase 3 host.

## VM bytecode heaps (related)

The Phase 3 **compiler** guest is separate from Rad **user** bytecode loaded in the browser or in `RadRuntime`. User chunks built with a scratch heap (e.g. `WasmChunk` in `core/vm/src/wasm.rs`) must be loaded with **`load_chunk_with_gc`** so constants merge into the VM's `GcHeap`. See the [architecture guide](architecture.md) for the VM memory model.

## Recovery note (`core/vm/src/main.rs`)

The full CLI (including `fmt`, `lint`, `test`, `new`, `snapshot`, `play`, **`build`**) lives in [`core/vm/src/main.rs`](../../../core/vm/src/main.rs). If the file was ever truncated locally, recover from version control or editor history.

## Cutover target

The Rust parser/checker/AST in `core/vm` remain the source of truth until a
future VM-owned compiler artifact can replace them for every command.
