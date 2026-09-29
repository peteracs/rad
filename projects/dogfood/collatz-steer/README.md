# RAD strategy steering for Collatz

In this workload, RAD chooses the next research direction from measured
evidence. Nothing here is a proof.

```powershell
.\target\release\rad.exe projects/dogfood/collatz-steer/steer.rad --strict-types --deny-warnings --experimental-laws
```

The run takes 0.4 s. It works as follows:

1. Each strategy probe (`probes.rad`) runs in its own forked world.
   `assert_only_changed` checks that it wrote only its evidence.
2. Each probe submits a `Lead` intent with a measured leverage score and a
   calibration flag. The flag records whether the probe separates 3x+1 from
   3x-1, which has genuine cycles.
3. The `Choose` resolver ignores every lead that fails calibration and
   selects the highest-leverage remaining one. `why(direction, Direction)`
   records every proposal that took part in the decision.

## Current probes and decision

| strategy | measured signal | calibrated | leverage |
|---|---|---|---|
| automatic-certificate | stopping-time Hankel rank 38/64 for both maps | no | 406 (rejected) |
| mirror-families | mirror law explains 15.5% of the stopping time of `2^L - m` | yes | 155 |
| cycle-arithmetic | naive minimal-cycle bounds, largest 39 for q < 39 | yes | 120 |

The selected direction is **mirror-families**.

The Hankel probe (`hankel_probe.rad`) shows that the rank of the
prefix/suffix matrix of the stopping time grows about 1.75 times per digit
(4, 7, 12, 21, 38 for k = 2..6) and does not stabilize. So no finite
automaton computes the stopping time. The ranks for 3x+1 and 3x-1 are nearly
identical, so this complexity is generic rather than arithmetic, and the
engine retires the automatic-certificate route.

## Mirror exit law (`exit_law.rad`, `exit_probe.rad`)

Three corrections to the original mission shaped this probe:

1. A mirror family built from a 3x-1 cycle cannot exit below its start. A
   3x-1 cycle with `q` odd steps in length `j` exists only when
   `3^q > 2^j`. That is exactly the condition that the 3x+1 family grows
   during the mirror window. The densities are 1, 2/3 and 7/11, all above
   `log_3 2 = 0.6309`.
2. One full 3x-1 cycle returns `m` to itself, so for every `k >= 1` the
   exit state is exact:
   ```text
   T+^(kj)(2^(kj) - m) = 3^(kq) - m.
   ```
3. The halving count depends on `c mod 8`. If `c = 1, 3 (mod 8)`, then
   `c = 3^s` for a 2-adic integer `s`. The count is 1 when `Q - s` is odd
   and `2 + nu2(Q - s)` when it is even, so it is unbounded. If
   `c = 5, 7 (mod 8)`, then `c = -3^s`, and the count is 2 when `Q - s` is
   odd and 1 when it is even, so it is bounded.

The probe runs one forked world per family. A `Collapse` intent feeds the
`Combine` resolver, and `why(boundary, Boundary)` records the decision. The
run takes 0.1 s. It checks the exact identity by direct iteration, the
valuation law for every `k <= 4096`, and the immediate-collapse set
`{k : nu2(3^(kq) - m) > k log2(3^q/2^j)}`:

| m | δ bits per period | immediate collapse at k | proof status |
|---|---|---|---|
| 1 | 0.585 | 1, 2, 4, 8 | all k: `nu2 <= 2 + log2 k < 0.585k` for `k >= 9` |
| 5 | 0.170 | 1..11 | all k: `nu2 = 2` always |
| 17 | 0.0947 | 26 values, largest 60 | exact for `k <= 4096` (largest valuation 14) |

For `m = 17`, a collapse beyond `k = 4096` would require a valuation above
386. That would need a zero run hundreds of bits long in the 2-adic
expansion of `log_3(17)/7`. Explicit p-adic linear-forms bounds
(Baker–Yu) are the standard tool for excluding such runs.

**Structural finding.** On these families the excess over the start grows
linearly (`k δ` bits). The 2-adic cancellation available at exit grows only
logarithmically (`nu2 = O(log k)`). So every mirror family collapses at exit
for only finitely many `k`. After that, the next `D` steps of the exit
trajectory depend on `k mod 2^(D+O(1))`. Removing the excess needs
`D ~ kδ/0.2075` steps, and `2^D` then exceeds `k`, so 2-adic continuity
gives no uniform law for the family beyond the exit layer.

## Exit walk unified with the ladder (`bigwalk.rad`, `unify.rad`)

**Correcting the mission.** The ladder envelope bounds the offset only for
first-contraction words, whose prefixes keep `3^i >= 2^(e_i)`. A walk after
the exit may dip below zero before it pays off the excess. The part that
transfers is the ladder's block lemma (Lemma B), which holds at every phase.
Before the walk `W_t = V_t - t log2 3` first exceeds `E = k δ`, every prefix
satisfies `e_i <= i log2 3 + E`. So each term `2^(e_i)/3^(i+1)` is a
phase-shifted ladder term, and at the first-passage time the additive part of
`T^s(3^(kq) - m)` is `O(t)`. Crudely it is at most `t/3`; the sharp envelope
gives about `0.2405 t + 0.77`. Against `2^(kj)` that is negligible. Descent
of the family is therefore, up to an explicit margin, first passage of the
coefficient walk to `k δ`.

**Exact check.** The probe iterates exact big-integer shortcut steps from
`3^(kq) - m`, for `k <= 60` (up to `2^660`), in one forked world per family.
`Finding` intents feed the `Merge` resolver, and `why(unified, Unified)`
records the decision. The run takes 0.47 s.

| family | runs | descent step = first-passage step | deepest post-exit dip `min W` |
|---|---|---|---|
| m = 1 | 59 (k = 2..60) | all 59 | -4.21 bits (k = 5) |
| m = 5 | 60 | all 60 | -5.55 bits (k = 16) |
| m = 17 | 60 | all 60 | -3.43 bits (k = 15) |

In every one of the 179 runs, the step where the value first drops below
`2^(kj) - m` is exactly the step where the coefficient walk first clears
`k δ`. The additive accumulator never delayed or advanced descent.

The halving rate at first passage (at least 1.651) is tautological, since
first passage means `V_t > t log2 3 + E`. It is not evidence and is not
used.

**Where the route ends.** The mirror families reduce exactly to the
coefficient walk of their exit states. For a general `n` the same reduction
is the coefficient-stopping-time statement: the parity walk of every
positive integer eventually becomes positive. The hard numbers are the
2-adic neighbours of the closed set `S` of 2-adic integers whose walk never
becomes positive. `S` contains the negative-integer cycles `-1`, `-5` and
`-17`, which are exactly the mirror images of the 3x-1 cycles. Every route
explored so far converges on one question: why no positive integer lies in
`S`. Answering it requires Archimedean input that the 2-adic structure alone
does not provide.

## The trapped set S (`sieve.rad`, `rational.rad`, `sieve_steer.rad`)

`S = {x in Z_2 : W_t(x) <= 0 for all t}`. A least counterexample above the
barrier threshold would lie in S. `sieve_steer.rad` runs two forked worlds,
gathers their `Fact` intents in the `Assemble` resolver, and applies a
constraint that fails if S ever shows an integer point beyond `-1, -5, -17`.
`why(anatomy, Anatomy)` records the provenance. The run takes 0.85 s.

**Survival measure.** `mu(K) = |S_K| / 2^K` is computed by an exact DP over
(steps, odd count), in `O(K^2)` rather than by enumerating `2^K` residues.
The counts `|S_K|` for `K <= 60` are exact:
`1, 1, 2, 3, 4, 8, 13, 19, 38, 64, ...`, the known sequence of residues whose
stopping time exceeds `K`. The decay is exponential, with a ballot-type
correction:

```text
mu(K) ~ C K^(-3/2) 2^(-eta K),   eta = 1 - H(log_3 2) = 0.0500
```

Near `K = 950`, `log2 mu` falls 5.223 bits per 100 steps; the formula
predicts 5.232. So S has Hausdorff dimension about 0.95 in `Z_2`. The
prefactor oscillates (10.35 at K = 600, 10.21 at K = 700, 10.44 at K = 800).
A barrier of irrational slope suggests a link to the continued fraction of
`log2 3`, but that is unconfirmed at this sampling resolution.

**Rational points.** Each primitive periodic parity word whose walk stays
negative has the rational fixed point `x = C/(2^j - 3^q)`. By the cycle lemma
every expanding necklace has such a rotation, so `S ∩ Q` is infinite. A
pruned depth-first search replaced full enumeration: period 18 went from
6.5 s to 0.27 s, a 24x speed-up.

| period <= | trapped points | cycles | integer points | smallest other denominator |
|---|---|---|---|---|
| 14 | 1567 | 403 | -1, -5, -17 | 11 |
| 18 | 16630 | 3168 | -1, -5, -17 | 11 |
| 20 | 58881 | 10764 | -1, -5, -17 | 11 |

A rational point `-a/d` of S is the image, under `x -> -x`, of a positive
cycle of the map `3x - d`. The integer points are therefore exactly the 3x-1
cycles, and "S ∩ Z consists only of -1, -5, -17" is the open conjecture that
3x-1 has exactly three cycles.

**Circular framings removed.**

- The "Archimedean escape deadline" is circular. Every `S_K` contains
  `2^K - 1`. Let `m(K)` be the least positive integer whose walk stays `<= 0`
  for `K` steps. Then `S ∩ N = ∅` exactly when `m(K) -> ∞`, which is the
  conjecture itself. The published verification below `2^71` already
  excludes every smaller `n` directly.
- `S ∩ N = ∅` is, up to the barrier's finite threshold, Terras's
  coefficient-stopping-time conjecture. It is a clean formulation, not a new
  one.

## Escape delays and what really survives (`delay.rad`, `delay_steer.rad`, `hug.rad`)

**Logic correction.** Positive cycles are excluded from S, since
`2^j > 3^q` forces `W_j > 0`. That does not reduce Collatz to divergence. A
cycle's least element has contracting coefficient but never descends, so it
is a coefficient-stopping-time (CST) failure, not a point of S. The correct
structure is:

- Collatz implies `S ∩ N = ∅`.
- `S ∩ N = ∅` together with CST implies Collatz.
- Every counterexample lies in `S ∩ N` or is a CST failure. The barrier
  workload bounds the CST failures.

**Escape delays.** The residues of `S_K` are generated by Terras's rule
`T^t(r + 2^t u) = T^t(r) + 3^q u`, visiting only surviving prefixes. The
counts match the exact `|S_K|` table. Each depth runs in its own forked world,
and `Winner` intents feed the `Tally` resolver. The run takes 0.18 s.

| K | `|S_K|` | max delay `tau - K` | argmax | shadow delays of `2^K - 1, -5, -17` |
|---|---|---|---|---|
| 10 | 64 | 71 | 703 | 8, 2, 11 |
| 12 | 226 | 69 | 1407 | 39, 1, 1 |
| 14 | 734 | 91 | 10087 | 29, 40, 13 |
| 16 | 2114 | 119 | 35655 | 32, 2, 15 |
| 18 | 7495 | 117 | 35655 | 71, 2, 3 |

The worst delays at every depth come from the classical stopping-time record
holders 703, 10087 and 35655, not from the cycle shadows. The shadows have
the deepest dip at step `K` (`-0.585 K` for `2^K - 1`) but escape quickly.
This corrects the earlier claim that the hardest numbers are the 2-adic
neighbours of the negative cycles.

**Record holders dive; they do not hug the barrier.** `hug.rad` tested
whether they track the line of slope `log_3 2`. They do not:

| n | bits | tau | deepest dip | mean W while trapped |
|---|---|---|---|---|
| 27 | 5 | 59 | -7.30 | -3.59 |
| 703 | 10 | 81 | -7.47 | -4.30 |
| 10087 | 14 | 105 | -6.94 | -3.45 |
| 35655 | 16 | 135 | -9.17 | -5.01 |
| 270271 | 19 | 164 | -15.48 | -9.03 |

Long survival is a deep excursion followed by slow recovery at a drift of
about `+0.21` bits per step. Imitating a 2-adic pattern explains at most
`0.585 L` bits of dip, yet 27, with 5 bits, dives 7.3 bits. The deficit is
created by the map's own dynamics after the starting digits are exhausted.
The finite length of `n` does not bound it.

## Excursion depth and the cycle bound (`excursion*.rad`, `cycle_bound.rad`)

**Corrections to the mission.**

- The carry-chain argument fails. The state of the dynamics is the current
  integer, whose bit length grows, so determinism does not force
  periodicity. A divergent orbit would be deterministic and aperiodic.
- A universal bound `D(n) <= C log2 n` would bound every trajectory by about
  `n^(1+C)`, which rules out divergence. So it is as hard as the divergence
  half of Collatz, and measurements can only probe it. The random-walk model
  predicts the answer. Steps are +1 or -0.585 with equal odds, and
  `(2^-1 + 2^0.585)/2 = 1` exactly, so the chance of ever dipping `D` bits is
  `2^-D`. Among `2^L` starts, the deepest dip is then about `L`, so
  `D/L -> 1`. This matches Lagarias–Weiss's heuristic that trajectory maxima
  grow like `n^2`.
- The ladder envelope does not apply to cycle words. The right tool is the
  product identity (below).

**Excursion scan.** Every `n < 2^18` is covered by eight forked worlds, with
exact integer micro-bit walks, in 3.8 s. The best `D/L` is 1.536 at 27, a
small-number effect. Past that, each range's best stays between 0.77 and
0.97. The deepest dips grow in step with `L`: 7.3 bits for `n < 64`, 12.2 for
`n < 2^16`, and 15.7 for `n < 2^18` (at 159487). The longest odd run after
the horizon grows slowly: 6, 8, 13, 13, 13, 15, 15, 15. There is no sign of
an entropy barrier that would shrink `D/L`, and the data fits `D ~ L`.

**Cycle-length bound (proved).** Around a cycle of length `j` with `q` odd
steps, the product of `(3 + 1/x)` over the odd elements equals `2^j`. Every
element exceeds `N = 2^71` by the published verification. Hence

```text
0 < j - q log2 3 < q / (3 N ln 2).
```

The barrier's Farey certificate (`check_barrier` with
`a/b = 103768467013/65470613321` and `c/d = 10439860591/6586818670`) proves
`3 N (c ln 2 - d ln 3) > d` with arbitrary-precision logarithm intervals, so
`j/q` cannot be `>= c/d`. It also checks `cb - ad = 1`. A fraction strictly
between Farey neighbours has denominator `>= b + d` and numerator
`>= a + c`. `cycle_bound.rad` runs in 0.09 s and proves:

```text
every nontrivial positive cycle has q >= 72,057,431,991 odd steps
and length j >= 114,208,327,604 shortcut steps.
```

To my knowledge this matches the best published bound (Hercher). It
reproduces that bound from the repository's own certificate; it is not a new
record.
