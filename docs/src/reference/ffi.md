# Native extension ABI

RAD extension ABI version `3` is declared by
`adapters/native/rad_extension.h` and implemented by `core/vm/src/ffi/`.
Extensions are native-only; browser hosts use explicit JavaScript imports.

## Required exports

```c
RAD_EXPORT const RadExtensionDescriptor *rad_extension_descriptor(void);
RAD_EXPORT void rad_extension_init(const RadPluginApi *api);
```

The descriptor supplies ABI version, extension identity/version, and a
version-1 JSON C-layout contract. Verification rejects missing exports, wrong
ABI/calling convention, duplicate/empty names, invalid alignment,
overlapping/out-of-bounds fields, and mismatched expected layouts before any
function is callable.

## Function and effect declaration

Each `RadNativeFunctionDecl` contains a stable name/pointer, exact arity and
logical signature, complete effects (`reads:Type`, `writes:Type`,
`emits:Event`, `io`, `async`), and deterministic/replayable declarations.
Unknown, empty, or duplicate effects are rejected. Native effects enter the
same transitive authority graph as RAD helpers; they do not grant ownership.

## Value ownership

Functions receive an immutable array of VM-owned `uint64_t` handles and return
one handle made through `RadPluginApi`. Inspect only with `as_*`; allocate only
with `make_*`. String pointers are borrowed for the call. Handle bits are not
durable and cannot cross VM/plugin lifetimes.

`make_host_handle` creates an opaque token sealed to image digest and nominal
type. Another extension cannot forge it; RAD cannot serialize or interpret it.
The owner exposes any close operation.

## Containment, generations, replay

Application calls execute in an isolated worker. Crash, timeout, malformed
output, protocol error, or worker exit becomes a typed host failure. An
enclosing transaction rolls back and later requests remain processable.

The loader seals/hashes the image. Every call pins extension, digest, ABI, and
generation. Reload creates a new generation; an in-flight transaction finishes
against its starting generation.

A recorded call stores plugin/image/generation identity, export, input digest,
output bytes/digest, effect class, and outcome. Replay consumes the record and
never invokes the live plugin. Missing, extra, reordered, or mismatched records
fail closed. Host provenance nests into the caller's causal chain.

## Verification

```bash
rad ffi verify plugin.dll --contract expected.json --json
```

This checks descriptor/layout/export/effect contracts and declared determinism.
Loading a library is not an ABI pass. Sovereign Grid and RiskBridge exercise
positive, malformed, crash, timeout, generation, transaction, and replay paths.
