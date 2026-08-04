# Facts, Relations, and Derived Facts

RFC-0003 is the accepted semantic contract for RAD's next World-Law
Programming layer. Its bounded experimental front end now parses, checks,
formats, and seals relation declarations and derivation rules. The
authoritative relation store and derived-fact runtime are not implemented yet;
front-end acceptance does not make relation programs executable.

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
produces immutable relation schemas, sealed typed rule plans, a dependency
DAG, and one canonical manifest digest.

The intended model is:

```rad
relation Owns(owner: entity, item: entity)
    unique item

derive TotalWeight(person, sum(weight))
    when Owns(person, item)
     and ItemWeight(item, weight)
```

The experimental front end also checks authoritative operation shapes without
executing them:

```rad
Insert(Owns, (alice, sword))
Remove(Owns, (alice, sword))
ReplaceBy(Owns, item, sword, (bob, sword))
```

For a composite named unique constraint, the selected key is a tuple. Source
and module declaration order do not affect the sealed manifest identity.

Authoritative relation rows will be staged by resolvers inside the same atomic
candidate as components. Positive, nonrecursive rules derive read-only facts
from the complete **relation** candidate. Constraints then inspect components,
authoritative relations, and derived facts together before the world commits.

```text
authoritative component + relation candidate
        ↓
nonrecursive derivation
        ↓
validation-only constraints
        ↓
atomic commit or rejection
```

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
production front end lives in `core/vm/src/relation_frontend/`; it deliberately
has no path to mutate a VM or relation store.
