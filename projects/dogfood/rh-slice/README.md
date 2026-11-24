# RH pillar slices

The Riemann Hypothesis proof-compression benchmark (riemann.verifytask.cn, graded on
play.bohrium.com) splits Levent Alpöge's 175,187-file Lean formalization into pillars. Each Fast
pillar freezes a few hundred definition modules and scores a `Solution.lean` by how few
normalized tokens it needs, against the token count of Alpöge's editable slice.

Most of a slice is unused by its theorem. `slice.rad` keeps only what the theorem needs:

    rad slice.rad -- <frozen statement .lean> <entry full name> <output .lean> <source .lean>...

The sources are the editable modules of the proof, each named `<Module.Name>.lean`. They are
ordered by their imports; every top-level declaration is parsed with the docstrings, attributes
and `open ... in` lines in front of it; and only declarations reachable from the entry theorem,
through the names their text mentions, are kept (a repeated full name keeps its first
definition). Each source keeps its namespaces and `open` lines inside a `section`, and the whole
slice sits under `<Core>.Slice`, so nothing clashes with frozen names. A source none of whose
declarations is reached is dropped whole. The frozen theorem is then closed by the entry.

`tokens.rad` counts tokens exactly as the grader does (the board's `TOKEN_RE`: comments and
imports are free, an identifier is one token however long), and `slice.rad` reports the count of
what it writes. `count.rad` prints the count and projected score of any Lean files:

    rad count.rad -- <T0 or 0> <file .lean>...

| Pillar | Slice | Kept | Declarations |
| --- | --- | --- | --- |
| Core 36, pure family decomposition | 394 files, 1,023,975 tokens | 1 file, 1,318 tokens; scored 79.55 (attempt 102708) | 2 of 4 in `PureTypeLatticesOrthogonal.lean` |
| Core 1, zero-free region (not submitted) | 21 files, 21,641 tokens | 1 file, 19,176 tokens | 73 of 94 |
