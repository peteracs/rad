# RFC-0002: Candidate Constraints—Order-Independent Settlement Validation

- **Status:** Implemented — stable experimental transactional core
- **Product name:** RAD Causal Laws
- **Paradigm:** World-Law Programming
- **Author:** peteracs
- **Created:** 2026-08-03
- **Last updated:** 2026-08-04
- **Depends on:** [RFC-0001](0001-causal-settlements.md)
- **Proposed feature gate:** `--experimental-laws`

## Summary

RFC-0002 adds validation-only `constraint` declarations to the candidate-patch
boundary established by RFC-0001. After all resolvers have produced one
complete, conflict-free patch, constraints inspect the same immutable base
snapshot and the same immutable candidate view. They can report structured
violations but cannot change candidate state. A settlement with no violations
commits normally; any violation aborts the settlement atomically.

The defining rule is:

> All constraints read one complete candidate. Constraints do not write,
> order, project, or repair it. Their canonically ordered violations determine
> whether the candidate may commit.

Constraint execution order never carries program meaning. This RFC does not
introduce projection, correction, priorities, or fixed-point solving.

## Motivation

RFC-0001 gives one resolver semantic ownership of each intent and rejects
overlapping candidate writes. It deliberately reserves this transition:

```text
base
  → proposals
  → resolution patch
  → future constraints
  → atomic commit
```

Resolvers answer:

```text
How do these causes combine into candidate state?
```

They should not also have to duplicate every world invariant that determines
whether that state is legal. For example, movement may combine velocity, wind,
and knockback correctly while still producing a position outside the playable
world or inside static geometry.

Putting validation into the movement resolver has three problems:

- every resolver that writes `Position` must repeat the same invariants;
- the invariant sees only that resolver's local reasoning rather than the
  complete settlement candidate; and
- future projection pressure encourages manually ordered mutation stages.

A validation-only constraint phase creates one explicit, explainable boundary:

```text
simultaneous causes
        ↓
semantic resolution
        ↓
complete candidate
        ↓
order-independent validation
        ↓
commit or atomic abort
```

The first version is intentionally less powerful than a general constraint
solver. Its purpose is to prove that invariants can inspect a causal transition
without reintroducing schedule semantics.

## Product objective

The first release must prove:

> Independent invariants can validate the same complete settlement candidate,
> and RAD can deterministically explain every violation without relying on
> constraint declaration or execution order.

A successful vertical slice demonstrates:

```text
Velocity + Wind + Knockback proposals
              ↓
      ResolveDisplacement
              ↓
       candidate Position
              ↓
    WithinWorldBounds validation
    NonPenetration validation
              ↓
    zero violations → atomic commit
    violations      → atomic abort
```

## Proposed syntax

### Constraint declaration

```rad
constraint WithinWorldBounds for Position(subject, proposed) {
    require proposed.x >= -1000.0 else "position.x_below_world_min"
    require proposed.x <=  1000.0 else "position.x_above_world_max"
    require proposed.y >= -1000.0 else "position.y_below_world_min"
    require proposed.y <=  1000.0 else "position.y_above_world_max"
}
```

A constraint is attached to one component type and may explicitly watch other
same-entity component types:

```rad
constraint ValidMotion
    for Position(subject, proposed)
    watches Velocity, MovementMode
{
    // ...
}
```

Its parameters are:

- `subject`, whose type is `entity`; and
- `proposed`, whose type is the attached component.

The attached component plus every `watches` component form the constraint's
trigger set. The runtime invokes the constraint once for a subject when any
trigger component is staged and the attached component exists in the complete
candidate world. The `proposed` parameter is the attached component's candidate
value, including base fallback when only a watched component triggered the
invocation.

Every watched component type must be visible in the defining module. The
checker rejects duplicate watched types and rejects listing the attached
component again in `watches`.

Staging a value identical to its base value still triggers validation. If two
or more trigger components are staged for one subject, the constraint is still
selected exactly once.

In v0, a constraint must be declared in the module that defines its component.
Several named constraints may be declared for one component because they do
not own or mutate the component. Imported modules cannot attach new constraints
to a foreign component.

This same-module rule makes the invariant set part of the component's stable
API and prevents importing a module from silently changing commit behavior.

### Requirement statement

The contextual requirement syntax is:

```text
require <boolean expression> else <string literal violation code>
```

The code must be non-empty and unique within its constraint declaration. It is
a stable machine-readable identifier, not an execution-order key. The checker
rejects a non-boolean predicate, a dynamic code expression, or duplicate codes.

When the predicate is false, the runtime appends one structured violation. It
does not return early. Loops and local control flow may reach a requirement
site more than once; duplicate violations are preserved rather than silently
deduplicated.

The existing `require(entity, Component)` ECS builtin remains distinct because
it uses call syntax with parentheses. Inside a constraint it reads the base
snapshot, just as it does inside a resolver.

### Candidate view

Constraints receive their attached candidate component directly and may inspect
declared watched components through a contextual read:

```rad
candidate(subject, Velocity)
```

`candidate(subject, Component)` has type `option<Component>` and reads the
immutable candidate world:

```text
if (entity, Component) is staged in the patch:
    return the staged value
otherwise:
    return the value from the immutable base snapshot
```

It never reads a partially validated result because constraints cannot modify
the view. In v0, the entity expression must be the constraint's `subject`
parameter, and the component must be either the attached type or listed in
`watches`. Cross-entity candidate reads, implicit dependency inference,
candidate ECS queries, candidate resource reads, and structural candidate
changes are not supported.

Explicit watches are mandatory even when the checker could infer a direct read.
This keeps trigger dependencies visible when values are passed through helper
functions and prevents a helper refactor from silently changing selection.

Normal `get`, `require`, queries, and readonly helpers continue to read `base`.
The distinct `candidate(...)` spelling makes future-state reads visible during
review and in effect diagnostics.

Example:

```rad
constraint MovingBodyRemainsFinite
    for Position(subject, proposed)
    watches Velocity
{
    let future_velocity = expect(
        candidate(subject, Velocity),
        "MovingBodyRemainsFinite requires Velocity",
    )

    require proposed.x >= -1000000.0 and proposed.x <= 1000000.0
        else "position.x_not_finite_range"
    require proposed.y >= -1000000.0 and proposed.y <= 1000000.0
        else "position.y_not_finite_range"
    require future_velocity.dx >= -10000.0 and future_velocity.dx <= 10000.0
        else "velocity.dx_out_of_range"
    require future_velocity.dy >= -10000.0 and future_velocity.dy <= 10000.0
        else "velocity.dy_out_of_range"
}
```

### Complete movement example

```rad
component Position { x: float = 0.0, y: float = 0.0 }
component Collider { radius: float = 0.5 }

pure fn overlaps_static_world(position: Position, collider: Collider) -> bool {
    // A static wall occupies x > 10.0 in this minimal dogfood world.
    return position.x + collider.radius > 10.0
}

intent Displacement {
    key target: entity
    dx: float
    dy: float
    cause: str
}

law InertialMovement(target: entity, dx: float, dy: float) {
    propose Displacement {
        target: target,
        dx: dx,
        dy: dy,
        cause: "velocity",
    }
}

law Wind(target: entity, dx: float, dy: float) {
    propose Displacement {
        target: target,
        dx: dx,
        dy: dy,
        cause: "wind",
    }
}

law Knockback(target: entity, dx: float, dy: float) {
    propose Displacement {
        target: target,
        dx: dx,
        dy: dy,
        cause: "knockback",
    }
}

resolver ResolveDisplacement for Displacement(target, proposals) {
    let old = require(target, Position)
    let dx = proposals |> map(fn(p) { return p.dx }) |> sum()
    let dy = proposals |> map(fn(p) { return p.dy }) |> sum()

    next(target, Position {
        x: old.x + dx,
        y: old.y + dy,
    })
}

constraint WithinWorldBounds for Position(subject, proposed) {
    require proposed.x >= -1000.0 else "position.x_below_world_min"
    require proposed.x <=  1000.0 else "position.x_above_world_max"
    require proposed.y >= -1000.0 else "position.y_below_world_min"
    require proposed.y <=  1000.0 else "position.y_above_world_max"
}

constraint NonPenetration
    for Position(subject, proposed)
    watches Collider
{
    let collider = expect(
        candidate(subject, Collider),
        "NonPenetration requires Collider",
    )
    require not overlaps_static_world(proposed, collider)
        else "position.penetrates_static_world"
}

entity hero {
    Position {}
    Collider {}
}

settle {
    InertialMovement(hero, 0.16, 0.0)
    Wind(hero, 0.0, 0.03)
    Knockback(hero, 1.2, -0.4)
}
```

The resolver combines causes. The constraints decide whether the one completed
candidate is admissible. They do not alter the position.

## Normative semantics

### Phase boundary

Constraints run only after RFC-0001 has produced one coherent patch:

```text
1. capture immutable base snapshot
2. execute laws and collect proposals
3. canonicalize proposals
4. run resolvers against base
5. reject duplicate or overlapping candidate writes
6. validate component existence and sandbox write ACLs
7. freeze the complete sparse patch as CandidateView
8. evaluate applicable constraints against base + CandidateView
9. canonicalize all outcomes
10. zero violations/errors → apply patch and commit provenance atomically
11. any violation/error    → abort without world or ledger changes
```

Constraints never run if resolver execution, conflict checking, component
validation, or sandbox write authorization has already failed. This prevents a
constraint from observing an incoherent or unauthorized candidate.

The candidate view is a read-only overlay. Implementations do not need to clone
or mutate a temporary world before validation.

### Constraint selection and watched dependencies

Each constraint statically declares:

```text
attached component
+ zero or more same-entity watched components
= trigger set
```

For every entity with at least one trigger component in the patch, the runtime
selects that constraint exactly once if its attached component exists in the
complete candidate world. The attached `proposed` value comes from the
candidate overlay even when only a watched component was staged.

For example, a `ValidMotion for Position watches Velocity` constraint runs when
either `Position`, `Velocity`, or both are staged for the subject. If only
`Velocity` changes, it receives base-fallback `Position` as `proposed` and the
staged `Velocity` through `candidate(subject, Velocity)`.

Constraints are not invoked when none of their trigger components changed.
RFC-0002 therefore validates declared transition dependencies, not every
pre-existing fact in the world. A separate world-audit operation, if useful,
requires another RFC.

Watch dependencies are restricted to the current subject. Cross-entity watches
would require reverse indexes, invalidation fan-out, capability rules, and a
bounded selection contract; they require a later RFC.

The runtime may enumerate patch entries and constraints in a canonical internal
order for reproducible resource use and diagnostics. That order is unobservable
to the program because all constraint inputs are immutable and every outcome is
collected before the settlement result is chosen.

### Snapshot and candidate reads

Every constraint invocation sees:

```text
base      = the RFC-0001 settlement snapshot
candidate = the same complete, conflict-free sparse patch overlay
```

No constraint can observe:

- resolver-local staging before the complete patch exists;
- another constraint's progress;
- a filtered patch containing only its attached component; or
- world state committed by the same settlement.

Constraint-safe RAD bytecode helpers may be called. Readonly ECS helpers
continue to read base and are accepted only when their complete transitive call
graph satisfies the constraint-safe effect contract below. Candidate components
may be passed to safe helpers as ordinary values. `candidate(...)` itself is
contextual and may only appear directly inside a constraint body in v0;
candidate-aware helper effects are deferred.

The checker requires each candidate component type to appear in the declaration's
trigger set and requires the lookup entity to be the exact `subject` parameter.

### Effect rules

A constraint has the checked effect context:

```text
ReadECS + ReadCandidate + Violate
```

It may:

- read base ECS facts;
- read the immutable candidate overlay;
- perform local deterministic computation;
- call RAD bytecode helpers whose complete transitive effect set is
  constraint-safe;
- call builtins from a small versioned deterministic-pure whitelist;
- execute local control flow; and
- report violations through contextual `require` statements.

It may not:

```text
set / update / spawn / despawn / remove
next / propose
emit / flush_events
commit / load_world / merge
perform I/O
use randomness or clock time
call async code
call a law, resolver, or constraint
call settle
call arbitrary native or FFI code
mutate a global or captured binding
spawn a task or retain hidden host state
```

Forbidden effects are rejected statically and transitively with the complete
effect call chain. `readonly` alone is not sufficient: native registrations and
helpers must be proven to belong to the constraint-safe call closure. Arbitrary
native and FFI calls are deferred from v0 even if a host labels them readonly.

List iteration preserves list order. Map and set iteration inside a constraint
uses canonical stable-key order; collection types without a defined canonical
iteration are rejected by the checker. Branching may not depend on allocation,
hash-table, registration, or host callback order.

Local values may be transformed during evaluation, but no mutation may survive
outside the invocation's isolated frame/stack checkpoint. Runtime checks remain
defense in depth for malformed or untrusted bytecode. The observational
invariant is:

> After a constraint invocation, the VM is identical to its pre-invocation
> state except for isolated fuel accounting and the returned structured
> outcome.

### Structured violations

Each failed requirement produces a value conceptually equivalent to:

```text
ConstraintViolation {
    constraint_name
    component_type
    entity
    code
    requirement_source_span
    candidate_key
}
```

`candidate_key` references one rejection-level candidate detail and one
ephemeral causal explanation. The detail is frozen at most once for each
`(entity, component type)`, no matter how many requirements reject it. Its
explanation follows the one resolution patch that staged the component and
therefore contains only that resolver's proposal fan-in, not every proposal in
the settlement.

Violation codes are user-authored semantic identifiers. Constraint name,
component type, entity, and source span are runtime metadata and are not exposed
to constraint code as arbitration inputs.

All violations are sorted by:

```text
entity id
component stable type name
constraint stable qualified name
violation code
requirement source span
canonical candidate-write origin
```

This produces:

```text
same base + same proposal multiset + same constraint set
    ⇒ same candidate result
    ⇒ same canonically ordered violation list
```

Declaration order, patch enumeration order, and constraint execution order are
not tie-breakers. Opaque record or allocation identifiers are never sorting
inputs. Byte-identical duplicate violations preserve multiplicity and are
semantically indistinguishable from one another.

### Outcomes and deterministic resource contract

Each selected invocation returns exactly one semantic outcome:

```rust
enum ConstraintOutcome {
    Valid,
    Violations(Vec<ConstraintViolation>),
    EvaluationFailure(ConstraintEvaluationFailure),
}
```

Settlement validation then returns:

```rust
enum ValidationResult {
    Accepted,
    Rejected(SettlementRejection),
    HostAborted(HostFault),
}
```

Every invocation receives an isolated deterministic resource contract:

```text
fuel_per_invocation
max_heap_bytes_per_invocation
max_violations_per_invocation
max_violations_per_settlement
max_serialized_outcome_bytes
max_aggregate_fuel
max_aggregate_heap_bytes
```

All limits are finite, immutable for the settlement, exposed through the
runtime capability profile, and included in attempt-replay metadata. The
serialized-outcome limit measures the exact canonical UTF-8 rejection bytes,
including escaping and delimiters. Hosts may choose a supported profile before
execution; changing it creates a different attempt contract rather than a
nondeterministic replay of the same attempt.

Proposal capture, candidate capture, violation details, explanations, and
canonical rejection output use one synchronized transaction value-limit
domain. A host cannot configure a causal value that is legal to capture but
illegal merely because rejection encoding silently applies a narrower value
profile.

The aggregate limits are a process-safety envelope, not a shared semantic
meter. Before any invocation runs, the runtime checks the complete selected set
against that envelope. Failure returns `HostAborted` without exposing a partial
collection. Inside the reservation, every invocation receives exactly its own
declared fuel and heap allowance; earlier execution cannot consume a later
invocation's contract. Fuel is charged at every dispatched constraint opcode,
including straight-line basic blocks. Each invocation allocates into a fresh
throw-away GC heap; variable-size aggregate constructors preflight temporary
and retained storage, every opcode performs a retained-heap backstop check, and
the heap is discarded before the next invocation begins. Native builtins are
admitted only when their preflight quote is a conservative upper bound for
both native work and peak temporary-plus-retained allocation. The quote uses
the existing input size as well as requested growth (including source clones
and empty-pattern string expansion); a helper whose upper bound is not yet
proved fails closed instead of running under an estimate.

Admission is generated from a native-proof registry. Each admitted builtin has
a named proof class and boundary-suite identity; no independent whitelist may
drift away from those records. `range` pricing and execution normatively share
one checked `RangePlan { start, step, count }`. Planning uses widened checked
arithmetic, rejects an unrepresentable count before allocation, and validates
the final generated value. Execution emits exactly `count` values by checked
index arithmetic. An open-ended `i += step` loop is forbidden because signed
overflow would invalidate termination and the quoted resource bound.

A constraint can fail while evaluating—for example, a base `require(entity,
Component)` may find a missing component. Ordinary errors and per-invocation
fuel exhaustion become canonical `EvaluationFailure` records. When an
invocation exceeds its violation-count or byte limit, its partial output is
discarded and replaced by one stable output-limit evaluation failure.

The runtime evaluates every independently selected invocation within its own
contract and collects both violations and evaluation failures. Outcomes are
canonically sorted only after evaluation. If the settlement-wide count or byte
limit is exceeded, no order-dependent prefix is exposed; the rejection contains
one bounded aggregate limit failure. Candidate values are retained once per
candidate key and violations reference them. Proposal origins are bounded and
filtered to the candidate's owning patch. Canonical output is written directly
through a bounded sink rather than building an oversized JSON tree first. The
settlement-wide outcome meter charges records before retention. Once it
overflows, all detailed outcomes are discarded immediately, remaining
constraints continue inside their isolated contracts, and no later detail or
order-dependent prefix is retained.

Process cancellation, allocator failure, VM termination, or another true
host-fatal condition produces `HostAborted`. A host-fatal result exposes no
partial violation or evaluation-failure collection. Candidate state and durable
provenance remain unchanged for `Rejected` and `HostAborted` alike.

Evaluation failures are canonically ordered by entity, component, qualified
constraint name, source span, and stable error code. This makes “collect all”
mean:

> Collect every semantic outcome from every selected invocation that runs
> inside the declared deterministic resource contract.

### Atomic abort and VM reuse

If validation is rejected or host-aborted:

```text
world == pre-settlement world
ledger == pre-settlement ledger
active settlement == none when the public execution boundary returns
```

Transient proposals, patches, candidate views, and constraint outcomes are
discarded. A rejected or host-aborted constraint phase must not poison a reused
VM. Host-fatal process termination is the sole case where reuse cannot be
promised because the VM no longer returns control.

### Ephemeral failed-transition explanation

Successful settlements extend the durable RFC-0001 provenance tree normally.
Failed settlements do not append settlement, proposal, resolution, candidate,
or constraint records to the durable ledger.

Instead, the returned settlement error owns an ephemeral explanation tree:

```text
Settlement rejected: 2 constraint violations

`Position` of entity `hero` candidate = { x: 1001.36, y: 3.60 }

  <- constraint `WithinWorldBounds`
     code: position.x_above_world_max

     candidate staged by resolver `ResolveDisplacement`
       <- proposal Displacement { cause: "velocity", ... }
       <- proposal Displacement { cause: "wind", ... }
       <- proposal Displacement { cause: "knockback", ... }

  <- constraint `NonPenetration`
     code: position.penetrates_static_world
```

Hosts inspect structured outcomes and may render a bounded tree. Catchable
in-language rejection values and durable failed-transaction history remain
outside v0.

Repeated common proposal ancestry is collapsed, and large fan-in uses the same
bounded presentation policy as `why()`. Retention lasts only as long as the
returned error value or host response.

`settlement_id` and other opaque runtime record identities may be retained for
diagnostics, but they are excluded from canonical semantic rejection bytes.

### Typed host failure boundary

A rejected candidate is not a VM bug or host failure. New detailed embedding
entry points return a typed boundary:

```rust
pub enum VmFailure {
    SettlementRejected(Arc<SettlementRejection>),
    Runtime(RuntimeError),
    Host(HostFault),
}

pub fn run_detailed(...) -> Result<(), VmFailure>;
pub fn call_value_detailed(...) -> Result<Value, VmFailure>;
```

Existing string APIs remain compatibility wrappers:

```rust
pub fn run(...) -> Result<(), String> {
    run_detailed(...).map_err(|failure| failure.render())
}
```

They do not store a mutable `last_rejection()` side channel. The structured
rejection is owned by the returned error, which remains safe under nested host
calls, callbacks, and future concurrency.

WASM and JSON host surfaces expose a tagged object after capability filtering:

```json
{
  "kind": "settlement_rejected",
  "violations": [],
  "evaluation_failures": [],
  "why": {}
}
```

`Runtime` represents language/bytecode execution faults;
`SettlementRejected` represents a successfully evaluated but inadmissible
candidate; and `Host` represents cancellation, allocation, process, or other
host-fatal failure.

### Capability-filtered rejection rendering

The runtime maintains an internal canonical rejection for trusted settlement
machinery, then derives a recipient-specific rendered rejection:

```text
internal canonical rejection
        ↓ capability filter
deterministic rendered rejection
```

Constraint evaluation itself uses the settlement's read capability context. An
unauthorized `candidate(...)` or base read becomes a stable ACL evaluation
failure without exposing the value. A trusted invariant may also possess more
authority than the sandbox or debugger receiving its rejection; the recipient
never receives values or proposal origins outside its rendering capability.

Redaction uses stable placeholders:

```text
candidate value: <redacted>
proposal source: <redacted-origin>
```

The filtered renderer re-canonicalizes on visible metadata and stable redacted
tokens, so hidden payloads cannot influence exposed ordering. Multiplicity is
preserved, but secret values, source identity, lengths beyond declared bounded
counts, and hidden canonical sort keys are not leaked. Redaction changes only
presentation, never validation or the internal accept/reject result.

When origins are not visible, the renderer hides the origin as one unit. It
does not expose the producing law, resolver, intent, source location, payload,
payload length, or an internal sort identity.

## Ordering is deliberately absent

The language must not support:

```rad
constraint A before B
constraint A after B
constraint A priority 100
```

Constraints do not short-circuit one another, write a shared error buffer in
observable order, or select the first declaration failure. No constraint may
repair state for another constraint to inspect.

When validation genuinely depends on an earlier committed transition, the
program uses two settlements:

```rad
settle { /* first transition validates and commits */ }
settle { /* second transition sees the committed result as base */ }
```

Within one settlement, all invariants judge one simultaneous candidate. Between
settlements, temporal causality is explicit.

## Interoperation with RFC-0001 and existing RAD

Everything remains opt-in under the experimental Causal Laws surface. Existing
systems, schedules, handlers, events, forks, simulation, direct mutation, and
`accum` behavior do not change.

Additional rules:

- constraints are selected only from trigger-component writes staged by a
  settlement;
- legacy writes outside a settlement do not invoke constraints;
- a handler may invoke a settlement and receives a typed
  `SettlementRejected`, distinct from a runtime or host failure;
- constraints cannot emit events, so a failed settlement never leaks events;
- ledger replay deterministically reconstructs successful committed constraint
  decisions only;
- forks and simulation use the same constraint registry as their source VM;
- candidate reads and candidate write origins obey sandbox read/write ACLs;
- `settle` does not flush events before or after validation;
- nested settlements and parallel worker settlements remain rejected; and
- the frozen C backend does not parse or execute constraint syntax.

### Ledger replay versus attempt replay

RFC-0002 defines two distinct replay products:

```text
Ledger replay:
    reproduces committed authoritative transitions only.

Attempt replay:
    re-executes one versioned input/event request against the same base-world
    digest and deterministic runtime profile, producing the same commit or
    structured rejection.
```

Attempt replay is observational: it executes in a detached child VM and always
discards that child. A mismatch, host abort, runtime fault, or unexpected
commit cannot mutate the authoritative VM being inspected. The fork is a
graph clone rather than an independent tree copy: object and capture-cell
identity are memoized, closures have every capture pointer rewritten to a
child-owned cell, shared aliases stay shared inside the child, and cycles close
only over child storage. Replay-visible globals, sealed constants, queued event
payloads, event-log payloads, and completed-task values participate in the
same clone context. Native/FFI calls and irreversible host-effect builtins fail
closed during observational replay.

Attempt recording is accepted only at a quiescent authoritative main-timeline
boundary. Worker and simulation-fork VMs are not portable attempt roots in
RFC-0002. The replay shell restores the source gameplay execution role before
running the request, then independently enables observational safeguards such
as output suppression and irreversible-host-effect denial. Observational mode
must never obtain worker scheduling or event semantics merely because worker
construction machinery was reused internally.

The in-process checkpoint is captured before the attempted request executes.
This distinction is normative: a request may update a global or captured cell
before entering `settle`, and replay must still begin from that original value.
`RecordedFailedAttempt` pairs the private detached seed with the pointer-free
`FailedSettlementAttempt` recipe. Portable replay requires a separately
supplied checkpoint whose canonical digest matches the recipe; it never falls
back to cloning the authoritative VM's current post-attempt state.

One captured `AttemptReplayState` drives both detached-child construction and
checkpoint hashing. It includes execution role, serial scheduling, trace and
simulation state, current/next emit IDs, handler fired bits, world/provenance,
limits, sandbox state, and the remaining replay-semantic counters and buffers.
Mutation-sensitivity regressions require each such field to change checkpoint
identity. Observational-only safety flags are explicitly nonsemantic and are
applied after the gameplay state is restored.

A rejected settlement is absent from the durable ledger and therefore cannot
be reconstructed from ledger provenance alone. Attempt replay consumes an
explicit non-authoritative attempt record containing at least:

```text
source/module version hashes
compiled-program digest
constraint-registry digest
base-world digest
input or exact event request
runtime-feature fingerprint and constraint-limit profile
capability profile identity
```

The debugger or test harness may serialize an ephemeral rejection beside that
attempt record for comparison. Neither the attempt record nor rejection becomes
an authoritative committed-world event. Equivalence compares canonical typed
outcomes while ignoring opaque allocation and record identifiers.

Once implemented, capability negotiation reports:

```json
{
  "causal_laws": 1,
  "causal_constraints": 1,
  "constraint_limits": {
    "version": 3,
    "fingerprint": "<sha256>",
    "fuel_per_invocation": 100000,
    "max_heap_bytes_per_invocation": 1048576,
    "max_aggregate_fuel": 1000000000,
    "max_aggregate_heap_bytes": 1073741824,
    "max_violations_per_invocation": 256,
    "max_violations_per_settlement": 4096,
    "max_serialized_outcome_bytes": 1048576
  }
}
```

A host must not infer constraint support solely from `causal_laws`. The numeric
values above are the proposed default v0 profile; an implementation may expose
another supported finite profile, but it must advertise and freeze that profile
before settlement execution and include it in attempt replay.

## Compile-time diagnostics

Diagnostics should teach the validation-only model.

```text
Error: constraint `WithinWorldBounds` cannot call `next`

Constraints validate one immutable complete candidate and cannot modify it.

help: compute the final candidate in the owning resolver, or reject it with
      a `require ... else "stable.code"` statement.
```

```text
Error: `candidate` is only valid inside a constraint
```

```text
Error: constraint `ValidMotion` reads candidate `Velocity` without declaring
`watches Velocity`

Candidate dependencies must be explicit so trigger selection remains stable.
```

```text
Error: candidate reads in v0 must target constraint subject `subject`

Cross-entity candidate dependencies require a later RFC.
```

```text
Error: constraint `ForeignBounds` must be declared in the module that defines
component `Position`

Component constraints form part of the component's stable invariant set.
```

```text
Error: constraint `WithinWorldBounds` uses violation code
`position.x_above_world_max` more than once

Violation codes must be unique within one constraint.
```

```text
Error: constraint requirement must have type bool, found float
```

```text
Error: constraint violation code must be a non-empty string literal
```

```text
Error: constraint `NonPenetration` cannot call `emit`

Constraint evaluation is read-only and cannot publish effects from a
transition that may still abort.
```

Full transitive effect diagnostics remain required. A constraint calling a
readonly-looking helper that eventually performs I/O or mutation must show the
entire forbidden call chain.

## Runtime architecture

RFC-0002 extends the settlement kernel without turning constraints into systems
or resolver stages. Concepts are equivalent to:

```text
ConstraintRegistry
CandidateView
ConstraintInvocation
ConstraintLimits
ConstraintOutcome
ConstraintViolation
ConstraintEvaluationError
SettlementRejection
VmFailure
AttemptRecord
```

Responsibilities remain separated:

- parser/checker/compiler constraint modules own syntax and effects;
- the RFC-0001 settlement module owns the phase transition and atomicity;
- a dedicated constraint runtime module selects invocations and collects
  deterministic outcomes;
- `CandidateView` owns read-only sparse-overlay lookup;
- each invocation owns an isolated frame/stack checkpoint, fuel budget, local
  outcome buffer, and capability context;
- the typed host boundary owns rejection/runtime/host failure separation;
- the capability renderer owns filtered ephemeral rejected-transition trees;
- attempt replay owns non-authoritative rejection reproduction; and
- the durable causality ledger remains unchanged for failed settlements.

Do not implement validation by letting constraints mutate a temporary world or
by appending then rolling back durable provenance.

## Dogfood vertical slice

Create a separate project:

```text
projects/dogfood/causal-constraints/
```

It contains one moving body receiving velocity, wind, and knockback proposals.
`ResolveDisplacement` stages one `Position`. `WithinWorldBounds` and
`NonPenetration` independently validate the completed candidate.

Required demonstrations:

### Valid movement

All producer permutations commit the same position and durable provenance.

### Multiple violations

One candidate violates both constraints. Reordering constraint declarations
or internal invocation produces the same structured violation list and bounded
ephemeral explanation.

### Complete candidate visibility

A constraint reads a second component staged by the same settlement and sees
its candidate value, while ordinary `get` sees its base value.

### Watched-component triggering

`ValidMotion for Position watches Velocity` runs when only `Velocity` is
staged, receives base-fallback candidate `Position`, and runs once—not twice—
when both `Position` and `Velocity` are staged.

### Atomic failure

On violation or constraint evaluation error:

```text
world digest unchanged
durable ledger unchanged
no active settlement
same VM can execute a later valid settlement
```

### Event bridge and replay

A handler-triggered settlement retains event ancestry on success. A rejected
settlement returns ephemeral proposal ancestry and commits no ledger records.
Ledger replay reproduces commits; attempt replay reproduces the same canonical
rejection from the same versioned request, base digest, limits, and capability
profile, modulo opaque IDs.

### Sandbox

Candidate reads excluded by the execution read ACL and candidate writes
excluded by the write ACL both fail closed without changing the fork. A
recipient with narrower rendering capability receives useful stable codes and
redacted placeholders without hidden candidate or proposal payloads.

## Semantic fixtures

Before runtime implementation, fixtures must specify:

- valid declaration and requirement syntax;
- constraints declared on foreign components;
- every forbidden direct and transitive effect;
- base reads versus complete candidate reads;
- attached-component and watched-component triggering;
- one invocation when several watched triggers are staged;
- multiple constraints on one component;
- canonical multiple-violation ordering;
- duplicate violation preservation;
- ordinary evaluation failure collection;
- isolated per-invocation fuel exhaustion;
- per-invocation and settlement-wide violation/output caps;
- host-fatal abort with no partial outcomes;
- constraint-safe helper/builtin closure and native-call rejection;
- atomic abort and VM reuse;
- feature-gate rejection;
- ledger replay versus failed-attempt replay equivalence;
- sandbox read/write ACL behavior; and
- deterministic capability redaction of values and origins.

The fixtures and RFC form the semantic contract. No runtime shortcut may
weaken them to declaration-order evaluation or first-error-wins behavior.

## Implementation sequence

### Milestone A — Executable semantic contract

Add parser/checker snapshots and runtime fixtures under a dedicated
feature-gated constraint directory. Lock diagnostics and canonical rejection
output before implementing the runtime.

### Milestone B — Front end and tooling

Add the declaration and contextual statements to the authoritative Rust path:

```text
lexer
parser / AST
formatter
checker / transitive effects
compiler
LSP syntax, symbols, completion, and hover
```

Do not represent constraints as unrestricted functions with runtime-only
checks.

### Milestone C — Candidate view and runtime

Add an immutable sparse overlay and a dedicated constraint evaluator after
candidate conflict/ACL validation but before candidate world application.
Select attached and watched triggers once per subject. Run every invocation
with an isolated frame/stack checkpoint, deterministic fuel budget, local
bounded outcome buffer, and capability context. Collect all semantic outcomes,
canonicalize them, and commit only when the collection is empty. No constraint
may observe another invocation's progress or outcome.

### Milestone D — Diagnostics and host surfaces

Add typed `SettlementRejected`, `RuntimeError`, and `HostFault` boundaries to
CLI, sandbox, attempt replay, WASM, and wire hosts, with string compatibility
wrappers. Apply deterministic capability filtering before returning rejection
data. Bound rendering while retaining the authorized full structure for its
ephemeral lifetime. Do not add failed outcomes to `why()` or the durable ledger,
and do not add a mutable `last_rejection` side channel.

### Milestone E — Dogfood and release gate

Complete the movement project, permutation/property tests, failure injection,
fuzzing, and benchmarks. Publish under `--experimental-laws` only after all
acceptance criteria pass.

The required permutation property is:

```text
same base + same proposal multiset + same selected constraints + same limits
    ⇒ same world digest or same canonical structured rejection,
       independent of declaration and execution order
```

## Performance requirements

Measure separately:

```text
candidate overlay lookup
constraint selection
constraint execution
watched-trigger selection and deduplication
isolated invocation checkpoint/fuel accounting
violation allocation
canonical violation sorting
ephemeral explanation rendering
capability-filtered rendering
attempt-replay encoding
successful commit after validation
failed settlement cleanup
```

Sweep:

```text
1, 10, 100, 1,000, 10,000 candidate writes
one versus many constraints per component
one versus many watched triggers per subject
zero versus many violations
ordinary failure versus fuel/output-limit failure
base fallback versus staged candidate lookup
small versus large proposal fan-in origins
```

Memory must remain proportional to touched candidate entries, selected
constraint invocations, violations, and retained ephemeral origins. Untouched
archetypes remain shared.

## Drawbacks

- Component-defining modules gain responsibility for stable invariants.
- Complete candidate lookups add runtime and checker complexity.
- Explicit watches add declaration maintenance but make invalidation auditable.
- Collecting every violation costs more than first-error short-circuiting.
- Isolated fuel and outcome buffers cost more than one shared validation budget.
- Ephemeral fan-in explanations can be large without bounded rendering.
- Transition-only validation does not prove that old worlds are globally valid.
- Validation without repair may require applications to propose a different
  action or run a later explicit settlement.

These costs are intentional. First-error behavior and mutation-based repair
would make execution order semantically observable.

## Alternatives considered

### Ordered mutating constraints

Rejected. Allowing constraints to rewrite candidates in priority or declaration
order recreates an imperative schedule and makes invariants interfere.

### Projection in v0

Rejected. Two projectors correcting one component need semantic ownership or a
typed correction algebra. That requires a later RFC.

### Resolver-local validation only

Rejected. It duplicates invariants and cannot reliably inspect the complete
cross-resolver candidate.

### First failing constraint wins

Rejected. The visible error changes with execution order and hides independent
violations.

### Constraints attach from any importing module

Rejected for v0. Importing a module would silently alter another component's
commit contract. Same-module declarations keep the invariant set reviewable.

### Durable provenance for failed settlements

Rejected. A settlement that does not commit must not mutate the main ledger.
The returned ephemeral explanation carries the necessary evidence.

### General solver or fixed-point engine

Rejected. It expands termination, performance, and explanation semantics far
beyond validation of one completed patch.

## Explicit v0 non-goals

- constraint `before`, `after`, or priorities;
- constraint writes, projection, repair, or correction proposals;
- constraint-to-constraint communication;
- short-circuit selection of the first violation;
- proposal or event emission;
- resolver reads of candidate state;
- fixed-point or multi-round evaluation;
- candidate ECS queries or candidate resource reads;
- cross-entity candidate reads or watches;
- inferred watch dependencies;
- global/keyless constraints;
- constraints on untouched world state;
- structural changes, spawn, or despawn;
- parallel constraint execution guarantees;
- catchable in-language settlement rejection values;
- arbitrary native or FFI calls from constraints;
- shared order-sensitive fuel or output budgets;
- mutable `last_rejection` host side channels;
- durable failed-transaction provenance;
- GPU lowering or AOT optimization; and
- support in the frozen C backend.

Parallel execution may be a compatible runtime optimization later because
inputs and outputs are immutable, but RFC-0002 does not promise it.

## Release acceptance gate

The experimental feature is complete only when:

1. Every constraint reads the same immutable base and complete candidate view.
2. Attached plus explicitly watched same-entity components form the trigger set;
   staging several triggers still invokes the constraint once per subject.
3. No constraint can write, propose, emit, perform I/O, mutate persistent VM
   state, call arbitrary native code, or use nondeterminism.
4. Constraint declaration and execution order cannot alter commit or rejection.
5. Every applicable constraint is evaluated within an isolated deterministic
   resource contract; failures are not first-error-wins.
6. Violations and evaluation failures have canonical structured ordering, and
   limit failures expose no order-dependent partial prefix.
7. `candidate(...)` is restricted to the subject and declared trigger types,
   returns staged values with base fallback, and cannot observe partial patches.
8. Any rejection or host abort leaves world and durable ledger byte-identical
   to their pre-settlement state.
9. Every public failure return leaves no active settlement, and the VM remains
   reusable whenever the host returns control.
10. Successful settlements preserve RFC-0001 provenance unchanged.
11. Failed settlements return typed, bounded ephemeral causal explanations and
    commit no provenance or mutable side-channel state.
12. Ledger replay reconstructs commits; attempt replay reconstructs canonical
    rejection from the same versioned request, base digest, limits, and caps.
13. Sandbox read/write ACLs cannot be bypassed, and narrower recipients receive
    deterministic redaction without hidden-value ordering leaks.
14. Existing RAD programs, snapshots, and causal settlements behave unchanged.
15. Memory remains proportional to candidate writes, selected constraints, and
    bounded outcomes; untouched archetypes are not deep-cloned.

The product thesis to protect is:

> A constraint is not another resolver or schedule stage. It is an independent
> judgment over one complete causal candidate.

## Implementation status

The initial semantic blockers are resolved normatively:

1. `candidate` is the v0 contextual read spelling.
2. Stable violation codes are the v0 user-authored payload; optional human
   messages are deferred.
3. Candidate reads and explicit watches are same-entity only.
4. Typed detailed host failures coexist with string compatibility wrappers.
5. Every semantic invocation uses isolated limits and returns an outcome;
   true host-fatal faults expose no partial collection.
6. Ledger replay covers commits, while attempt replay covers rejections.
7. Internal rejection data is capability-filtered and deterministically
   redacted for each recipient.

The executable fixtures live under `tests/fixtures/causal-constraints/`; the
versioned limit-profile handshake is exposed by `runtime_features()`; the Rust
host surface returns typed `VmFailure` values; WASM returns tagged rejection
JSON; and capability-redaction, failed-attempt replay, permutation, fuel,
atomicity, and reusable-VM regressions are part of the implementation suite.
The movement vertical slice is in `projects/dogfood/causal-constraints/`.

The experimental implementation additionally uses one synchronized
transaction value-limit domain; preflights a separate aggregate host envelope;
gives each invocation an independent semantic fuel/heap meter; freezes each
rejected candidate detail once; streams canonical rejection bytes into an
exact bounded writer; emits a real `HostAborted` route; redacts origin identity
as one opaque value; follows only candidate-specific proposal fan-in; and binds
attempt replay to compiled-program, runtime-feature, constraint-registry,
limit-profile, capability, base-world, and request identities. Opaque
settlement IDs do not participate in semantic rejection equality. Constraint
fuel is charged per opcode, invocation allocations use disposable heaps,
settlement outcomes are metered before retention, and replay runs only on a
discarded, graph-isolated child timeline. Child closures rewrite captures onto
child-owned cells while preserving internal sharing and cycles; replay-visible
constant, event, and task roots are cloned in the same graph. Irreversible host
effects fail closed. Constraint-safe native builtins use conservative work and
peak-allocation quotes derived from their actual inputs. Admission is a closed,
enumerated whitelist, and dynamically allocating contracts are checked against
a per-thread peak allocator oracle that includes GC allocations and temporary
Rust collections. `find`, `max_by`, `min_by`, `reduce`, and every other helper
without that proof remain unavailable in constraints. Invocation-local
violation retention also has an independent byte meter before results reach
the settlement-wide outcome meter.

Attempt recording now captures the complete graph-isolated seed before the
host call. Recorded replay forks only that seed; portable replay requires a
matching canonical checkpoint digest. Replay graph cloning is node/byte
bounded, and cycle-breaking placeholders are re-accounted to the populated
object's retained size before the child is exposed.

The final execution-context closure preserves main-timeline semantics in the
observational child (including delayed-event behavior), rejects worker attempt
roots, and derives replay copying and checkpoint identity from the same
`AttemptReplayState`. Native admission is proof-registry-driven, while the
checked shared range plan makes boundary stepping total and count-bounded.

Projection, priorities, fixed points, and parallel constraint execution remain
out of scope and require follow-on RFCs.
