# Facts, Relations, and Derived Facts (Draft)

RFC-0003 is the next experimental World-Law Programming layer. It is a design
and executable-reference milestone today; relation syntax is not yet accepted
by the compiler.

The intended model is:

```rad
relation Owns(owner: entity, item: entity)
    unique item

derive TotalWeight(person, sum(weight))
    when Owns(person, item)
     and ItemWeight(item, weight)
```

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

The Draft now fixes the identity-sensitive v0 rules:

- an `entity` column is a live generational foreign key, with schema-selected
  `on delete restrict` or `on delete cascade` behavior;
- all rows are classified against the complete despawn set before any cascade,
  so mixed restrict/cascade outcomes cannot depend on entity order;
- candidate-local spawn handles are resolved deterministically before relation
  validation;
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
  allocator, while schema-aware semantic wire validates canonical fact keys and
  live generational entity references;
- bindings, facts, proof branches, support nodes, depth, capability
  alternatives, and canonical bytes are deterministically bounded before
  retention;
- scans, join attempts, intermediate states/bytes, proof combinations, and
  aggregate groups are independently charged before materialization;
- globally unique typed rule plans, positive atoms, and predicates use
  canonical evaluation order, so declaration or module order cannot select a
  different typed resource failure.

The executable oracle validates rows through generic typed schemas, preserves
assertion ancestry across deletion and reinsertion, rejects unknown, ill-typed,
dead, duplicate, or noncanonical wire rows, and derives checkpoint encoding and
restoration from one operational state inventory. It remains deliberately
independent of the parser and VM.

V0 intentionally excludes component-backed derivation views, recursion,
unrestricted negation, priorities, fixed points, and storage-layout semantics.
Any later runtime will provide generic relation, join, aggregate, indexing,
transaction, and provenance mechanisms; domain models remain ordinary RAD
code.

See [RFC-0003](../rfcs/0003-first-class-relations.md) for the normative Draft
and the executable oracle in `core/vm/tests/rfc0003_reference.rs`.
