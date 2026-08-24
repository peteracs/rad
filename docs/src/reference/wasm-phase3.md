# WASM and JavaScript API
`wasm-bindgen` exports `RadRuntime` and `WasmChunk` on `wasm32`. The generated
[host API surface](./generated/host-api-surface.md) lists every production
method; changing a Rust export makes the documentation gate stale.

## Availability and imports

The single-source browser entry points reject module `use` declarations
because they have no filesystem loader. Production multi-module browser
programs use the canonical package boundary described below. File, process,
TCP, UDP, and dynamic-library builtins are unavailable. Browser hosts provide
explicit, effect-declared imports and record nondeterministic results when
replay is required.

Returned JSON strings are UTF-8 command data or tagged failure.
`compile_and_run_result_json` returns `ok`, `settlement_rejected`,
`runtime_error`, or `host_fault`; consumers switch on `kind` and reject unknown
variants.

## TypeScript declaration shape

The wasm-bindgen package generates concrete declarations. Host adapters may
depend on this structural subset:

```typescript
export class RadRuntime {
  constructor();
  runtime_features(): string;
  compile_and_run(source: string): string;
  compile_and_run_result_json(source: string): string;
  compile_only(source: string): string;
  check_source(source: string): string;
  reset(): void;

  session_start(source: string): string;
  session_start_package(packageJson: string): string;
  session_emit(event: string, fieldsJson: string): void;
  session_emit_binary(event: string, payload: Uint8Array): void;
  session_pump(): string;
  session_drain_binary(channel: string, maxRecords: number, maxPacketBytes: number): Uint8Array;
  session_delta(): string;
  session_apply(delta: string): void;
  session_state(): string;
  session_load(state: string): void;
  session_digest(): string;
  session_checkpoint(): void;
  session_undo(): boolean;
  session_redo(): boolean;
  session_why(entityName: string, component: string): string;
  session_call(name: string, argsJson: string): string;
  session_export_snapshot(): Uint8Array;
  session_import_snapshot(snapshot: Uint8Array): void;

  session_render_buffer_refresh(): void;
  session_render_buffer_refresh_bounded(maxRecords: number, maxEntitiesScanned: number): void;
  session_render_buffer_ptr(): number;
  session_render_buffer_u32_len(): number;
}
```

## Multi-module browser packages

Browser hosts do not concatenate imported source files. Produce one hermetic
package through the native canonical module loader:

```bash
rad build --target browser-package src/main.rad app.radpkg.json
```

The package binds the exact source bytes, canonical source layout and import
edges, compiler version, runtime API, and sorted semantic feature set under one
BLAKE3 digest. `session_start_package()` verifies all of those fields, rebuilds
the module graph without filesystem access, checks it, compiles it, and only
then adopts a new world. Unknown fields, stale versions, changed source,
changed imports, feature mismatch, more than 4,096 units, more than 64 MiB of
source, or more than 96 MiB of encoded package fail closed. Rejected packages
leave the running session untouched.

`session_start(source)` remains the intentional single-source playground API
and still rejects `use`; production multi-module apps use the package API.

Rust `Result<T, String>` exports throw JavaScript exceptions on `Err`; generated
declarations present successful `T`. Callers use `try`/`catch`. `Vec<u8>`
crosses as `Uint8Array`.

## Streaming session

`session_start` or `session_start_package` compiles once and retains one
authority world. Emit/pump advances it; `session_delta` returns an authenticated delta and
`session_apply` rejects wrong lineage/order. `session_digest` is the
convergence receipt. State/snapshot imports validate fully before adoption.
Checkpoint/undo/redo retain canonical snapshots.

## Binary session ingress and publications

Hosts must confirm `"binary-session-io-v1"` in the `runtime_features()`
`features` array before enabling this boundary.

`session_emit_binary(event, payload)` avoids JSON expansion at the browser
boundary. The named event must have exactly one field and that field must be
declared `bytebuf`. The complete event contract and the 1 MiB payload limit are
validated before the live queue changes, so rejection cannot partially enqueue
an event.

RAD publishes ordered host records with `publish_bytes(channel, payload)`.
Publications are transient host output: they do not enter snapshots, world
digests, forks, or replay state. Inside a transaction the builtin is rejected
from the transaction body and is allowed only in `post_commit`, after the state
patch succeeds. The runtime bounds this output to 32 channels, 1,024 queued
records per channel, 1 MiB per record, and 16 MiB total queued bytes.

`session_drain_binary(channel, maxRecords, maxPacketBytes)` atomically removes
only complete records that fit the caller's limits. It returns `RBS1`, followed
by a little-endian `u32` record count and repeated little-endian `u32` byte
length plus payload records. An empty drain is `RBS1` plus a zero count. If the
next record cannot fit, the call fails without removing it.

## Render buffer ownership

`runtime_features()` publishes the versioned presentation descriptor, field
offsets, record/header lengths, limits, packet kinds, and features. Call
`session_render_buffer_refresh_bounded`, then create a read-only `Uint32Array`
view from WASM memory, pointer, and length. It is borrowed only until the next
runtime call or memory growth. Validate descriptor, count, packet kind,
sequence/base sequence, flags, booleans, enum IDs, and finite floats.

## Determinism and errors

Sessions initialize deterministic RNG state. Equal source and ordered inputs
must converge to equal digest independent of host worker count. Wrong-version
JSON, unknown descriptors, invalid limits, malformed deltas/snapshots, and
stale render packets fail closed; no partial state is adopted.
