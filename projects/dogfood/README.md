# Dogfood workloads

This directory is intentionally organized by stable workload name, not by an
extra hierarchy of subjective labels. A single workload may be an app, a
security lab, and a language stress test at once; moving it between category
folders would create churn without changing authority.

Use these discoverability groups when choosing a workload:

- Applications: `opsdesk`, `radsheet`, `radtrack`, `syncdesk`, `todo`.
- Security and persistence labs: `bastion`, `strongbox`, `worldmerge`,
  `migration-chain`, `schema`.
- Research workloads: `collatz-lab`, `frankl-search`, `sudoku`.
- Language and causal verticals: `causal-laws`, `causal-constraints`,
  `causality`, `speculation`, `timetravel`, `world-law-rpg`,
  `authority-effects`.
- Host/extension proofs: `native-math-kernels`, `orianna_gui`, `radgui`.

Add a new workload here only when it exercises several language/runtime
boundaries. Small teaching examples belong under `examples/`; canonical
language behavior belongs under `tests/conformance/`; project-specific MOBA
work belongs under `projects/moba/`.
