# Executable language tour

This page is not a collection of copied snippets. Every RAD block is included
from the Sovereign Grid acceptance project, and the documentation gate executes
the compiler-owned language-surface manifest before accepting the book. Editing
an example therefore edits the program that CI checks.

For an exhaustive rule-by-rule index, use [Syntax rules and
evidence](generated/syntax-rules.md). For signatures and runtime contracts, use
the [Builtin API by category](generated/builtin-index.md).

## Canonical program and native data model

```rad
{{#include ../../../projects/dogfood/sovereign-grid/main.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/schema.rad}}
```

## Advanced expressions, patterns, and statements

```rad
{{#include ../../../projects/dogfood/sovereign-grid/language/surface.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/language/values.rad}}
```

## Ownership, transactions, views, indexes, and phases

```rad
{{#include ../../../projects/dogfood/sovereign-grid/owners/grid_owner.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/systems/control_loop.rad}}
```

## Stateful model checking and test isolation

```rad
{{#include ../../../projects/dogfood/sovereign-grid/tests/grid_model.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/tests/shared_world.rad}}
```

## Complete runtime API harnesses

```rad
{{#include ../../../projects/dogfood/sovereign-grid/builtins/values_collections_text.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/builtins/ecs_queries_provenance.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/builtins/speculation_persistence.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/builtins/host_io.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/builtins/host_network.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/builtins/ffi_native.rad}}
```

## Experimental causal and relation surfaces

These files execute only under their explicit feature gates. Their inclusion
does not enable experimental syntax for the stable program.

```rad
{{#include ../../../projects/dogfood/sovereign-grid/experimental/causal_dispatch.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/experimental/relations.rad}}
```

```rad
{{#include ../../../projects/dogfood/sovereign-grid/experimental/relation_builtins.rad}}
```
