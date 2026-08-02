# RAD Causal Laws (experimental)

RAD Causal Laws is an opt-in causal layer above the existing ECS. It is the
first implementation slice of
[RFC-0001](https://github.com/peteracs/rad/blob/main/docs/rfcs/0001-causal-settlements.md).
Enable it with `--experimental-laws`:

```powershell
rad projects/dogfood/causal-laws/main.rad --experimental-laws
```

The feature is implemented only by the authoritative Rust VM. The frozen C
backend does not parse or execute this syntax.

## The model

A settlement is one atomic causal transition:

```text
base world snapshot
  -> read-only laws produce typed proposals
  -> proposals group by intent and entity key
  -> one resolver owns each intent type
  -> resolvers stage isolated candidate writes
  -> conflicts are rejected
  -> the complete patch commits atomically
```

Every law and resolver reads the same base snapshot. A resolver cannot read a
candidate write from itself or another resolver. Resolver order therefore has
no program meaning; `before` and `after` are intentionally unavailable.

Within one `settle`, causes are simultaneous. Put genuinely sequential
transitions in separate `settle` blocks.

## Typed intents and laws

An intent has exactly one `key` field, and v0 requires that key to be an
`entity`:

```rad
intent Damage {
    key target: entity
    source: entity
    amount: int
    kind: str
}
```

Intent values are transient. Duplicate proposals are preserved; they never
become components, events, resources, or serialized world rows.

A law can read the settlement snapshot and propose intents, but cannot mutate
the world, emit events, perform I/O, use randomness, call another law, or start
a settlement:

```rad
law DirectHit(source: entity, target: entity, amount: int, kind: str) {
    propose Damage {
        target: target,
        source: source,
        amount: amount,
        kind: kind
    }
}
```

Laws may be invoked only inside `settle`:

```rad
settle {
    DirectHit(attacker_a, hero, 20, "physical")
    DirectHit(attacker_b, hero, 30, "fire")
}
```

The runtime canonicalizes proposal lists by typed payload rather than producer
execution order. If arbitration needs a priority or sequence number, make it
an explicit intent field.

## One owning resolver

Every proposed intent has exactly one resolver, declared in the intent's
module. The resolver runs once for each distinct key and receives every
proposal for that key:

```rad
resolver ResolveDamage for Damage(target, proposals) {
    let health = require(target, Health)
    let shield = require(target, Shield)
    let raw = proposals |> map(fn(p) { return p.amount }) |> sum()
    let absorbed = min(shield.hp, raw)

    next(target, Shield { hp: shield.hp - absorbed })
    next(target, Health {
        hp: max(0, health.hp - (raw - absorbed)),
        max: health.max
    })
}
```

In v0, `next` replaces an existing component on the resolver's current key
entity. It cannot add/remove components, write resources or another entity,
spawn/despawn, emit/propose, or stage the same component twice in one resolver
invocation.

If two resolution paths stage the same `(entity, component type)`, the
settlement aborts. There is no last-write-wins fallback. Any law/resolver error,
candidate conflict, or sandbox ACL denial leaves both the world and the main
provenance ledger unchanged.

## Existing RAD code

The feature is opt-in and does not alter systems, schedules, handlers,
`accum`, forks, or simulation. A synchronous handler or ordinary ECS function
may start a settlement. Writes before it are visible in the captured snapshot;
the atomic patch is visible after it returns. Settlements do not flush events,
cannot be nested, and cannot run inside a parallel system worker.

## Causal explanations

`why(entity, Component)` renders a tree for settled writes. The write points to
its resolver, which points to every proposal and producing law, which in turn
retains the ordinary handler and exact-event ancestry.

The ledger retains every proposal reference under the normal retention policy.
The default renderer shows eight proposals and reports the omitted count for
larger fan-ins. Replay and fork wire provenance preserve the same tree, modulo
internal record IDs.

## v0 boundaries

Constraints, resolver ordering, resolver-to-resolver reads, resource or
composite-key intents, derived intents, fixed-point evaluation, structural
world changes through `next`, cross-entity writes, and parallel settlement
execution are deliberately outside RFC-0001.
