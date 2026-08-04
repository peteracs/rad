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
candidate transaction as components, but v0 derivation rules read **relations
only**. Nonrecursive derivation rules compute read-only facts from the complete
relation candidate. Constraints may inspect those derived facts before commit,
and provenance explains every derivation back to authoritative facts and
causal settlements.

The first version is deliberately narrow:

```text
authoritative relation facts at T
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
- **Fact key:** a relation identity plus one canonical typed tuple. It defines
  semantic row equality.
- **Fact assertion:** one committed lifetime of a fact key, with its own
  causal ancestry and opaque assertion version.
- **Logical binding:** one oriented variable assignment produced by querying a
  physical relation row.

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
3. `unique column` declares a named unique constraint (the default name is the
   column name) and means at most one row may exist for each value of that
   column in the complete candidate.
4. Several `unique` clauses may be declared and are checked independently.
5. `symmetric` is valid only for a binary relation whose column types match.
   Storage canonicalizes `(a, b)` and `(b, a)` to one physical row.
6. V0 prohibits `unique` on a symmetric relation. Endpoint-wide uniqueness is
   deferred rather than assigning meaning to a physically sorted column.
7. Duplicate insertion is idempotent. It does not create duplicate
   provenance or alter multiplicity.
8. Relation iteration and serialization use canonical tuple order.

Relation constraints are schema invariants, not declaration-order checks.
Every candidate is judged as a set of rows.

## Entity-reference lifetime

An `entity` relation column is a **live candidate foreign key**, not a reusable
numeric ID. The semantic value is a generational entity handle. A raw allocator
slot is never sufficient to identify a relation endpoint.

Normative rules:

1. Every committed `entity` value references an entity alive in the complete
   committed candidate.
2. A resolver may reference an entity spawned in the same candidate through a
   deterministic candidate-local handle. Handles are resolved in canonical
   handle order before relation conflicts and foreign-key checks.
3. Each entity column declares `on delete restrict` or `on delete cascade`.
   Omission means `restrict`.
4. `restrict` rejects a candidate that despawns a referenced entity unless
   that candidate explicitly removes or replaces every restricting row.
5. `cascade` removes every remaining referencing row in canonical fact-key
   order. A row with any restricting reference rejects rather than partially
   cascading.
6. A cascade is an implicit authoritative relation removal attributed to the
   entity-despawn resolution. It receives its own canonical fact-change record
   and causal ancestry.
7. Allocator-slot reuse increments the generation. A row referring to
   `(slot=42, generation=7)` can never refer to a later entity at
   `(slot=42, generation=8)`.

A historical identifier, if introduced later, uses a distinct `entity_id`
type and is not subject to live foreign-key validation.

## Symmetric logical query semantics

Symmetry affects logical bindings, not only storage:

- ground lookup for `R(a, b)` succeeds when the canonical physical row for
  either orientation exists;
- scanning a distinct-endpoint row `(a, b)` produces logical bindings
  `(a, b)` and `(b, a)`;
- a self-edge `(a, a)` produces one binding;
- both orientations carry the same fact key, assertion version, causal
  provenance, and capability label;
- canonical wire storage contains one physical row;
- aggregates consume the set of complete logical body bindings. The second
  orientation is therefore a real contribution for the opposite endpoint,
  while a self-edge is not doubled.

Alternative proof paths never multiply a logical binding. When an aggregate
input has several proof alternatives, the value is counted once; its aggregate
provenance retains bounded canonical proof combinations separately.

## Authoritative updates

Relation updates are causal writes. A resolver stages typed operations:

```text
Insert(relation, canonical tuple)
Remove(relation, canonical tuple)
ReplaceBy(relation, named unique constraint, unique key, canonical tuple)
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

### Base-relative patch algebra

The v0 operations are:

```text
Insert(relation, tuple)
Remove(relation, tuple)
ReplaceBy(relation, unique_constraint, key, tuple)
```

`ReplaceBy` names exactly one declared unique constraint. Its tuple must carry
the selected key. It removes the base/candidate row claiming that key, if any,
and inserts the replacement as one atomic operation.

Operations from isolated resolver patches are normalized against the immutable
base before candidate adoption:

| Base-relative situation | Canonical result |
| --- | --- |
| Insert a present row | no-op; existing assertion and ancestry remain |
| Remove an absent row | no-op |
| Several identical inserts of a new row | one insert; canonical cause fan-in |
| Several identical removes | one remove; canonical cause fan-in |
| Insert and remove the same fact key | conflict, never a net no-op |
| Symmetric aliases | operations on the same canonical fact key |
| Replace a key with its existing tuple | no-op |
| Several identical replacements | one replacement; canonical cause fan-in |
| Different replacements for one selected key | conflict |
| Replacement naming an unknown/ambiguous constraint | checker error or malformed-patch rejection |
| Final candidate violates any other unique constraint | atomic conflict |

Direct operations and replacement expansions use the same table. Identical
effects coalesce; incompatible effects conflict. Resolver registration and
patch enumeration order cannot change the result.

### Fact key and assertion identity

`FactKey = relation + canonical tuple` defines semantic equality, joins,
duplicate elimination, and content wire equality. `FactAssertionId` identifies
one committed lifetime of that key for provenance and incremental support.

- inserting an already-present key is a true no-op and cannot rewrite its
  assertion ancestry;
- identical inserts that first create a row in one settlement create one
  assertion with complete canonical fan-in;
- removing a row retires that assertion;
- reinserting the same key later creates a new assertion ID and new ancestry;
- derived supports reference assertion IDs internally;
- opaque assertion IDs are excluded from semantic tuple equality but included
  in operational checkpoint identity.

## Derived facts

V0 derivations are positive, nonrecursive, and stratified by a finite
dependency DAG. The checker rejects every cycle between derived relations.

Rules are **range-restricted**. Every variable in a head, scalar predicate,
group key, or aggregate input must be bound by a positive relation atom in the
same rule. Aggregate output variables are the only variables introduced by an
operator. This rejects unbounded forms such as `derive Positive(x) when x > 0`.

Each derived relation has one checker-owned schema. Its arity and column types
are inferred from the first canonical rule head: bound variables retain their
source column type, constants retain their literal type, and aggregate outputs
use the types specified below. Every other rule with that head name must unify
with the exact schema. There are no implicit numeric coercions. Ambiguous,
inconsistent, or unconstrained head types are checker errors, and the resulting
derived schema is part of compiled-program identity.

Allowed operations are:

- typed relation scans;
- equality joins;
- deterministic scalar predicates;
- projection;
- duplicate elimination;
- grouping with explicitly admitted deterministic aggregates.

The initial aggregate contract is exact:

- `count` returns `u64` and fails before incrementing past `u64::MAX`;
- `sum` accepts `i64` only and uses checked addition;
- `min` and `max` accept and return `i64` only;
- floats are excluded from v0 aggregates;
- a group with no positive input binding produces no aggregate row;
- overflow is a typed derivation evaluation failure and atomically rejects the
  settlement;
- the aggregate group identity is the rule ID plus canonical group binding;
- aggregate proof support contains the assertion/proof support for every
  distinct logical input binding, without host-order dependence.

Not allowed in v0:

- recursive rules;
- negation over an open world;
- mutation, proposals, events, I/O, clocks, randomness, tasks, native calls,
  or FFI;
- rule priorities or declaration-order tie breaking;
- a derived fact reading a candidate constraint outcome.

V0 derivations do not scan ECS components. Component-backed fact views require
a later RFC defining field-version support identity, candidate invalidation,
entity destruction, manifest identity, and wire/replay behavior. Components
and relations still commit atomically; laws can bridge component observations
into relation intents explicitly.

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
  → complete authoritative component + relation candidate
  → derived-fact maintenance
  → candidate constraints
  → atomic commit or rejection
```

Constraints read one immutable `CandidateView` containing components plus
authoritative and derived relation facts. Derivation itself reads relations
only. A constraint cannot observe partially maintained indexes or another
constraint's result.

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
    ← authoritative relation facts
    ← causal settlement resolution
    ← proposal fan-in and event ancestry
```

Opaque record IDs do not participate in semantic equality. Proof alternatives
are ordered by canonical rule identity and support keys. Duplicate paths are
deduplicated. Hosts apply capability filtering before rendering; a hidden fact
becomes a stable redacted support and cannot leak a relation name, tuple,
payload size, or hidden sort key.

Proof identity is:

```text
rule identity
+ canonical atom bindings
+ support assertion/proof identities
+ optional aggregate group identity
```

The semantic proof graph is maintained independently from bounded `why()`
rendering. Each authoritative assertion has a set of required read
capabilities. A proof's required capability set is the union of all support
requirements (equivalently, its visibility is the intersection of support
visibility). A derived fact is visible to a recipient if at least one complete
proof alternative is visible. Hidden alternatives do not affect visible
ordering, reveal their count, or change visible canonical bytes. Aggregates
over hidden inputs remain hidden unless a separately specified declassification
rule grants visibility. Constraint evaluation may use privileged candidate
truth, but public rejection rendering never implicitly declassifies supports.

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
- authoritative fact keys and assertion-version semantics;
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
or reveal multiplicity. A later explicit declassification contract may expose
a separately bounded aggregate; ordinary row visibility never does.

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
4. unique-key conflicts reject component, entity, and relation writes through
   their shared atomic candidate;
5. insert, delete, and named unique replacement maintain the same result as full
   recomputation;
6. deleting one support retains a derived fact with another proof;
7. deleting the final support removes it;
8. aggregate overflow and resource limits reject deterministically;
9. entity restrict/cascade, same-candidate handles, and allocator-slot reuse
   preserve live foreign-key identity;
10. `why()` reaches exact assertion versions and settlement fan-in;
11. capability rendering redacts hidden facts/proofs without multiplicity or
    ordering leakage;
12. semantic wire and operational attempt replay preserve their deliberately
    different fact/assertion identities.

The repository integration test `core/vm/tests/rfc0003_reference.rs` is the
executable contract. It uses generic typed schemas, fact keys/assertions,
relation patches, rule plans, proof alternatives, deterministic encodings, and
an incremental dependency maintainer. It is an oracle, not a hidden VM
implementation.

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
