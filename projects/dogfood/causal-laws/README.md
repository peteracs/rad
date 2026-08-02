# Causal Laws dogfood

This project is the executable vertical slice for
[RFC-0001](../../../docs/rfcs/0001-causal-settlements.md). It demonstrates
three independent damage causes settling into one atomic `Shield` + `Health`
transition, with `why()` rendering the complete proposal fan-in and its
originating event.

```powershell
rad projects/dogfood/causal-laws/main.rad --experimental-laws
```

Expected state:

```text
Health: 55/100
Shield: 0
```

The Rust acceptance tests additionally exercise all six law-call
permutations, conflicting candidate rollback, replay, wire provenance, and
sandbox write ACLs.
