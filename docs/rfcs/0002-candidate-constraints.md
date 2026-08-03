# RFC-0002: Candidate Constraints—Order-Independent Settlement Validation

- **Status:** Draft
- **Product name:** RAD Causal Laws
- **Paradigm:** World-Law Programming
- **Author:** peteracs
- **Created:** 2026-08-03
- **Last updated:** 2026-08-03
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

A constraint is attached to one component type. Its parameters are:

- `subject`, whose type is `entity`; and
- `proposed`, whose type is the attached component.

The runtime invokes it once for every staged candidate write of that component.
Staging a value identical to the base value still counts as a candidate write
and therefore runs the constraint.

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
other components through a contextual read:

```rad
candidate(subject, Velocity)
```

`candidate(entity, Component)` has type `option<Component>` and reads the
immutable candidate world:

```text
if (entity, Component) is staged in the patch:
    return the staged value
otherwise:
    return the value from the immutable base snapshot
```

It never reads a partially validated result because constraints cannot modify
the view. Direct candidate lookup is supported in v0; candidate ECS queries,
candidate resource reads, and structural candidate changes are not.

Normal `get`, `require`, queries, and readonly helpers continue to read `base`.
The distinct `candidate(...)` spelling makes future-state reads visible during
review and in effect diagnostics.

Example:

```rad
constraint MovingBodyRemainsFinite for Position(subject, proposed) {
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

constraint NonPenetration for Position(subject, proposed) {
    let collider = require(subject, Collider)
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

### Constraint selection

For each `(entity, component type)` in the patch, the runtime selects every
constraint declared for that component type. Each selected constraint runs
exactly once with that entity and staged component value.

Constraints are not invoked for untouched components. RFC-0002 therefore
validates transitions, not every pre-existing fact in the world. A separate
world-audit operation, if useful, requires another RFC.

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

Pure and readonly helper functions may be called. They continue to read base.
Candidate components may be passed to pure helpers as ordinary values.
`candidate(...)` itself is contextual and may only appear directly inside a
constraint body in v0; candidate-aware helper effects are deferred.

### Effect rules

A constraint has the checked effect context:

```text
ReadECS + ReadCandidate + Violate
```

It may:

- read base ECS facts;
- read the immutable candidate overlay;
- perform local deterministic computation;
- call `pure fn` and `readonly fn` helpers;
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
```

Forbidden effects are rejected statically and transitively with the complete
effect call chain. Runtime checks remain defense in depth for malformed or
untrusted bytecode.

### Structured violations

Each failed requirement produces a value conceptually equivalent to:

```text
ConstraintViolation {
    settlement_id
    constraint_name
    component_type
    entity
    code
    requirement_source_span
    candidate_write_origin
}
```

`candidate_write_origin` refers ephemerally to the resolver and proposal fan-in
that staged the rejected component. The violation renderer can therefore say
both which invariant failed and which simultaneous causes produced the invalid
candidate.

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

### Evaluation failures

A constraint can fail while evaluating—for example, a base `require(entity,
Component)` may find a missing component. Since constraint effects are read-only,
the runtime evaluates every independently selected constraint and collects both
ordinary violations and evaluation failures before deciding the result.

Evaluation failures are structured separately from violated requirements and
are canonically ordered by entity, component, qualified constraint name, source
span, and stable error code. They always abort the settlement.

This avoids making “the first failing constraint” depend on execution order.

### Atomic abort and VM reuse

If any violation or evaluation failure exists:

```text
world == pre-settlement world
ledger == pre-settlement ledger
active settlement == none when the public execution boundary returns
```

Transient proposals, patches, candidate views, and constraint outcomes are
discarded. A failed constraint must not poison a reused VM.

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

Hosts may inspect structured outcomes and render a bounded tree. RAD source in
v0 observes the normal settlement runtime error; catchable constraint results
or durable failed-transaction history require separate design.

Repeated common proposal ancestry is collapsed, and large fan-in uses the same
bounded presentation policy as `why()`. Retention lasts only as long as the
returned error value or host response.

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

- constraints run only for candidate writes staged by a settlement;
- legacy writes outside a settlement do not invoke constraints;
- a handler may invoke a settlement and receives its constraint failure in the
  same way as another settlement runtime error;
- constraints cannot emit events, so a failed settlement never leaks events;
- replay deterministically reconstructs successful constraint decisions and
  reproduces failed outcomes without committing provenance;
- forks and simulation use the same constraint registry as their source VM;
- candidate reads and candidate write origins obey sandbox read/write ACLs;
- `settle` does not flush events before or after validation;
- nested settlements and parallel worker settlements remain rejected; and
- the frozen C backend does not parse or execute constraint syntax.

Once implemented, capability negotiation reports:

```json
{
  "causal_laws": 1,
  "causal_constraints": 1
}
```

A host must not infer constraint support solely from `causal_laws`.

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
ConstraintViolation
ConstraintEvaluationError
SettlementRejection
```

Responsibilities remain separated:

- parser/checker/compiler constraint modules own syntax and effects;
- the RFC-0001 settlement module owns the phase transition and atomicity;
- a dedicated constraint runtime module selects invocations and collects
  deterministic outcomes;
- `CandidateView` owns read-only sparse-overlay lookup;
- the diagnostic renderer owns ephemeral rejected-transition trees; and
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
settlement returns ephemeral proposal ancestry, commits no ledger records, and
replays to the same canonical rejection modulo opaque IDs.

### Sandbox

Candidate reads excluded by the read ACL and candidate writes excluded by the
write ACL both fail closed without changing the fork.

## Semantic fixtures

Before runtime implementation, fixtures must specify:

- valid declaration and requirement syntax;
- constraints declared on foreign components;
- every forbidden direct and transitive effect;
- base reads versus complete candidate reads;
- multiple constraints on one component;
- canonical multiple-violation ordering;
- duplicate violation preservation;
- runtime evaluation failures;
- atomic abort and VM reuse;
- feature-gate rejection;
- replay equivalence; and
- sandbox read/write ACL behavior.

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
Collect all outcomes, canonicalize them, and commit only when the collection is
empty.

### Milestone D — Diagnostics and host surfaces

Add structured ephemeral settlement rejection to CLI, sandbox, replay, WASM,
and wire host boundaries. Bound rendering while retaining the full returned
structure for its ephemeral lifetime. Do not add failed outcomes to `why()` or
the durable ledger.

### Milestone E — Dogfood and release gate

Complete the movement project, permutation/property tests, failure injection,
fuzzing, and benchmarks. Publish under `--experimental-laws` only after all
acceptance criteria pass.

## Performance requirements

Measure separately:

```text
candidate overlay lookup
constraint selection
constraint execution
violation allocation
canonical violation sorting
ephemeral explanation rendering
successful commit after validation
failed settlement cleanup
```

Sweep:

```text
1, 10, 100, 1,000, 10,000 candidate writes
one versus many constraints per component
zero versus many violations
base fallback versus staged candidate lookup
small versus large proposal fan-in origins
```

Memory must remain proportional to touched candidate entries, selected
constraint invocations, violations, and retained ephemeral origins. Untouched
archetypes remain shared.

## Drawbacks

- Component-defining modules gain responsibility for stable invariants.
- Complete candidate lookups add runtime and checker complexity.
- Collecting every violation costs more than first-error short-circuiting.
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
- global/keyless constraints;
- constraints on untouched world state;
- structural changes, spawn, or despawn;
- parallel constraint execution guarantees;
- catchable in-language settlement rejection values;
- durable failed-transaction provenance;
- GPU lowering or AOT optimization; and
- support in the frozen C backend.

Parallel execution may be a compatible runtime optimization later because
inputs and outputs are immutable, but RFC-0002 does not promise it.

## Release acceptance gate

The experimental feature is complete only when:

1. Every constraint reads the same immutable base and complete candidate view.
2. No constraint can write, propose, emit, perform I/O, or use nondeterminism.
3. Constraint declaration and execution order cannot alter commit or rejection.
4. Every applicable constraint is evaluated; failures are not first-error-wins.
5. Violations and evaluation failures have canonical structured ordering.
6. `candidate(...)` returns staged values with base fallback and cannot observe
   partial patches.
7. Any violation or evaluation failure leaves world and durable ledger
   byte-identical to their pre-settlement state.
8. Every public failure return leaves no active settlement, and the VM remains
   reusable.
9. Successful settlements preserve RFC-0001 provenance unchanged.
10. Failed settlements expose bounded ephemeral causal explanations and commit
    no provenance.
11. Replay reconstructs equivalent success or rejection outcomes.
12. Sandbox read/write ACLs cannot be bypassed through candidate validation.
13. Existing RAD programs, snapshots, and causal settlements behave unchanged.
14. Memory remains proportional to candidate writes, selected constraints, and
    outcomes; untouched archetypes are not deep-cloned.

The product thesis to protect is:

> A constraint is not another resolver or schedule stage. It is an independent
> judgment over one complete causal candidate.

## Unresolved questions

The following remain open during RFC review and must be resolved before the
semantic fixtures are accepted:

1. Should the contextual read be named `candidate`, `future`, or `proposed`?
2. Should v0 expose optional static human messages in addition to stable codes?
3. Should candidate reads be restricted to the current entity initially?
4. Which host API shape best preserves structured ephemeral rejection without
   changing existing string-based embedding calls?
5. Should constraint evaluation collect all runtime failures, or stop only for
   resource exhaustion and other host-fatal conditions?

These questions may refine surface syntax and host representation. They must
not weaken the immutable-input, canonical-output, no-ordering semantics.
