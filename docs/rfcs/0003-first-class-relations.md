# RFC-0003: First-Class Facts, Relations, and Derived Facts

- **Status:** Draft
- **Product name:** RAD World Facts
- **Paradigm:** World-Law Programming
- **Author:** peteracs
- **Created:** 2026-08-04
- **Depends on:** [RFC-0001](0001-causal-settlements.md), [RFC-0002](0002-candidate-constraints.md)
- **Proposed feature gate:** `--experimental-relations`

## Summary

RFC-0003 makes typed facts and relations part of RAD's programmer-visible
world model. Authoritative relation facts participate in the same atomic
candidate transaction as components. Nonrecursive derivation rules compute
read-only facts from the complete candidate world. Constraints may inspect
those derived facts before commit, and provenance explains every derivation
back to authoritative facts and causal settlements.

The first version is deliberately narrow:

```text
authoritative facts at T
        +
component state at T
        ↓
causal proposals and resolvers
        ↓
complete authoritative candidate
        ↓
nonrecursive derived facts
        ↓
validation-only constraints
        ↓
one atomic commit or rejection
```

This RFC does not introduce recursive logic, unrestricted negation, rule
priorities, fixed points, or an optimizer that changes observable semantics.

## Motivation

ECS storage is a useful implementation target, but it is not a complete world
model. Relationships such as ownership, location, allegiance, visibility, and
dependency are awkward when programmers must encode every edge manually as a
component and reconstruct joins in imperative loops.

RAD should let a program state what is true:

```rad
relation Owns(owner: entity, item: entity)
    unique item

relation LocatedIn(subject: entity, place: entity)
    unique subject

relation AlliedWith(left: entity, right: entity)
    symmetric
```

and state what follows:

```rad
derive TotalWeight(person, sum(weight))
    when Owns(person, item)
     and ItemWeight(item, weight)

derive Encumbered(person)
    when TotalWeight(person, total)
     and CarryCapacity(person, capacity)
     and total > capacity
```

Laws and constraints then consume those facts without knowing whether the
runtime uses archetypes, hash indexes, sorted columns, or another storage
strategy.

## Terminology

- **Authoritative fact:** a stored relation tuple changed only through the
  causal transaction boundary.
- **Derived fact:** a read-only tuple justified by one or more rule proofs.
- **Relation schema:** a typed tuple shape plus canonical invariants such as
  uniqueness or symmetry.
- **Support:** an authoritative or derived fact used by one derivation proof.
- **Proof alternative:** one canonical support set that derives the same fact.
- **Candidate fact view:** the complete authoritative candidate plus all
  derived facts recomputed from it.

## Relation declarations

The v0 declaration surface is:

```rad
relation Owns(owner: entity, item: entity)
    unique item

relation AlliedWith(left: entity, right: entity)
    symmetric
```

Normative rules:

1. Relation and column names use normal module qualification and visibility.
2. A row is a typed tuple with fixed arity.
3. `unique column` means at most one row may exist for each value of that
   column in the complete candidate.
4. Several `unique` clauses may be declared and are checked independently.
5. `symmetric` is valid only for a binary relation whose column types match.
   Storage canonicalizes `(a, b)` and `(b, a)` to one row.
6. Duplicate insertion is idempotent. It does not create duplicate
   provenance or alter multiplicity.
7. Relation iteration and serialization use canonical tuple order.

Relation constraints are schema invariants, not declaration-order checks.
Every candidate is judged as a set of rows.

## Authoritative updates

Relation updates are causal writes. A resolver stages typed operations:

```text
Insert(relation, canonical tuple)
Remove(relation, canonical tuple)
ReplaceUnique(relation, unique key, canonical tuple)
```

The owning resolver is selected through the RFC-0001 intent registry. Laws do
not mutate relations directly. Resolver patches remain isolated and cannot
observe another resolver's relation writes.

Candidate conflicts are defined over canonical relation keys:

- incompatible operations on the same row conflict;
- two different rows claiming the same unique key conflict;
- symmetric aliases refer to the same canonical row;
- identical idempotent insertion may be coalesced only when its provenance
  fan-in remains complete and canonical.

Relation and component patches join one global conflict check and one atomic
world adoption. There is no separate relation commit.

## Derived facts

V0 derivations are positive, nonrecursive, and stratified by a finite
dependency DAG. The checker rejects every cycle between derived relations.

Allowed operations are:

- typed relation scans;
- equality joins;
- deterministic scalar predicates;
- projection;
- duplicate elimination;
- grouping with explicitly admitted deterministic aggregates.

The initial aggregate set is `count`, `sum`, `min`, and `max` over bounded
numeric values. Empty-group behavior and overflow are defined by the aggregate
type, never by host iteration order.

Not allowed in v0:

- recursive rules;
- negation over an open world;
- mutation, proposals, events, I/O, clocks, randomness, tasks, native calls,
  or FFI;
- rule priorities or declaration-order tie breaking;
- a derived fact reading a candidate constraint outcome.

For one authoritative candidate, derivation has one semantic result
independent of rule registration or execution order.

## Transaction phase ordering

RFC-0003 inserts derivation after all authoritative candidate writes have
passed conflict, type, schema, and write-capability checks:

```text
base snapshot
  → proposals
  → isolated resolver patches
  → component/relation conflict and schema checks
  → complete authoritative candidate
  → derived-fact maintenance
  → candidate constraints
  → atomic commit or rejection
```

Constraints read one immutable `CandidateView` containing both authoritative
and derived candidate facts. A constraint cannot observe partially maintained
indexes or another constraint's result.

Any derivation error or resource exceed rejects before adoption and leaves the
world, relation store, derived indexes, provenance, events, output, RNG, and
tasks unchanged.

## Incremental maintenance

Full recomputation defines semantics. Incremental maintenance is an
optimization that must be observationally equivalent to that reference.

The runtime maintains indexes from each rule input key to affected output
groups. An authoritative delta invalidates only reachable groups in the
nonrecursive dependency DAG. For every maintained candidate:

```text
incremental(candidate, delta)
    ==
full_recompute(apply(candidate, delta))
```

where equality includes derived tuples and canonical proof alternatives.

Shared supports use reference counts or an equivalent exact support set. A
derived fact disappears only when its final proof alternative disappears.
Aggregate groups are recomputed from their indexed affected input multiset in
v0; algebraic differential aggregates may be added later if they preserve the
same reference result.

## Provenance and `why()`

Every authoritative relation change receives the same settlement and
resolver ancestry as a component write. Every derived fact records bounded,
canonical proof alternatives:

```text
DerivedFact
    ← rule identity
    ← ordered support fact IDs
    ← authoritative relation/component facts
    ← causal settlement resolution
    ← proposal fan-in and event ancestry
```

Opaque record IDs do not participate in semantic equality. Proof alternatives
are ordered by canonical rule identity and support keys. Duplicate paths are
deduplicated. Hosts apply capability filtering before rendering; a hidden fact
becomes a stable redacted support and cannot leak a relation name, tuple,
payload size, or hidden sort key.

Proof count, depth, node count, and canonical encoded bytes use the same
versioned transaction limit profile. Exceeding an explanation limit returns a
bounded typed result rather than constructing an unbounded tree.

## Storage and compilation

The language semantics do not require a particular kernel layout. A compiler
may lower relations to ECS components, edge tables, sorted columns, hash
indexes, or a hybrid. The following identities remain stable across layouts:

- module-qualified relation and rule IDs;
- canonical row encoding;
- schema and index manifest digest;
- authoritative fact IDs;
- derived proof semantics;
- wire and replay encoding.

No problem-specific domain algorithm belongs in the VM. Domain examples are
ordinary relation declarations and rules compiled through the same generic
schema, join, aggregate, indexing, and provenance machinery.

## Capability model

Capabilities distinguish:

- reading a relation schema;
- reading visible rows;
- staging authoritative writes;
- reading derived rows;
- reading proof origins.

Selection and internal validation operate on the complete candidate. Rendering
is capability-filtered afterward. Hidden rows must not affect visible ordering
or reveal multiplicity beyond an explicitly granted bounded count.

## Wire and replay identity

The compiled-program manifest binds relation schemas, rule dependency graphs,
aggregate semantics, index declarations, and semantic versions. The
operational world checkpoint binds authoritative rows, derived indexes or their
verified rebuild identity, and provenance supports.

Portable replay requires matching program, operational world, limits, and
capabilities before executing. A replay implementation may rebuild derived
indexes, but it must verify their canonical digest before exposing the child
VM.

## Reference model and fixtures

The first executable oracle is intentionally independent of the VM. It stores
canonical relation sets in ordered maps, fully recomputes nonrecursive rules,
and compares that result with indexed affected-group maintenance. It also
constructs canonical proof alternatives.

Required semantic fixtures before parser/runtime work:

1. insertion permutations produce identical authoritative and derived sets;
2. symmetric aliases canonicalize to one row;
3. duplicate insertion is idempotent;
4. unique-key conflicts reject atomically;
5. insert, delete, and unique replacement maintain the same result as full
   recomputation;
6. deleting one support retains a derived fact with another proof;
7. deleting the final support removes it;
8. aggregate overflow and resource limits reject deterministically;
9. relation and component writes share one atomic rejection boundary;
10. `why()` reaches the exact authoritative supports and their settlements;
11. capability rendering redacts hidden rows without ordering leakage;
12. wire and attempt replay preserve canonical relation/proof identity.

The repository integration test `core/vm/tests/rfc0003_reference.rs` is the
initial executable contract. It is an oracle, not a hidden VM implementation.

## Dogfood slice

The first vertical slice is ownership and carrying capacity:

```text
Owns(owner, item)
ItemWeight(item, weight)
CarryCapacity(person, capacity)
        ↓
TotalWeight(person, total)
        ↓
Encumbered(person)
        ↓
movement law and constraint
```

The required explanation is:

```text
movement denied
  ← Encumbered
  ← TotalWeight + CarryCapacity
  ← Owns + ItemWeight
  ← authoritative causal settlements
```

## Implementation sequence

1. executable reference fixtures and canonical schema/value encodings;
2. parser, AST, checker, formatter, LSP, and module identity;
3. authoritative relation store, indexes, and transactional patches;
4. nonrecursive rule planner and full reference evaluation;
5. indexed incremental maintenance with differential tests;
6. candidate constraints, provenance, ACL, wire, WASM, and replay integration;
7. dogfood, fuzzing, benchmarks, and operational tooling.

The RFC remains **Draft** until the reference fixtures, candidate-phase
placement, canonical row encoding, proof limits, and capability-redaction
contract are reviewed. It becomes **Accepted** before syntax lands and
**Implemented experimentally** only after the full dogfood and differential
suite pass.

## Explicit non-goals

- recursive Datalog or fixed-point evaluation;
- unrestricted negation;
- projection or correction constraints;
- rule priorities;
- declaration-order semantics;
- distributed query planning;
- automatic parallel execution;
- exposing ECS storage layout as relation semantics.
