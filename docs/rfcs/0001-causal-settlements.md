# RFC-0001: Causal Settlements—Typed Intents, Laws, and Resolvers

- **Status:** Accepted for experimental implementation
- **Product name:** RAD Causal Laws
- **Paradigm:** World-Law Programming
- **Author:** peteracs
- **Created:** 2026-08-03
- **Last updated:** 2026-08-03
- **Feature gate:** `--experimental-laws`

## Summary

RAD Causal Laws adds an opt-in causal layer above the existing ECS. Read-only
`law` declarations propose transient, typed `intent` values. Each intent has
one same-module `resolver`, which runs once per distinct entity key and stages
component replacements with `next`. A `settle` block collects all proposals,
resolves them against one immutable base snapshot, rejects overlapping
candidate writes, and commits the resulting sparse patch atomically.

The defining rule is:

> All laws read one snapshot. All proposals are collected. Every intent type
> has one owning resolver. All resolvers produce an isolated candidate patch.
> Conflicting candidate writes are errors. The patch commits atomically.

Resolver execution order never carries program meaning. A settlement denotes
simultaneous causes, not another schedule.

## Product objective

The first release proves one property:

> Several independent causes can propose changes to the same entity, and RAD
> can deterministically explain how those proposals became one atomic state
> transition—without relying on producer execution order.

The vertical slice is:

```text
committed world
      ↓
one immutable snapshot
      ↓
read-only laws propose typed intents
      ↓
one resolver owns each intent type
      ↓
resolvers create a candidate patch
      ↓
conflicts are rejected
      ↓
atomic commit
      ↓
why() explains the fan-in
```

This RFC does not replace systems, events, schedules, or direct ECS mutation.
It introduces an explicit causal transaction for rules that benefit from
order-independent fan-in and first-class explanation.

## Syntax

### Intent declarations

```rad
intent Damage {
    key target: entity
    source: entity
    amount: int
    kind: str
}
```

An intent declaration has these v0 restrictions:

- Exactly one field is marked `key`.
- The key has type `entity`.
- Every field has an explicit type.
- Values are transient and never become components, resources, events, or
  serializable world rows.
- Duplicate proposals are preserved.
- An intent and its owning resolver are declared in the same source module.

Composite keys, resource keys, keyless/global intents, defaults, and
serialization are reserved for later RFCs.

### Law declarations

```rad
law DirectHit(
    source: entity,
    target: entity,
    amount: int,
    kind: str,
) {
    propose Damage {
        target: target,
        source: source,
        amount: amount,
        kind: kind,
    }
}
```

A law is a restricted, no-result function. It may:

- read the settlement's immutable world snapshot;
- perform local computation;
- call `pure fn` and `readonly fn` helpers; and
- issue `propose` statements.

A law may not mutate ECS state, emit or flush events, commit or load worlds,
merge forks, perform I/O, use randomness or clock time, call asynchronous code,
call another law in v0, or invoke `settle`. Law effects are checked statically
and transitively. A law may only be called lexically within a `settle` body.

### Resolver declarations

```rad
resolver ResolveDamage for Damage(target, proposals) {
    let health = require(target, Health)
    let shield = require(target, Shield)

    let raw_damage = proposals
        |> map(fn(p) { return p.amount })
        |> sum()

    let absorbed = min(shield.hp, raw_damage)
    let remaining = raw_damage - absorbed

    next(target, Shield {
        hp: shield.hp - absorbed,
    })
    next(target, Health {
        hp: max(0, health.hp - remaining),
        max: health.max,
    })
}
```

The resolver key parameter has the intent key's type. Its proposals parameter
has type `list<Intent>`. It runs once per distinct key and reads only the
settlement's base snapshot.

In v0, `next` may only replace an existing component on the current key entity.
One resolver invocation may stage a given component at most once. It cannot add
or remove components, spawn or despawn entities, write resources or another
entity, read candidate state, propose, emit, perform I/O, use randomness, or
call asynchronous code.

### Settlement blocks

```rad
settle {
    DirectHit(attacker_a, hero, 20, "physical")
    DirectHit(attacker_b, hero, 30, "fire")
    DirectHit(environment, hero, 5, "burn")
}
```

The whole block is one causal transaction. Reordering those law calls must
produce the same committed world and an equivalent causal explanation, modulo
opaque record identifiers.

## Normative semantics

### Snapshot boundary

On entry, the runtime captures the settlement base:

```text
base = immutable snapshot of current world
```

The settlement body, every law, every resolver, and every helper they call
reads `base`. A law cannot observe a proposal as world state. A resolver cannot
observe any resolver's candidate writes.

### Law phase

The settlement body performs local control flow and invokes laws. Each
`propose` appends a transient record containing:

```text
proposal id
intent type
entity key
typed payload
producing law
source span
enclosing settlement
enclosing causal origin
```

If a law fails, resolution never starts. The world and committed provenance
ledger remain unchanged.

### Grouping and canonicalization

Proposals are grouped first by intent type and then by key. Before a resolver
receives a proposal list, the runtime sorts that list by a canonical stable
encoding of the complete typed payload. Stable producer metadata is used only
as a tie-breaker for byte-identical payloads.

Therefore:

```text
same proposal multiset ⇒ same resolver input sequence
```

The resolver receives no hidden producer-order metadata. Priority, timestamps,
source rank, or semantic tie-breakers must be explicit payload fields.

### Resolver ownership

- Each used intent has exactly one resolver.
- The resolver is declared in the same module as the intent.
- Imported modules cannot override an intent's resolver.
- Multiple resolvers are a compile-time error.
- Proposing an intent with no resolver is a compile-time error.
- A resolver with no statically possible producer is allowed with a warning.

An intent and its arbitration semantics form one stable API.

### Candidate phase

Resolvers stage a sparse patch, not mutations to a temporary world:

```text
CandidatePatch:
    (entity, component type)
        → new component value
        → resolver provenance
        → contributing proposal ids
```

The patch provides the future insertion point for constraints:

```text
base → proposals → resolution patch → future constraints → atomic commit
```

No constraint syntax or constraint solver is part of v0.

### Candidate conflicts

Two candidate paths writing the same `(entity, component type)` conflict. A
second `next` to the same component in one resolver invocation also conflicts.
There is no last-write-wins behavior and declaration order is irrelevant.

```text
Settlement aborted: conflicting candidate writes

`Health` of entity `hero` was written by:
  - resolver `ResolveDamage` for Damage(hero)
  - resolver `ResolveRegeneration` for Healing(hero)

No world state was changed.

help: combine these causes under one owning intent/resolver,
      or split them into two explicit settle boundaries.
```

### Atomic commit

After every resolver succeeds and the patch is conflict-free, the runtime:

1. validates every candidate write, including sandbox component-write ACLs;
2. applies the patch to a copy-on-write world;
3. appends settlement provenance records; and
4. atomically adopts the candidate world.

Any failure discards transient proposals and staged writes. The live world and
main provenance ledger remain byte-identical to their pre-settlement state.
Memory cost is proportional to proposals and touched candidate columns;
untouched archetypes are not deep-cloned.

## Resolver ordering is absent

The language does not support resolver `before` or `after` clauses. Resolvers
do not execute sequentially against candidate state. Declaration order is not
a conflict policy. The runtime may choose a canonical internal invocation order
for reproducibility and diagnostics, but it is unobservable because every
resolver reads the same base snapshot and writes an isolated patch.

Genuine temporal dependence is written as two settlements:

```rad
settle { /* one simultaneous causal transition */ }
settle { /* sees the first transition's committed result */ }
```

Within one settlement, causes are simultaneous. Between settlement blocks,
sequence is explicit.

## Interoperation with existing RAD

Causal Laws is opt-in. Existing systems, mutable parameters, accum resources,
events, handlers, schedules, forks, simulation, and commit preserve their
current behavior.

A synchronous event handler or ordinary synchronous ECS function may invoke a
settlement. The snapshot is captured exactly when execution reaches `settle`:

- legacy writes before it are visible;
- settlement writes are visible after it returns; and
- legacy writes after it occur later.

Additional rules:

- `settle` does not flush events;
- laws and resolvers cannot emit events;
- handlers cannot call `propose` directly;
- systems cannot be included in a settlement;
- laws cannot be scheduled in v0;
- `settle` cannot execute in a parallel system worker;
- nested settlements are rejected;
- sandbox candidate writes pass through the existing write ACL; and
- record/replay reconstructs settlement provenance by deterministic execution.

The frozen C backend does not implement or expose this feature. `core/vm` is
the authoritative implementation.

## Provenance and `why()`

Settlement provenance extends the existing ledger rather than creating a
second debugger. The ledger gains records equivalent to:

```text
SettlementRecord
ProposalRecord
ResolutionRecord
CandidateWriteRecord
```

A committed write points to one resolution, which points to every contributing
proposal. Each proposal points to its law and enclosing causal origin. Thus a
settlement invoked by an event handler retains the exact event-instance and
emitter ancestry.

The `why()` renderer presents a tree, collapses repeated ancestry, and bounds
large fan-in output while retaining every proposal reference under the normal
ledger retention policy.

## Diagnostics

Diagnostics teach the causal model. Required cases include:

- forbidden law effects, direct and transitive;
- `propose` outside a law and law calls outside `settle`;
- missing, duplicate, or cross-module resolver ownership;
- forbidden resolver effects and `next` outside a resolver;
- `next` targeting an entity other than the current key; and
- nested settlements and settlements in system workers.

The checker preserves full transitive effect call chains in these diagnostics.

## Implementation architecture

Implementation follows single-responsibility modules instead of extending the
already-large VM dispatcher:

- parser, checker, and compiler causal-law modules own syntax-specific logic;
- a settlement runtime module owns proposal buffers, grouping,
  canonicalization, candidate patching, validation, and atomic adoption;
- the causality ledger owns durable fan-in records and rendering; and
- the existing world remains the copy-on-write storage substrate.

Core runtime concepts are:

```text
SettlementContext
ProposalBuffer
IntentRegistry
ResolverRegistry
CandidatePatch
```

Experimental runtimes report `{ "causal_laws": 1 }`.

## Dogfood vertical slice

`projects/dogfood/causal-laws/` contains one damage scenario. A hero receives
physical, fire, and environmental burn damage in one settlement. The resolver
sums damage, consumes Shield, clamps Health at zero, and writes both components
atomically.

It demonstrates producer permutation independence, fan-in `why()` output,
atomic conflict failure, event ancestry, replay, and sandbox ACL enforcement.

## Explicit v0 non-goals

- constraints or constraint syntax;
- resolver ordering or candidate reads;
- laws as query systems or automatic law scheduling;
- parallel settlement execution;
- resolver-produced intents or fixed-point rounds;
- resource intents, composite keys, or global intents;
- spawn/despawn/add/remove through `next`;
- cross-entity writes;
- GPU or AOT lowering;
- user-defined algebra traits; and
- automatic conversion from `accum`.

## Release acceptance gate

The experimental feature is complete only when:

1. producer call order cannot alter the dogfood result;
2. every law and resolver reads the same base snapshot;
3. no resolver can observe candidate state;
4. every used intent has exactly one same-module resolver;
5. candidate conflicts abort rather than using last-write-wins;
6. resolver failure leaves the world byte-identical;
7. `why()` names every proposal and producing law;
8. event ancestry remains connected;
9. replay reconstructs the same causal result;
10. sandbox ACLs cannot be bypassed;
11. existing RAD programs and tests remain unchanged; and
12. untouched archetypes stay shared by copy-on-write.

> A settlement is not another schedule. It is a declaration that several
> causes are simultaneous, and that one explicit semantic owner determines
> their result.
