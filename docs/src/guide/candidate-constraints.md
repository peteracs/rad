# Candidate Constraints

Candidate constraints complete the validation phase of RAD Causal Laws. They
run after every resolver has produced a conflict-free patch and before that
patch is adopted. Enable them with `--experimental-laws`.

```text
immutable base snapshot
    -> laws propose intents
    -> resolvers build the complete candidate patch
    -> constraints validate one immutable candidate view
    -> zero outcomes: atomic commit
    -> any violation or evaluation failure: atomic rejection
```

Constraints never repair or project state. They have no ordering directives,
priorities, or first-error behavior.

## Declaring a constraint

A constraint is attached to one component type:

```rad
constraint WorldBounds for Position(subject, proposed) {
    require proposed.x >= -100 else "position.below_world_min"
    require proposed.x <= 100 else "position.above_world_max"
}
```

`subject` is the entity whose candidate component triggered validation.
`proposed` is the complete candidate value for the attached component. A
failed `require` adds its stable lowercase code to the settlement rejection;
it does not stop other constraints from running.

The attached component must be defined in the same module. Constraint names
and violation codes are semantic identities, so use stable names rather than
presentation text. Violation codes are limited to 128 lowercase ASCII bytes
using letters, digits, `.`, `_`, or `-`.

## Base and candidate reads

Every invocation sees two immutable worlds:

```rad
constraint ForwardOnly for Position(subject, proposed) {
    let old_position = base(subject, Position)
    require proposed.x >= old_position.x else "position.backwards"
}
```

- `base(subject, Component)` reads the pre-settlement snapshot.
- `candidate(subject, Component)` reads the complete candidate world: the
  staged value when present, otherwise the base value.

A constraint cannot read another entity. It cannot observe another
constraint's outcome.

## Watched components

Cross-component invariants declare their same-entity trigger dependencies:

```rad
constraint ConsistentMotion
    for Position(subject, proposed)
    watches Velocity
{
    let velocity = candidate(subject, Velocity)
    require proposed.x == velocity.x else "motion.mismatch"
}
```

This constraint runs when either `Position` or `Velocity` is staged, provided
the candidate entity has `Position`. If both are staged, the constraint still
runs exactly once for that subject. Reading another component without listing
it in `watches` is a compile-time error.

## Rejection behavior

Constraints collect all bounded semantic outcomes. Each invocation receives
an isolated deterministic fuel and heap budget. Every bytecode operation and
audited native builtin is charged by its work; aggregate constructors and
native collection operations preflight retained and peak temporary storage
from their actual inputs. A helper without a conservative, tested upper bound
fails closed. Each invocation uses a disposable GC heap, so previous
invocations cannot consume its allowance and its allocations are reclaimed
before the next constraint starts. Fuel exhaustion, memory exhaustion, or
another ordinary evaluation fault becomes an evaluation failure; it does not
suppress independent invocations.

The runtime reserves a separate aggregate fuel/heap envelope before the first
selected invocation runs. If the complete invocation set cannot fit that host
envelope, validation returns a typed host fault before exposing any partial
outcome. One versioned value-limit profile governs proposal capture, candidate
capture, rejection details, and canonical rejection output.

Any violation or evaluation failure rejects the whole settlement:

```text
world               unchanged
durable provenance  unchanged
events/output/RNG    unchanged
candidate patch      discarded
VM                   reusable
```

Hosts should use `run_detailed`, `call_global_detailed`, or
`call_global_attempt` to receive a typed `VmFailure::SettlementRejected`.
The older `run` and `call_global` methods remain compatibility wrappers that
render the same typed result as text. Browser hosts can use
`compile_and_run_result_json`, whose result is tagged with
`"kind": "settlement_rejected"`.

Rejected attempts are not authoritative ledger entries. `call_global_attempt`
captures a graph-isolated checkpoint **before** it invokes the target. A
rejected result is a `RecordedFailedAttempt`: it owns that private in-process
seed and exposes `portable_recipe()` for the pointer-free recipe. Calling
`replay_failed_attempt(recorded)` always forks the original pre-attempt seed,
so a global or closure capture changed before the failed settlement is replayed
from its original value—not the authoritative VM's later value.

Portable recipes deliberately do not smuggle a VM heap into wire data. A host
must supply the exact state checkpoint and call
`replay_portable_failed_attempt(recipe)`; RAD verifies the canonical checkpoint
digest in addition to the base, request, capability, limit-profile,
compiled-program, runtime-feature, and constraint-registry identities. Opaque
settlement record IDs are diagnostic only and do not participate in semantic
equality.

The compiled-program identity comes from an immutable canonical manifest. It
includes the authenticated source/import graph and exact global symbol slot
order as well as bytecode, constants, handler/system tables, causal registries,
schemas, indexes, and migrations.
Swapping two names while leaving global values untouched is therefore a
different program and is rejected before portable replay runs. Checkpoint
identity uses an explicit versioned encoding rather than diagnostic formatting.

Portable identity distinguishes three deliberately different digests:

- the world content digest covers visible entities, components, and resources;
- the operational world digest additionally covers allocator/type state,
  archetype and index state, event/timer payloads, provenance, and observable
  rollout metadata; and
- the attempt checkpoint digest combines that operational world with the
  complete VM execution context and compiled-program manifest.

Replay validation uses the operational form. The renderer-oriented content
digest is never accepted as proof that two worlds will execute identically.
`WorldFork` values and timeline snapshots use the same operational encoder, so
hidden allocator or queued-event differences change their replay identity.
Snapshot sharing is part of that identity too: two `WorldFork` wrappers that
share one immutable snapshot are distinct from two wrappers around equal but
separately allocated snapshots, matching RAD's observable fork equality.
Replay fingerprinting assigns deterministic IDs to heap objects, closure
captures, and world snapshots, then processes each distinct node once. This
keeps shared snapshot DAGs linear in their distinct nodes and avoids recursive
snapshot expansion. Node, edge, pending-work, world-count, and exact encoded
byte limits fail as a typed host error before portable replay executes.

Loaded native extensions are copied to a read-only, content-addressed image
before the dynamic loader sees them, and that image remains open for the
library's lifetime. Their binary digest, ABI,
target, exports, declared effect class, and resource-contract version are
bound into program identity, while process-local function pointers are not.
ABI-v1 extensions have no self-declared package version and are conservatively
treated as host-effecting and constraint-unsafe. Loading the same extension ID
with different sealed content is rejected.

Public attempt recording is an authoritative main-timeline operation; worker
and simulation-fork VMs are rejected as checkpoint roots. The detached replay
shell preserves the source gameplay execution role independently from its
observational safety flag. Checkpoint identity also covers scheduler mode,
trace state, emit ancestry IDs, simulation depth, and every event handler's
`once`/fired state, so portable replay cannot silently substitute worker or
otherwise semantically different execution state.

Candidate details are frozen once per `(entity, component)` and shared by all
violations that reference that candidate. One settlement outcome meter charges
violations, failures, metadata, details, and origins before retaining them.
Canonical rejection encoding adds an independent bounded sink; it never
constructs an oversized JSON tree merely to measure it. If the envelope is
exceeded, RAD discards detailed outcomes, continues evaluating the remaining
constraints, and returns one bounded aggregate limit outcome rather than an
order-dependent prefix.

Attempt replay is observational. It executes the recorded call in a detached
child VM and discards that child for every result, including an unexpected
commit. Its graph-aware fork rewrites closure captures to child-owned cells,
preserves shared aliases and cycles within the child, and clones globals,
sealed constants, queued event payloads, event-log payloads, and completed task
values through one identity map. Native/FFI and irreversible host effects are
disabled in this mode. Replaying a failed attempt therefore cannot mutate the
authoritative VM or host being inspected. Replay graph construction is bounded,
and replacing cycle-breaking placeholders re-accounts the populated object's
actual retained list, map, string, closure, or buffer storage.

## Capabilities and redaction

RAD builds one internal rejection and renders it through the recipient's
component-read and origin capabilities. Values outside that grant become a
stable redaction tag. When origins are hidden, the law, resolver, intent,
source location, payload, length, and hidden sort identity are all replaced by
opaque bounded placeholders. Hidden values never influence visible ordering.

## Complete movement example

The dogfood project combines three simultaneous movement causes:

```text
Velocity + Wind + Knockback
    -> Displacement resolver
    -> candidate Position
    -> WorldBounds + NonPenetration
    -> commit or canonical rejection
```

Run it with:

```powershell
rad projects/dogfood/causal-constraints/main.rad --experimental-laws
```

See [RFC-0002](https://github.com/peteracs/rad/blob/main/docs/rfcs/0002-candidate-constraints.md)
for the normative semantics and explicit non-goals.
