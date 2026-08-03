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
presentation text.

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
an isolated deterministic fuel and heap budget. Fuel exhaustion, memory
exhaustion, or another ordinary evaluation fault becomes an evaluation
failure; it does not suppress independent invocations.

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
can retain a pointer-free failed-attempt recipe, and `replay_failed_attempt`
re-executes it only when the base digest, request, capability profile, and
limit-profile fingerprint match. It also requires the same compiled-program,
runtime-feature, and constraint-registry digests. Opaque settlement record IDs
are diagnostic only and do not participate in semantic equality.

Candidate details are frozen once per `(entity, component)` and shared by all
violations that reference that candidate. Canonical rejection encoding writes
through a bounded sink; it never constructs an oversized JSON tree merely to
measure it. If the envelope is exceeded, RAD returns one bounded aggregate
limit outcome.

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
