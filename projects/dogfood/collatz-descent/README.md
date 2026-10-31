# Sharp descent at the first contraction

This workload sharpens result 8 of the Collatz review (the reduction from a
stopping-time bound). In the certified range it needs no Baker-type
citation, and it pays for the sharper statement with exact arithmetic. It
does not prove the conjecture.

```powershell
.\target\release\rad.exe projects/dogfood/collatz-descent/main.rad --strict-types --deny-warnings --experimental-laws
.\target\release\rad.exe projects/dogfood/collatz-descent/audit.rad --strict-types --deny-warnings
```

```powershell
python projects/dogfood/native-math-kernels/survival_receipts.py --out projects/dogfood/collatz-descent/out --bits 25 26 27 28 29 30 31 32 33 34 35 36 --marks 4000
.\target\release\rad.exe projects/dogfood/collatz-descent/cst.rad --strict-types --deny-warnings --experimental-laws
.\target\release\rad.exe projects/dogfood/collatz-descent/blocks_main.rad --strict-types --deny-warnings --experimental-laws
```

`main.rad` takes about 2 minutes. Most of that time is the exact check of the
intercept. `audit.rad` takes about a minute.

## Statement

Let `alpha = log2 3` and `d(q) = ceil(q alpha) - q alpha`. For a floor
`N = 2^k`, let `q*(N)` be the certified value below.

**Theorem.** If `n >= N` and the first contraction of `n` has
`q <= q*(N)` odd steps, then `T^tau(n) < n`.

**Corollary.** Let `n0` be the least element of a nontrivial cycle or of a
divergent orbit. Then either `tau(n0) = infinity` or `q_tau(n0) > q*(n0)`.

**Proof.**

1. At `tau`, every proper prefix is non-contracting, so
   `tau - 1 <= q alpha < tau`. Hence `tau = ceil(q alpha)` and the walk
   value there is exactly `d(q)`, a function of `q` alone.
2. `T^tau(n) < n` holds iff `n (2^tau - 3^q) > C`. Here
   `2^tau - 3^q = 3^q (2^d(q) - 1) > 3^q d(q) ln 2`, and the ladder envelope
   (result 1) gives `C/3^q <= s* q + beta*`. So `n` descends whenever
   `N g(q) >= s* q + beta*`, where `g(q) = d(q) ln 2`.
3. `d(q)` reaches a new record low only at the upper one-sided best
   approximations `p/q > alpha`. These are exactly the right boundaries of
   the Stern–Brocot descent toward `alpha`. Between two records, `d(q)` is
   at least the current record.
4. `descent.rad` walks the Stern–Brocot tree with rigorous 192-bit intervals
   for `ln 2` and `ln 3`, and decides every side test exactly. It then
   scans the records until one fails the inequality over its whole run.
5. `beta* <= 0.6882` is checked in exact integers for every `q <= 7489`.
   This covers all vertices of the ladder certificate.

## Certified ranges

| floor N | safe odd steps q*(N) | safe tau | first dangerous record |
|---|---|---|---|
| 2^71 | 72,057,431,990 | 114,208,327,603 | 114208327604/72057431991 |
| 2^72 | 108,227,849,329 | 171,537,082,721 | 114208327604/72057431991 |
| 2^76 | 282,377,568,122 | 447,557,856,519 | 217976794617/137528045312 |
| 2^80 | 2,723,792,422,700 | 4,317,108,849,728 | 3798934114911/2396860564955 |
| 2^90 | 64,021,008,208,554 | 101,470,897,268,920 | 101470897268921/64021008208555 |
| 2^100 | 3,501,913,073,316,261 | 5,550,400,901,991,451 | 5282454920184382/3332857981044265 |
| 2^110 | 111,738,283,365,989,050 | 177,100,989,030,047,174 | 177100989030047175/111738283365989051 |

`log2 q*(N) / log2 N` is at least 0.500 on every floor. Within the certified
range, the frontier therefore reads `tau(n) <~ sqrt(n)`, not the `n^eps`
with `eps(kappa + 1) < 1` that a Baker measure allows. Measured record
holders have `tau <= 14.5 log2 n`.

## Negative result: the cycle bound does not move

At `N = 2^71` the first dangerous record is the Farey mediant behind result
9, with `q = 72,057,431,991`. There `N g / q ≈ 0.1806`. That value is below
the limit slope `1/(6 ln 2) ≈ 0.2404`, and no envelope exists below that
slope (result 2). `danger_at_limit_slope` certifies `6 ln2 N g < q`
rigorously. So no sharpening of the envelope can raise the cycle bound of
result 9 by this route. The corollary gives exactly the same
72,057,431,991 odd steps.

## Coefficient stopping time conjecture for bounded tau (`cst.rad`)

**Theorem.** Every `n >= 2` with `tau(n) <= 21,317` satisfies
`sigma(n) = tau(n)`: the orbit first drops below `n` exactly at the first
contraction.

**Proof.** Sixteen forked worlds run in parallel through `simulate_many`.
Together they iterate every `2 <= n < 2^24` exactly and find
`T^tau(n) < n` in every case. The largest `tau` there is 287. For
`n >= 2^24`, the descent theorem at floor `2^24` gives `q* = 13,449`,
which is `tau <= 21,317`. The `Join` resolver marks the claim proved only
when the spans tile `[2, 2^24)` with no failure. The direct check takes about 48 s, and `cst.rad` takes about 3.5 minutes in all.

Terras's conjecture below `2^71` would follow from any bound
`tau(n) <= 21,317` for `n < 2^71`. Since `tau <= sigma`, a published
glide-record bound would supply this. The measured ratio
`tau / log2 n <= 14.5` predicts about 1,030 at `2^71`, but that ratio is a
measurement, not a certificate. This workload does not verify it.

### Native extension to 2^36

After the direct check, `cst.rad` reads a `trapped_survival` receipt for every
bit length `b = 25..36`; `survival_receipts.py` writes them. The tool enumerates only the trapped residue
classes, 558,591,985 in total, and follows each real orbit to step 4000.
None is still trapped at step 4000, so every `n < 2^36` has `tau <= 4000`.
This is inside the direct floor's safe range (`tau <= 21,317` at `2^24`).
The descent theorem at `2^36` then gives:

**Theorem.** `sigma(n) = tau(n)` for every `2 <= n < 2^36`, and for every
`n >= 2` with `tau(n) <= 301,993`.

The receipts take about 10 s on 16 cores.

## Calibration on 3x+d (`calibrate.rad`, `arith.rad`)

The maps `x -> (3x + d)/2` have offsets exactly `d` times those of 3x+1,
so every condition above has a 3x+d version. These maps also have real
cycles. The sweep covers every odd `d < 200` not divisible by 3 and every
cycle with least element below 5000: 302 cycles, 109 of them single-block.

- **Soundness.** Every cycle satisfies the descent condition, the envelope
  and the drift lemma.
- **The analytic tools cannot separate 3x+1.** The cycles that are largest
  relative to `d` are single-block cycles at resonant lengths, for example
  `d = 29`, `n0 = 3811` and `7055`, `q/j = 41/65`, where 41 is a
  convergent denominator. They use only 14–69% of the envelope. This is
  exactly the case left open for 3x+1.
- **What separates 3x+1 is arithmetic.** Every one of the 109 single-block
  cycles has `gcd(d, 2^j - 3^q) > 1`. This is forced: if the gcd is 1, then
  `D | C`, so `n0 = d (C/D)` is a scaled copy of a 3x+1 cycle. So every
  primitive 3x+d cycle exists only because a prime factor of `d` divides
  `2^j - 3^q`. For `d = 1` that help is absent.

The analytic route (envelope, resonance, drift, blocks) is now calibrated
as unable to exclude 3x+1 cycles. What remains is the divisibility
`(2^j - 3^q) | C(w)` for a chain of resonant first-contraction words. That
is the Böhm–Sontacchi formulation that Steiner and Simons–de Weger attack
with linear forms in logarithms.

## Cycle-length sieve (`lengths.rad`)

Hercher's argument allows a cycle with `Q` odd steps only when
`g(Q) < Q/(3N)`. The block theorem adds a condition: the cycle must split
into resonant blocks whose prefixes pass `N g(q_k) < s* Q_k + beta* (k+1)`.
Because `g` is additive over blocks, prefix states reduce to cumulative odd
counts, and a breadth-first search over certified block lengths finds all
reachable `Q`.

**Result (negative).** All 104 Hercher-admissible odd counts up to `10^12`
are reachable, out of 175 reachable states. The block structure excludes
no cycle length that Hercher's argument allows. The run takes about 80 s.

## Arithmetic randomness of C mod p (`residues.rad`)

A single-block cycle needs `2^j - 3^q` to divide `C(w)`. For each prime
`p <= 43`, one fork runs an exact dynamic program over all first-contraction
words with `q <= 150` and computes the distribution of `C(w) mod p`. The
weights are floats, so this is a measurement.

- The total-variation distance to uniform decays by a factor of about
  `e^(-0.45 .. -0.56)` per odd step, and `e^(-0.30)` for `p = 5`. It
  reaches float noise by `q ≈ 75`.
- At lengths where `p | D_q`, each prime's worst deviation of the weight on
  residue 0 from `1/p` either occurs at a small length (`q <= 11`, up to
  100%, for example `p = 11` at `q = 11`: +1.9%) or is at most 317 ppm
  (`p = 19` at `q = 19`). The deviation shrinks together with the
  total-variation distance.

No small-prime structure favours or forbids the divisibility a cycle needs.
The heuristic "C mod D is uniform" holds wherever it can be measured. This
is the discrete analogue of the equidistribution that Tao's almost-all
theorem uses modulo `3^k`. The run takes about 3 minutes with 8 workers.

## Resonant block structure (`blocks.rad`, `blocks_main.rad`)

**Drift lemma.** For every `n` with finite `tau` and `q` odd steps,
`T^tau(n) = 2^(-d(q)) (n + C/3^q) < n + 0.24046 q + 0.6882`. So the
coefficient stopping time never ends more than an additive `O(q)` above its
start.

**Block theorem.** Let `n0 >= N` be the least element of a cycle or of a
divergent orbit. Cut its orbit into first-contraction blocks with `q_k`
odd steps, as long as they stay finite. Each block endpoint satisfies
`n0 <= n_(k+1) < n_k + c_k`. Each block also satisfies

```text
d(q_k) ln 2 < (s* (q_0 + ... + q_k) + beta* (k + 1)) / N.
```

So every block length is resonant, meaning `q_k log2 3` lies just below an
integer. A length `q` can be used only once the cumulative odd count
reaches `unlock(q) = N g(q) / s*`.

The first block of a counterexample must itself fail to descend. That
failure forces the mean of `2^(-D_i)` over its odd steps to be at least
`3 N g(q)/q`, where `D_i >= 0` is the depth of the walk below the
contraction line. The maximum possible mean is `3 s* ≈ 0.7214`. Since
`2^(-{i alpha}) > 1/2`, the fraction `phi_D` of odd steps deeper than `D`
satisfies `phi_D (1/2 - 2^-D) <= 0.7214 - mean`.

Findings at `N = 2^71`, certified with exact intervals:

- 146 resonant lengths up to `10^12`. 73 of them can be a first block, and
  the smallest is 72,057,431,991.
- A first block of 72,057,431,991 odd steps needs mean at least 0.541,
  which is 75% of the maximum. At most 41.4% of its odd steps can lie more
  than 4 bits below the line. So on at least 58.6% of about `7.2 * 10^10`
  odd steps, the orbit stays below `16 n0 (1 + 10^-11)`.
- The confinement is not uniform. Multiples of the convergent
  denominator 137,528,045,312 need mean only 0.046, so they are almost
  unconstrained. At a convergent the Diophantine gap is so small that
  descent failure costs nothing.
- The length 6,586,818,670 unlocks only at a cumulative 99,403,550,326 odd
  steps. So no counterexample can start with two short resonant blocks.

The structure localizes the obstruction to convergent-denominator lengths,
but the random-walk heuristic already makes that case negligible. A
non-contracting prefix of `t` steps has density about `2^(-0.05 t)`, which
is `2^(-5.7e9)` at the first allowed length. The theorems here add
structure, not a proof.

## How RAD is used

- `main.rad` runs one forked world per floor through `simulate_many`.
  `assert_only_changed` checks that each fork wrote only its evidence.
- `Submit` proposes `SafeRange` intents, and `Tabulate` folds them into the
  frontier. The `GrowsWithFloor` constraint rejects a certificate whose
  exponent drops below 0.49.
- Leaf status is proposed through laws (`Establish`, `Obstruct`), so
  `why(...)` traces every claim to the proposals that set it.
- `audit.rad` checks the theorem against direct iteration for floors `2^3`
  through `2^15`, over all `n < 2^18`. The first contraction is detected
  exactly through `tau = ceil(q alpha)`. It finds zero violations in about
  3.3 million (n, floor) pairs.

Building this found a compiler bug. Inside a counted `for i in range(..)`
loop, a read of a local that is reassigned after the loop was rewritten to
`MoveLocal`. The `ForRangeNext` back-edge did not end the last-use window,
so the second iteration read `nil`. `MatchState` had the same gap. The fix is
in `core/vm/src/compiler/emit.rs`, with regression tests in
`compiler/tests/regressions.rs`.
