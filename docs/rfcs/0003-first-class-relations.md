# RFC-0003: First-Class Facts, Relations, and Derived Facts

- **Status:** Accepted
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

1. Relation, column, unique-constraint, and rule identities are nonempty.
   Relation and rule identities use normal module qualification and visibility.
   Column names and unique-constraint names are unique within their relation;
   rule IDs are globally unique within the compiled program.
2. A row is a typed tuple with fixed arity.
3. `unique column` declares a named unique constraint (the default name is the
   column name) and means at most one row may exist for each value of that
   column in the complete candidate.
4. Several `unique` clauses may be declared and are checked independently.
   Constraint names are unique within the relation; duplicate names are a
   schema error because `ReplaceBy` must identify exactly one constraint.
5. `symmetric` is valid only for a binary relation whose column types match.
   Both endpoints must also have identical endpoint-semantic metadata,
   including `on delete`. Storage canonicalizes `(a, b)` and `(b, a)` to one
   physical row.
6. V0 prohibits `unique` on a symmetric relation. Endpoint-wide uniqueness is
   deferred rather than assigning meaning to a physically sorted column.
7. Duplicate insertion is idempotent. It does not create duplicate
   provenance or alter multiplicity.
8. Relation iteration and serialization use canonical tuple order.
9. A unique constraint cannot repeat a column index. Delete policy metadata is
   valid only on `entity` columns.
10. Authoritative and derived relations occupy one module-qualified namespace;
    the same identity cannot name both kinds.

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
4. Explicit relation operations are applied first. The complete despawn set is
   then frozen, and every remaining row is classified once against that set
   before any implicit removal or entity destruction occurs.
5. `restrict` rejects a candidate that despawns a referenced entity unless
   that candidate explicitly removes or replaces every restricting row. A
   cascade caused by another endpoint cannot satisfy a restricting reference.
6. If any despawned endpoint of a row uses `restrict`, the whole candidate
   rejects. Otherwise `cascade` schedules that row for one removal in canonical
   fact-key order. Cascades are applied only after every row passes
   classification, and entities are despawned only after every cascade.
7. A cascade is an implicit authoritative relation removal attributed to the
   entity-despawn resolution. It receives its own canonical fact-change record
   with the exact settlement, resolver, capabilities, and proposal fan-in of
   every despawn that caused it.
8. Allocator-slot reuse increments the generation. A row referring to
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

Component targets are normalized by resolved generational entity plus
component identity before candidate mutation. Identical writes coalesce. Two
different values for one target reject with `component.write_conflict`; source
or resolver enumeration order never selects a last writer. Candidate-local
entity handles are resolved before this normalization. Component conflict
rejects every relation write, spawn, despawn, and component write in the same
candidate atomically.

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

Cross-algebra rules with entity destruction are also base-relative:

- an explicit remove before a restricting despawn satisfies that reference;
- a `ReplaceBy` away from a despawned endpoint satisfies the old reference,
  while its replacement tuple is validated against the complete despawn set;
- a newly inserted row with any restricting despawned endpoint rejects;
- a newly inserted row whose despawned endpoints are all cascading never
  commits, consumes no assertion version, and emits no durable fact change;
- insert-plus-despawn and replace-plus-despawn outcomes are independent of row,
  column, patch, and entity-ID order.

Candidate finalization has one normative phase order:

```text
normalize explicit operations
  -> create provisional rows without durable assertion IDs
  -> freeze and classify the complete despawn set
  -> apply canonical cascades
  -> validate final live foreign keys and unique constraints
  -> allocate assertion IDs only for surviving new FactKeys
  -> emit durable changes
  -> atomically adopt
```

Schema invariants and assertion exhaustion judge the final candidate, never an
intermediate row that a same-candidate cascade removes. A transient cascading
row therefore succeeds even when no further assertion ID is available, while
any surviving new assertion fails atomically if its durable ID cannot be
allocated.

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

### Entity allocator exhaustion

Entity allocation is total and fallible. A reusable slot at generation
`u32::MAX` is permanently retired and the allocator continues with the next
canonical reusable slot, then fresh slot space. The final representable fresh
slot may be allocated once. If neither reusable nor fresh identity remains,
the complete candidate rejects with `entity.id_space_exhausted`; allocation
never panics or wraps. Ordered retired slots and the fresh-space exhaustion bit
are part of operational checkpoint identity and restoration.

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

For an aggregate rule, the nonconstant head variables are exactly the unique
group variables plus one fresh aggregate output. A nongrouped body variable
cannot be projected by selecting a representative row. The output is not
bound by an atom and cannot also be an input or group variable. `count` has no
value input; `sum`, `min`, and `max` have exactly one positively bound `i64`
input. Atom arity, constants, predicates, head columns, and every rule sharing
one derived head are checked against exact schemas with no implicit coercion.
Postaggregate bindings are constructed solely from the canonical group key and
aggregate output.

Every aggregate rule has at least one positive body atom. An atomless aggregate
is a checker error (`derivation.aggregate_requires_positive_input`), rather
than treating the empty conjunction as one input. A valid positive scan that
returns no binding produces no aggregate row.

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
rendering. Each complete proof branch retains exactly one proof ID and one
capability set; derived scans never collapse visible and hidden proof IDs into
one support. A downstream nonaggregate rule creates a separate proof for each
complete support combination. An aggregate counts each logical binding once
while retaining its proof combinations as separate provenance branches.

Each authoritative assertion has a set of required read capabilities. A
proof's required capability set is the union of that complete branch's support
requirements (equivalently, its visibility is the intersection of support
visibility). A derived fact is visible to a recipient if at least one complete
proof alternative is visible. Rendering filters complete branches before
constructing identities or ordering them. Hidden alternatives do not affect
visible ordering, reveal their count or IDs, or change visible canonical bytes
at any derivation depth. Aggregates over hidden logical inputs remain hidden
unless a separately specified declassification rule grants visibility.
Constraint evaluation may use privileged candidate truth, but public rejection
rendering never implicitly declassifies supports.

The versioned derivation profile bounds successful bindings, derived facts,
proofs per fact, total proofs, support nodes, proof depth, capability
alternatives, and canonical encoded bytes. It separately bounds intermediate
work and storage:

```text
rows scanned
join attempts, including failed unifications
materialized intermediate states
deterministic intermediate bytes
proof-combination attempts, including later deduplication
aggregate group entries
```

The intermediate-byte charge is a deterministic conservative encoding budget
covering copied names, values, tuples, bindings, supports, derived logical
rows, aggregate keys, and proof identities. Every scan, clone, expansion, and
retention is charged before allocation or visibility. An exceed is one typed
derivation resource failure: the complete authoritative candidate and prior
derived state remain unchanged. Full recomputation and incremental maintenance
return the same exact limit class. One-under-limit succeeds and one-over-limit
fails deterministically. Bounded `why()` rendering has an independent final
envelope and never constructs an unbounded tree.

Typed rule plans are adjudicated in canonical order by derived-head identity,
globally unique rule ID, and canonical plan digest. Positive atoms and scalar
predicates use canonical plan order because conjunction is commutative in v0.
Source declaration, schema registration, module loading, and atom enumeration
cannot select which typed resource error wins.

A separate versioned sealed-plan profile bounds rule count, atoms per rule,
predicates per rule, total terms, dependency edges, and total canonical typed
plan bytes before evaluation. This makes scalar predicate work statically
bounded even though the derivation work meter is charged primarily at row,
join, intermediate-state, proof, and aggregate boundaries.

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

Its relation-state inventory includes the complete entity generation map,
next entity slot, fresh-space exhaustion, ordered free and retired slots, live
handles, the next assertion ID, current assertions, components, ancestry, and
every future-determining maintenance counter. The same state object drives
restoration and canonical encoding. Canonical semantic decoders receive the
sealed schema, live entity environment, and a versioned decode profile. They
reject unknown relations, wrong arity or types, dead entity handles,
noncanonical symmetric orientation, duplicate rows, out-of-order rows, and
input/fact/value/text/allocation limit excesses before oversized retention.
This decoder returns a canonical `FactKey` set; relation-wide uniqueness and
other authoritative-state invariants are validated when that set enters the
complete candidate. Semantic encoding contains fact keys; operational encoding
additionally contains assertion lifetimes and allocator state.

Portable replay requires matching program, operational world, limits, and
capabilities before executing. A replay implementation may rebuild derived
indexes, but it must verify their canonical digest before exposing the child
VM.

## Reference model and fixtures

The first executable oracle is intentionally independent of the VM. It stores
canonical relation sets in ordered maps, fully recomputes nonrecursive rules,
and constructs canonical proof alternatives. Its affected-relation projection
harness validates dependency closure and atomic state adoption by projecting
affected answers from a full reference result. It is not an independent
indexed delta-maintenance implementation; that proof belongs to implementation
sequence step 5.

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
8. aggregate overflow, no-match and match-all joins, large copied values,
   proof-branch explosion, aggregate-group growth, and every retained or
   intermediate derivation limit reject deterministically and atomically;
9. entity restrict/cascade, same-candidate handles, and allocator-slot reuse
   preserve live foreign-key identity;
10. `why()` reaches exact assertion versions and settlement fan-in;
11. capability rendering across joins, aggregates, and several derivation
    layers redacts hidden facts/proofs without multiplicity, identity, or
    ordering leakage;
12. schema-aware canonical semantic wire rejects unknown, ill-typed, dead,
    duplicate, or noncanonical rows, while operational attempt replay binds
    generation and assertion allocators as well as their deliberately different
    fact/assertion identities;
13. atomless aggregates and asymmetric symmetric metadata are checker errors;
14. final-candidate cascades precede uniqueness and assertion allocation;
15. every rule, atom, schema, row, and module permutation yields identical
    facts/proofs/bytes or the same exact typed failure.
16. component writes coalesce or conflict by resolved target without
    last-writer-wins behavior, and every conflict is atomic across the shared
    candidate;
17. final fresh-slot allocation, retired generation-exhausted slots, and total
    entity-ID exhaustion are typed, deterministic, checkpoint-bound, and
    panic-free;
18. sealed rule-plan and semantic-decoder profiles reject one-over-limit input
    before evaluation or oversized decoded retention.

The repository integration test `core/vm/tests/rfc0003_reference.rs` is the
executable contract. It uses generic typed schemas, fact keys/assertions,
relation patches, rule plans, proof alternatives, deterministic encodings, and
an affected-relation projection harness. It is an oracle, not a hidden VM or
independent incremental implementation.

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

The reference fixtures, candidate-phase placement, canonical row encoding,
proof and work limits, capability-redaction contract, component conflict
normalization, and total entity-allocation semantics are reviewed and
executable. RFC-0003 is therefore **Accepted** before syntax lands. It becomes
**Implemented experimentally** only after the parser/checker, full-recompute
runtime, dogfood, and independent indexed differential suite pass.

## Explicit non-goals

- recursive Datalog or fixed-point evaluation;
- unrestricted negation;
- projection or correction constraints;
- rule priorities;
- declaration-order semantics;
- distributed query planning;
- automatic parallel execution;
- exposing ECS storage layout as relation semantics.
