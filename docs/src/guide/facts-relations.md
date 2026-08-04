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
from that complete candidate. Constraints then inspect authoritative and
derived facts together before the world commits.

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

V0 intentionally excludes recursion, unrestricted negation, priorities,
fixed points, and storage-layout semantics. The VM kernel will provide generic
relation, join, aggregate, indexing, transaction, and provenance mechanisms;
domain models remain ordinary RAD code.

See [RFC-0003](../rfcs/0003-first-class-relations.md) for the normative Draft
and the executable oracle in `core/vm/tests/rfc0003_reference.rs`.
