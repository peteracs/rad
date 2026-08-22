# Authority, transactions, and native data

This page is normative for the first six production-boundary features. The
examples are exercised by PagerGrid through BookCore under
`projects/dogfood/`.

## 1. Transitive effect authority

### Guarantee

Every callable has one inferred record containing state reads/writes, emitted
events, I/O, async reachability, unbounded calls, allocation reachability, and
query costs. A system declaration is an enforced upper bound on its complete
synchronous effect set. The graph follows ordinary calls, imported functions,
statically bounded callbacks, transitions, and handlers made synchronous by
`flush_events()`.

Queued handlers are separate authority roots until an explicit flush makes
them synchronous. System-root reports are call-site-specialized; a generic
helper report may conservatively union callback targets, but enforcement and
scheduling do not use that union for a concrete caller.

```rad
system RouteCritical(
    incident: mut Incident,
    reads IncidentTimeline,
    writes IncidentTimeline,
    emits ResponderPaged,
    io true,
    async true,
) {
    audit.append_timeline(self, "selected")
    emit ResponderPaged { incident: self }
    flush_events()
}
```

Undeclared effects fail at the system and include the causal call path:

```text
System 'RouteIncident' exceeds its declared authority:
writes [IncidentTimeline]

authority path:
RouteIncident -> audit.append_timeline
```

Unknown dynamic dispatch is rejected; it is not silently granted wildcard
authority. Indexed entity names are narrow shared state:
`get_entity`/`require_entity` read `$entity_names`, `name_of` reads
`$entity_identity`, and named publication/removal writes those authorities.

### Inspection

```bash
rad effects RouteCritical --file projects/dogfood/pagergrid/main.rad --json
rad path RouteCritical '->' audit.append_timeline --file projects/dogfood/pagergrid/main.rad
rad writers IncidentTimeline --file projects/dogfood/pagergrid/main.rad
rad readers OnCallAssignment --file projects/dogfood/pagergrid/main.rad
```

These commands query the checker product's cached graph and reverse indexes;
they do not rescan function bodies.

### Non-guarantees and migration

Effect inference constrains what encoded code can do; it does not infer which
business transition is correct. Existing systems must add exact grants rather
than wildcard authority. Checker products are opaque, source/configuration
fingerprinted semantic products and cannot be mixed with another program or
mutated by API callers.

PagerGrid is the acceptance service. Its benchmark routes 50,000 alerts while
preserving independent callback effects and deterministic scheduler conflicts.

## 2. Component, field, and resource ownership

### Guarantee

`owned` is semantic write ownership. Only the declaring canonical module may
spawn, set, remove, update, or dynamically write the owned state. Imports that
spell the same normalized path differently still identify one owner and one
runtime population.

```rad
pub component Inventory owned with [warehouse_migration] {
    indexed sku: str,
    available: int,
    reserved: int,
}

pub component ProductProfile {
    description: str,
    owned reorder_level: int,
}

pub resource InventoryRevision owned { value: int = 0 }
```

`with [...]` names explicit co-owner modules. A typed replacement passed
through locals or helper parameters does not conceal its component identity.
Field ownership applies to update blocks and `write_field` as well as complete
replacement values.

Outside the owner:

```rad
set(stock, Inventory { available: 0 })
```

fails with the owner and public repair boundary:

```text
Ownership violation: shipping cannot set 'Inventory'.
Inventory is write-owned by inventory_owner.
```

Read-only public functions remain ordinary imports. Test capabilities are
lexically scoped to test execution and are not production module exports.

### Non-guarantees and migration

Ownership does not make a multi-write operation atomic; use a transaction for
that. It also does not prevent reads. Move authoritative state declarations and
their mutation operations into one module, name unavoidable co-owners, then
replace outside writes with owner API calls.

FulfillOS proves whole-component, field, resource, co-owner, typed-replacement,
and test-scope behavior across 10,000 orders.

## 3. Atomic contract transactions

### Guarantee

A stable `transaction` observes one base world, stages every authoritative
write in an undo journal, checks all postconditions, and commits all or none.
Its patch may include components, resources, spawn/despawn, names, indexes,
views, and provenance. One successful commit has one transaction cause.

```rad
transaction CapturePayment(payment: entity) {
    requires has(payment, AuthorizedPayment)
    changes_only [AccountBalance, PaymentHold, CapturedPayment, LedgerEntry]

    remove(payment, PaymentHold)
    set(payment, CapturedPayment {})

    ensures has(payment, CapturedPayment)
    ensures ledger_is_balanced(payment)

    post_commit { emit PaymentCaptured { payment: payment } }
}
```

`changes_only` is checked transitively through helpers. Preconditions execute
before the first staged mutation. Failed preconditions/postconditions restore
the exact allocator, entity, name, component, resource, index, view, event, and
provenance state. `post_commit` may perform declared external effects but cannot
extend the authoritative patch.

```text
Transaction 'Broken' failed postcondition ensures #1
Staged changes were rolled back; no state was committed.
```

The runtime's differential journal suite compares randomized commit/rollback
histories against full snapshots, including repeated writes, spawn/despawn,
resource changes, names, and allocator generations.

### Non-guarantees and migration

A transaction does not roll back an external system contacted before commit.
Perform fallible host work before authoritative writes when safe, or model it
as a post-commit operation with idempotent recovery. Replace procedural cleanup
sequences with explicit `requires`, exact `changes_only`, and observable
`ensures` clauses.

ClearPay proves rollback, idempotency, balanced ledgers, post-commit delivery,
and one-cause provenance for 10,000 captures.

## 4. Native-fidelity and opaque nominal types

### Guarantee

Opaque aliases preserve semantic identity through values, containers, maps,
snapshots, replay, and native calls. Equal representations are not implicitly
convertible.

```rad
opaque type DeviceId = u32
opaque type SessionId = u32

enum AlarmCode: u8 { None = 0, Overheat = 1, EmergencyStop = 7 }
bitflags DeviceStatus: u16 { Online = 1, Calibrated = 2, Fault = 8 }

repr(C) packed struct TelemetryFrame {
    device: DeviceId,
    sequence: u32,
    status: DeviceStatus,
    temperature: f32,
    checksum: u16,
}
```

RAD provides `u8/u16/u32/u64`, `i8/i16/i32/i64`, `f32/f64`, exact checked
conversions, enum discriminant validation, non-overlapping primitive bitflags,
`size_of`, `offset_of`, `repr(C)`, `packed`, and little/big-endian codecs.

```text
Expected DeviceId, found SessionId.
Both use u32 representation but are distinct opaque types.
```

### Non-guarantees and migration

`repr(C)` follows the declared ABI contract; it does not guess a third-party
compiler's undocumented packing. Validate sizes and offsets against the native
contract. Replace representation-only aliases with opaque types at identity
boundaries and insert explicit checked conversions where domain crossing is
intentional.

ForgeLink decodes and re-encodes captured frames byte-for-byte, including
overflow, signed minima, `f32`, endian, enum, bitflag, and packed-layout cases.

## 5. Transactional materialized views

### Guarantee

A materialized view is runtime-owned derived state. Membership and keys update
in the same commit as declared source dependencies; readers cannot mutate or
rebuild it.

```rad
materialized view SellableProducts {
    depends [Product, Price, Inventory, SellerStatus, ComplianceRestriction]
    key Product.product_id
    where Product.active == true
      and Inventory.available > 0
      and SellerStatus.active == true
      and ComplianceRestriction.blocked == false
}
```

Dependencies and referenced fields are validated. An unrelated source field
does not advance the view revision. Same-key replacement, removal, re-entry,
ordered changes, and dependency revisions are deterministic and part of
transaction rollback.

```rad
lookup(SellableProducts, product_id)
revision(SellableProducts)
changes_since(SellableProducts, prior_revision)
why_in_view(SellableProducts, product)
why_not_in_view(SellableProducts, product)
```

### Non-guarantees and migration

Views do not infer arbitrary host-side dependencies. Every authoritative source
fact must be a component/resource dependency. Replace reader-side scans and
caches with a view, then remove the reader-side cache and its revision code.

MarketLens maintains one million catalog products and 50,000 source updates
without a steady-state full-catalog rebuild.

## 6. Ordered indexes

### Guarantee

`ordered indexed` fields provide deterministic numeric or lexicographic key
order without sorting the population at read time.

```rad
component OrderIdentity {
    indexed order_id: OrderId,
    ordered indexed priority: (PriceTicks, Sequence),
}
```

Available operations are `first`, `last`, `lower_bound`, `upper_bound`,
`range`, `next`, and `previous`. Bounds use ascending key order; tuple keys are
lexicographic; equal-key buckets use stable entity order. Erase-miss is a no-op,
replacement removes the old key before publishing the new key, and snapshot/
replay preserve traversal. Mutating the indexed field invalidates an old
position; callers request a new bound rather than retaining an iterator.

### Non-guarantees and migration

An ordered index is not insertion order and does not normalize signed values to
unsigned. Encode side-specific exchange priority explicitly. Replace
`sort(entities(...))` hot paths with an ordered key whose semantics are named in
the schema.

BookCore replays one million orders with deterministic price-time fills and no
per-match sort.
