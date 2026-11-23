# RH pillar slices

The Riemann Hypothesis proof-compression benchmark (riemann.verifytask.cn, graded on
play.bohrium.com) splits Levent Alpöge's 175,187-file Lean formalization into pillars. Each Fast
pillar freezes a few hundred definition modules and scores a `Solution.lean` by how few
normalized tokens it needs, against the token count of Alpöge's editable slice.

Most of a slice is unused by its theorem. `slice.rad` keeps only what the theorem needs:

    rad slice.rad -- <frozen statement .lean> <entry full name> <output .lean> <source .lean>...

It parses every declaration in the source files, together with its namespace and the `variable`
lines in force, follows the names each declaration mentions, and keeps those reachable from the
entry theorem. Names defined nowhere in the sources come from the frozen interface or Mathlib.
The kept declarations move into `<Core>.Slice` so they cannot clash with frozen names, and the
frozen theorem is closed by the entry.

| Pillar | Slice | Kept | Declarations |
| --- | --- | --- | --- |
| Core 36, pure family decomposition | 394 files, 1,023,975 tokens | 1 file, about 800 tokens | 2 of 4 in `PureTypeLatticesOrthogonal.lean` |
