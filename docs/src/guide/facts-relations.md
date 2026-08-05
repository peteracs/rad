# Facts, Relations, and Derived Facts

RFC-0003 is the accepted semantic contract for RAD's next World-Law
Programming layer. Its bounded experimental front end now parses, checks,
formats, and seals relation declarations and derivation rules. The
authoritative ordered-map store now executes `Insert`, `Remove`, and
`ReplaceBy` as atomic candidate patches. The production full-recompute
runtime executes accepted nonrecursive rules after each complete
authoritative candidate and atomically adopts their bounded proof state.

Check a source file without installing runtime behavior:

```text
rad relations check facts.rad --experimental-relations --module game::facts
```

Editor diagnostics, formatting, and document symbols use the same bounded
front end when the server is started with:

```text
rad lsp --experimental-relations
```

An optional leading `// module: game::facts` directive gives an editor buffer
the same stable module ownership used by the CLI; otherwise the LSP derives a
workspace-local module identity from the filename.

The gate is mandatory. The parser meters source bytes, tokens, identifiers,
AST nodes, relation/rule/operation collections, terms, atoms, predicates,
aggregate groups, and structural cost before retention. Successful checking
produces immutable relation schemas with an authoritative/derived kind and
module owner, sealed typed rule plans, a dependency DAG, and one canonical
manifest digest. Kind and ownership participate in that digest.

The intended model is:

```rad
relation Owns(owner: entity, item: entity)
    unique item

derive TotalWeight(person, sum(weight))
    when Owns(person, item)
     and ItemWeight(item, weight)
```

The experimental front end checks authoritative operation shapes, and an
embedding can install the resulting immutable manifest and execute the ground
operation batch through `VM::apply_frontend_relation_operations`:

```rad
Insert(Owns, (alice, sword))
Remove(Owns, (alice, sword))
ReplaceBy(Owns, item, sword, (bob, sword))
```

These operations may target authoritative relations only. Bare operation
identifiers such as `alice` and `sword` are ground symbolic entity references,
not externally bound variables; integer and text columns require literals.
Derived relations are read-only and reject all three operations.

For a composite named unique constraint, the selected key is a tuple. Source
and module declaration order do not affect the sealed manifest identity.
Declarations are owned by their source module: an unqualified declaration is
local, while an explicitly qualified declaration must have exactly the current
module as its prefix. Qualified cross-module names remain valid references.

The runtime bridge now stages authoritative relation rows inside resolver-owned
patches and builds them into the same copy-on-write candidate as component
writes. Compiled resolvers use:

```rad
insert_fact("game::inventory::Owns", [owner, item])
remove_fact("game::inventory::Owns", [owner, item])
replace_fact_by("game::inventory::Owns", "item", [item], [new_owner, item])
```

All three forms are resolver-only and require literal module-qualified
relation identities; `replace_fact_by` also requires a literal named unique
constraint. The runtime validates values against the installed immutable
manifest, resolves entity values to their current generational handles, and
stages no live-world mutation. Relation conflicts or derivation failures
reject the complete candidate before adoption. Positive, nonrecursive rules
derive read-only facts from that complete relation candidate.

`why_fact("module::Relation", [tuple...])` renders the bounded proof tree for
an authoritative or derived fact. Derived branches name the exact sealed rule
and proof identity; authoritative leaves retain the exact assertion lifetime
and continue through the owning resolver, proposal fan-in, law, settlement,
and event ancestry. The typed assertion bridge is part of checkpoint and fork
provenance, so explanations survive transport without parsing diagnostic
strings. Sandboxed fact explanations fail closed until capability-filtered
proof rendering can preserve hidden-branch noninterference.

```text
authoritative component + relation candidate
        ↓
nonrecursive derivation
        ↓
validation-only constraints
        ↓
atomic commit or rejection
```

Authoritative state is part of world identity rather than an auxiliary table.
`save_world`/`load_world`, full fork wire, and fork deltas preserve the sealed
relation manifest, assertion allocator, canonical facts, assertion lifetimes,
causes, capabilities, entity generations, and verified unique indexes.
Relation-only changes alter semantic world digests, while operational replay
also binds the complete assertion history and future allocator state.

Three-way world merge currently fails closed when any branch has a different
authoritative relation state. Assertion-aware relation merge needs explicit
rules for unique transfers, entity remapping, deletion, and ancestry; silently
choosing the base or one branch would lose facts. Host-created transaction
batches should be constructed through `BoundedRelationTransactionBuilder`,
which charges each spawn, component write, relation operation, despawn, value,
text field, metadata entry, candidate handle, and structural byte before
retaining it.

The semantic reference is full recomputation over canonical ordered relation
sets. Indexed incremental maintenance is permitted only when differential
tests prove it produces the same tuples and canonical proof alternatives.
Derived provenance must explain a result through its rule and support facts
back to the causal settlements that established those facts.

The accepted v0 contract fixes the identity-sensitive rules:

- an `entity` column is a live generational foreign key, with schema-selected
  `on delete restrict` or `on delete cascade` behavior;
- all rows are classified against the complete despawn set before any cascade,
  so mixed restrict/cascade outcomes cannot depend on entity order;
- candidate-local spawn handles are resolved deterministically before relation
  validation;
- component writes are normalized by resolved entity and component identity;
  identical values coalesce and different values reject the entire shared
  candidate instead of selecting a last writer;
- a symmetric relation stores one canonical row but queries as both logical
  orientations (one for a self-edge), and its two endpoints have identical
  deletion semantics;
- `Insert`, `Remove`, and named `ReplaceBy` operations use one base-relative,
  order-independent patch algebra;
- a fact key identifies a tuple, while an assertion version identifies one
  committed lifetime and its causal ancestry;
- cascades and final foreign-key/uniqueness checks happen before durable
  assertion IDs are allocated, so transient candidate rows never consume an
  identity or cause a false intermediate conflict;
- rules are range-restricted and relation-only in v0;
- `count`, checked-integer `sum`, `min`, and `max` have exact failure and
  empty-group semantics, and aggregate heads contain only group variables plus
  one fresh output. Every aggregate has a positive input atom;
- a derived fact is visible only through at least one completely visible proof
  branch. Hidden alternatives reveal neither identity, count, nor ordering at
  any downstream join or aggregate;
- operational replay identity includes entity generations and the assertion
  allocator, ordered retired slots, and fresh-space exhaustion; allocation
  retires generation-exhausted slots and returns a typed
  `entity.id_space_exhausted` instead of panicking;
- bounded schema-aware semantic wire validates canonical fact-key sets and live
  generational entity references; relation-wide uniqueness is checked when
  decoded rows enter an authoritative candidate;
- bindings, facts, proof branches, support nodes, depth, capability
  alternatives, and canonical bytes are deterministically bounded before
  retention;
- scans, join attempts, intermediate states/bytes, proof combinations, and
  aggregate groups are independently charged before materialization;
- globally unique typed rule plans, positive atoms, and predicates use
  canonical evaluation order, so declaration or module order cannot select a
  different typed resource failure.
- every explicit and inferred schema passes one invariant validator before it
  enters the sealed relation namespace; duplicate derived-head columns and
  aggregate group variables are rejected explicitly;
- module admission is complete-set based: module count, total module-ID bytes,
  per-module source bytes, and total source bytes are checked before parsing;
  admitted modules are then parsed in canonical identity order and bounded
  diagnostics are reduced globally with source-module attribution;
- a raw-input envelope bounds source bytes, tokens, AST nodes, rules,
  identifiers, terms, atoms, predicates, aggregate groups, structural cost,
  and validation visits while parsing, before an oversized plan exists;
- admitted rule count always permits one complete canonical header pass;
  separately metered body work cannot hide an empty, unqualified, or oversized
  header identifier;
- the bounded parser records exact raw summaries while constructing each rule,
  so nested identifier, term, atom, predicate, aggregate-group, AST-node, and
  structural-cost diagnostics all participate in one global priority without
  rescanning rejected bodies;
- a sealed-plan profile then bounds typed rules, atoms, predicates, terms,
  dependency edges, and canonical plan bytes before evaluation;
- invalid plans use an explicit fixed-priority canonical diagnostic reduction,
  exact sorted child digests, and complete duplicate-conflict witnesses, while
  accepted plans precompute one sealed byte representation, digest, dependency
  set, inferred schema, and resource quote for all later consumers;
- source/token/module-envelope admission is a separate diagnostic domain from
  syntactic construction. An envelope failure prevents parsing; once admitted,
  syntax and semantic errors from every module are collected and reduced in
  canonical module order rather than by caller order;
- the decoder's `max_structural_bytes` is an abstract deterministic structural
  cost, not a measured allocator peak. The runtime implementation will use a
  bounded arena for a true peak-allocation contract.

The executable oracle validates rows through generic typed schemas, preserves
assertion ancestry across deletion and reinsertion, rejects unknown, ill-typed,
dead, duplicate, or noncanonical wire rows, and derives checkpoint encoding and
restoration from one operational state inventory. Its affected-relation
projection harness validates dependency closure and atomicity using the full
reference result; genuine indexed delta maintenance remains a later,
independent differential implementation. The oracle remains deliberately
independent of the parser and VM.

V0 intentionally excludes component-backed derivation views, recursion,
unrestricted negation, priorities, fixed points, and storage-layout semantics.
Any later runtime will provide generic relation, join, aggregate, indexing,
transaction, and provenance mechanisms; domain models remain ordinary RAD
code.

See [RFC-0003](../rfcs/0003-first-class-relations.md) for the accepted contract
and the executable oracle in `core/vm/tests/rfc0003_reference.rs`. The
production front end lives in `core/vm/src/relation_frontend/`. The separate
`core/vm/src/relation_runtime/` consumes only its sealed artifacts; relation
rows, assertion lifetimes, unique indexes, entity generations, and manifest
identity ride the same `WorldSnapshot` inventory as ECS state.

World and fork loading seal the entity allocator before reconstructing ECS
rows. The transmitted live, reusable-free, and generation-exhausted retired
slots must be a canonical, disjoint, exact partition of every issued slot.
Duplicate or out-of-order entries, overlaps, gaps, and noncanonical exhaustion
state reject before insertion, so a compact hostile payload cannot expand a
sparse identity range or allocate one reusable identity twice.
Generation-exhausted slots retire immediately on destruction and never appear
in the reusable-free set.

The embedding transaction may also carry candidate-local spawns and component
writes. Candidate handles resolve before schema validation; duplicate component
writes coalesce only when their values agree. Any relation, foreign-key,
uniqueness, component, or allocator failure discards the complete candidate.
The current runtime is intentionally an ordered-map reference implementation;
it does not evaluate `derive` rules yet.
