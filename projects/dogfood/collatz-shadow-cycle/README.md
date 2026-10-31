# Shadow barriers against the smallest cycle

This workload tries to exclude a nontrivial cycle with exactly
`Q* = 72,057,431,991` odd steps, the smallest length that Hercher's
argument allows. It does not succeed. It proves that this whole class of
arguments cannot succeed.

## Idea

- **Barriers.** Every `n < N = 2^71` reaches 1, so no preimage of a cycle
  element lies below `N`. A backward path with `c` odd and `e` even
  preimage steps forces `x >= N (3/2)^c 2^-e`. Whether a path is valid
  depends only on `x mod 3^c`. `barrier.rad` computes the strongest bound
  `b_k(r)` for each residue `r` mod `3^k`. About 69% of residues are barred,
  with a mean barrier of 0.6 bits.
- **Requirement.** The product identity forces a cycle with `Q*` odd steps
  and minimum `n0 = lambda N` to have mean `n0/x` over its odd elements of
  at least `0.5418 lambda`, where `lambda` lies in `[1, 1.3316]`.
- **Relaxation.** States are `(x mod 3^k, bin of D = log2(x/n0))`. Residues
  evolve exactly with each parity step, `D` moves by `+log2(3/2)` or `-1`,
  and states below a barrier are removed. If every closed walk has mean
  payoff below the requirement, no such cycle exists.

## Tools

`barrier.rad` computes the barriers, audits them and writes them to `out/`.
The relaxation runs past RAD's native-call deadline, so it is the project-owned
`shadow_walk` batch tool in `native-math-kernels`:

| command | what it does |
|---|---|
| `shadow_walk ceiling` | the maximum mean payoff per odd step, by Dinkelbach over Howard policy iteration, plus the optimal closed walk. Floating point: a measurement |
| `shadow_walk certify` | exact integer longest-walk potential: an edge-by-edge certificate that every closed walk stays below a requirement, or a checked witness walk |

Payoffs are computed in exact integers, so a certificate involves no floating
point. Interpreted RAD needed 7.6 minutes for `k <= 4` at 64 bins/bit. The
whole suite below takes about 70 s.

## Runs

`run_experiments.py` writes every result to `out/report.json`.

| experiment | question | result |
|---|---|---|
| barrier audit | how strong are the barriers? | 69% of residues barred, mean 0.61 bits at `k = 9`, stable in `k`; for `k <= 6`, no barrier exceeds the real backward paths of any integer below 3000 |
| `depth` | what do barriers do at `lambda = 1`, 64 bins/bit? | 0.7529 falls only to 0.7473 by `k = 7..9`, against the 0.5418 a cycle needs. Each ceiling is bracketed by exact certificates within `2^-12` |
| `lambda` | does `lambda` matter at 1024 bins/bit? | no: 0.7299 at 1, 1.2 and 1.3316 |
| `matched` | does the barrier effect survive finer bins? | the drop shrinks 0.0068, 0.0015, 0 as bins go 128/256, 512, 1024: a discretisation artefact |
| `refinement` | what is the limit as bins refine? | 0.7450, 0.7299, 0.7254, 0.7241 with and without barriers, tending to the ladder value 0.7213; optimal walks are semiconvergents such as 65/41 and 149/94 |
| `escape_walk` | how does the best walk escape? | a 27-step walk with 17 odd steps at `k = 9`, the 27/17 shape of the real 3x+5 cycles through 187 and 347, with 14 nodes pressed against barriers of its own backward path |

The proof graph records this as `cycle_by_shadow_barriers: refuted` in
`collatz-frontier`.

## Conclusion

Verified convergence below `2^71`, residues mod `3^9` and the depth walk
together cannot exclude even the smallest admissible cycle length. The
optimal closed walks hug the contraction line along semiconvergents of
`log2 3`. Their barriers come from their own backward paths, which they
satisfy automatically. So the barriers carry no information, and the
relaxed ceiling equals the ladder bound `1/(2 ln 2)`. This matches the 3x+d
calibration in `collatz-descent`: only global divisibility,
`(2^j - 3^Q) | C`, separates 3x+1.

```powershell
cargo build -p rad-cli -p rad-ffi-worker
python projects/dogfood/collatz-shadow-cycle/run_experiments.py
```
