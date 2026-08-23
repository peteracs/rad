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

Load-time ABI/determinism verification and application calls have separate
budgets. The native boundary allows at most 700 ms for cold worker
startup/verification. `load_extension(path, timeout_ms)` requires the caller to
choose each application call's deadline from 1 through 900 ms; there is no
one-argument fallback. During startup the
parent reads the framed response concurrently with process-status monitoring,
so an extension that crashes in its determinism probe reports worker exit or
transport failure as soon as either is visible instead of borrowing the hang timeout.
After a call-side socket timeout, an already-faulting Windows child receives a
bounded 20 ms status-publication grace period; a live child is then killed,
reaped, and reported as timeout. These are containment ceilings, not latency
promises for arbitrary third-party code; portfolio operations additionally
must satisfy the end-to-end one-second RAD-process gate.

The loader seals/hashes the image. Every call pins extension, digest, ABI, and
generation. Reload creates a new generation; an in-flight transaction finishes
against its starting generation.

A recorded call stores plugin/image/generation identity, export, input digest,
output bytes/digest, effect class, and outcome. Replay consumes the record and
never invokes the live plugin. Missing, extra, reordered, or mismatched records
fail closed. Host provenance nests into the caller's causal chain.

Calls originating in speculative simulations, parallel system batches, and
model-check trials use nested logical-lane tapes. Their canonical order is the
lane index rather than host thread completion order, so replay remains
worker-count independent while still rejecting cross-lane or reordered calls.

## Verification

```bash
rad ffi verify plugin.dll --contract expected.json --json
```

This checks descriptor/layout/export/effect contracts and declared determinism.
The CLI delegates verification to the same small isolated `rad-ffi-worker`
used for calls; it never recursively launches another full `rad` process and
does not retain a compatibility verifier. The verification child has a 700 ms
ceiling, while the complete CLI operation remains subject to the one-second
portfolio deadline. Loading a library is not an ABI pass. Sovereign Grid and
RiskBridge exercise positive, malformed, crash, timeout, generation,
transaction, and replay paths.
