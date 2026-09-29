# The sharp envelope ladder at the first Collatz contraction

[RESULTS.md](../RESULTS.md) proves one sharp linear bound on first-contraction
offsets, `C/3^q <= q/4 + 11/108`. This note determines the **whole family** of
sharp linear bounds for every slope down to `s* = 756421/3145728 ≈ 0.2404597600`.
No finite linear bound exists below the limit slope `1/(6 ln 2) ≈ 0.2404491735`.
The certified range therefore reaches within 0.0044% of it.

The optimal bounds form a convex ladder. Its vertices are separated by the
continued-fraction denominators 2, 12, 41 and 665 of `log_2 3`.

This restricts the additive term of Collatz descent thresholds. It is not a
proof of the Collatz conjecture or of Terras's coefficient-stopping-time
conjecture.

## Setup

We keep the notation of RESULTS.md. The shortcut map is `T(n)=n/2` or
`(3n+1)/2`. `C_q` is the largest affine offset among first-contraction words
with `q` odd steps, and satisfies `C_{q+1} = 3C_q + 2^{floor(q alpha)}` with
`alpha = log_2 3`. Every first-contraction word with `q` odd steps has offset
`C <= C_q`. Write

```text
f(x)   = 2^-{x}                       (right-continuous, period 1)
F_q(x) = sum_{i<q} f(x + i alpha)
M_m    = max_x F_m(x)
I      = integral_0^1 f = 1/(2 ln 2)
```

Equation (1) of RESULTS.md gives `C_q/3^q = F_q(0)/3`. Therefore a bound
`F_q(0) <= sigma q + g` is the same as `C_q/3^q <= (sigma/3) q + g/3`.

## Theorem 1 (certified sharp family)

Let

```text
V = {0, 1, 3, 15, 27, 68, 80, 121, 133, 174} ∪ {174 + 665k : 1 <= k <= 11},
```

so that the largest vertex is 7489. For every real `s >= s*`, every `q >= 0`,
and every first-contraction word with `q` odd steps,

```text
C/3^q  <=  C_q/3^q  <=  s q + beta(s),     beta(s) = max_{v in V} (C_v/3^v - s v).
```

The bound is attained at the maximizing vertex, so `beta(s)` is the sharp
intercept for slope `s`. Each `v` in `V` is the unique maximizer on a
nonempty open interval of slopes. Equivalently, the edges of the upper
concave hull of `{(q, C_q/3^q)}` with slope at least `s*` have exactly the
vertices `V`.

| slope s | sharp intercept beta(s) | attained at q |
|---|---|---:|
| 1/4 | 11/108 = 0.1018518519 | 3 |
| 0.245 | 0.1171588731 | 15 |
| 0.2425 | 0.1546588731 | 15 |
| 0.241 | 0.2173010276 | 121 |
| 0.2405 | 0.4473073427 | 4829 |
| s* = 756421/3145728 | 0.6881754927 | 7489 |

The first row recovers the existing theorem. The table's decimals are
exact rationals rounded to ten places. The verifier computes them exactly.

The hull's edge slopes decrease through 1/3, 0.259259, 0.245026, 0.241728,
0.241602, 0.241223, 0.241098, 0.240720, 0.240595 and 0.240567. After that the
665-rungs descend from 0.240556 to 0.240462 in steps of almost exactly 1.05e-5.

### Three lemmas

**Lemma A (candidate phases).** `F_m` strictly decreases between its
discontinuities, which lie at `x = {-h alpha}`, `0 <= h < m`. Hence
`M_m = max_h F_m({-h alpha})`, and each summand at such a phase is
`v_k = 2^-{k alpha}` for an integer `k` with `|k| < m`. RESULTS.md, section 2,
proves this.

**Lemma B (block bound).** Suppose `M_L <= sigma L` and set
`G = max_{0<=r<L} (M_r - sigma r)`. Then `F_q(x) <= sigma q + G` for all
`q >= 0` and all `x`.

*Proof.* Write `q = kL + r` with `0 <= r < L`. Cut the orbit segment into `k`
blocks of length `L` followed by one block of length `r`. Every block is an
`F` sum at some phase, so `F_q(x) <= k M_L + M_r <= sigma kL + sigma r + G`. QED.

**Lemma C (bootstrap).** Under Lemma B, for all `p >= Q`:
`F_p(0) - sigma p <= F_Q(0) - sigma Q + G`.

*Proof.* `F_p(0) = F_Q(0) + F_{p-Q}(Q alpha)`. Apply Lemma B to the second
term. QED.

### The certificate

Take `L = 15601`, a convergent denominator of `alpha`, and
`sigma* = 3s* = 756421/2^20`. The finite checks are:

1. **Block contraction.** `M_15601 <= sigma* · 15601`, with margin at least
   0.1431. This uses outward-rounded enclosures of all `2·15601-1` values
   `v_k` and all 15601 candidate phases.
2. **Block excess.** `G <= 2.3189812`, with the largest term at `r = 7841`.
   This checks 121,703,401 candidate phases over all `r < 15601`.
3. **Chords.** For consecutive vertices `a < b` and every `a < q < b`,
   `F_q(0)` lies strictly below the chord from `a` to `b`. The slopes of
   consecutive chords strictly decrease. The last chord, from 6824 to 7489, is
   steeper than `sigma*`.
4. **Tail ray.** For `7489 < q <= 14943`,
   `F_q(0) <= F_7489(0) + sigma* (q - 7489)`.
5. **Bootstrap.** `F_14943(0) - sigma* · 14943 + G < F_7489(0) - sigma* · 7489`,
   with margin 0.018818.

Checks 3 to 5 are exact. The independent verifier compares the true
rationals `3C_q/3^q` by integer cross-multiplication. Checks 1 and 2 use
rigorous integer enclosures.

**Proof of Theorem 1.** Fix `sigma = 3s >= sigma*`. Lemma B still holds at
`sigma`, with a `G` no larger than at `sigma*`.

- **q <= 7489.** Check 3 places every point under the concave polygon on `V`.
  So `F_q(0) - sigma q` is at most the value at one of the two endpoints of
  its chord.
- **7489 < q <= 14943.** Check 4 gives
  `F_q(0) - sigma q <= F_7489(0) - sigma 7489 - (sigma - sigma*)(q - 7489)`.
- **q > 14943.** Lemma C and check 5 give
  `F_q(0) - sigma q < F_7489(0) - sigma* 7489 - (sigma - sigma*) q`, and this
  is at most `F_7489(0) - sigma 7489`.

Hence `sup_q (F_q(0) - sigma q) = max_{v in V} (F_v(0) - sigma v)`. Strict
concavity (check 3) makes each vertex the unique maximizer on an open slope
interval. Divide by 3. QED.

## Theorem 2 (the method is complete above the limit slope)

For every `s > 1/(6 ln 2)`, the sharp intercept
`beta(s) = sup_q (C_q/3^q - s q)` is finite and attained. A finite certificate
of the form above computes it. For `s < 1/(6 ln 2)`, no finite intercept
exists.

*Proof.* The function `f` has total variation 1 on the circle: 1/2 along the
decreasing branch and 1/2 at the jump. The Denjoy–Koksma inequality gives
`|F_{q_n}(x) - q_n I| <= 1` for every convergent denominator `q_n` of `alpha`
and every `x`. Hence `M_{q_n} <= q_n I + 1`.

Let `sigma = 3s > I`. Choose `q_n >= 1/(sigma - I)`. Then `M_{q_n} <= sigma q_n`
and Lemma B applies.

For the bootstrap, pick `sigma'` in `(I, sigma)` and a second convergent
certificate at `sigma'`. It gives `F_Q(0) <= sigma' Q + G'`, so
`F_Q(0) - sigma Q + G` tends to minus infinity. The bootstrap therefore closes
at some finite `Q`, and the supremum is a maximum over `q <= Q`.

Below the limit, the phases `{h alpha}` equidistribute because `alpha` is
irrational. Hence `C_q/(q 3^q) = F_q(0)/(3q) -> I/3 = 1/(6 ln 2)`, and any
slope `s < I/3` leaves `C_q/3^q - s q` unbounded. QED.

This also explains the original proof. Its 12-step block works because 12 is
a convergent denominator. The rigorous probe table shows that no convergent
denominator up to 665 certifies `s*`:

| block L | M_L / (sigma* L), lower bound |
|---:|---:|
| 12 | 1.03534 |
| 41 | 1.01403 |
| 53 | 1.00754 |
| 306 | 1.00160 |
| 665 | 1.00050 |

## The convergent ladder

Within the certified range, every gap between hull vertices is 1, 2, 12, 41 or
665. The last four are the denominators of the convergents 3/2, 19/12, 65/41
and 1054/665 of `log_2 3`. The denominators 5, 53 and 306 never occur.

The mechanism is visible in the excess `E_q = F_q(0) - q I`. Stepping from
`v` to `v + q_n` adds `F_{q_n}({v alpha}) - q_n I`, and Denjoy–Koksma confines
that to `[-1, 1]`. Because `q_n alpha` is within `1/q_{n+1}` of an integer,
repeating the same step moves the phase very little. The increment therefore
changes slowly and almost linearly, and vertices form arithmetic
progressions.

On the 665-rungs the increments are 0.2341, 0.2131, 0.1922 and so on. They
fall by about 0.021 per rung, which matches `ln 2 · (665 alpha - 1054) · F_665`.
The ladder ends when the increment no longer beats the slope.

## Exploratory data (not certified)

A float64 scan of the strict records of `E_q` up to `q = 10^9` continues the
hull above the limit slope. The vertices are 8154, 39356, 40021, 150558,
10630758, 53866391, 64346591, 107582224 and 289991197. Their gaps are 665,
2·15601, 665, 111202−665, and further signed combinations of convergent
denominators.

The record excess grows slowly, from 2.306 at 8154 to 2.670 at 289991197. This
is consistent with the logarithmic growth expected for ergodic sums of a
function with a jump. It suggests that no finite intercept exists at the
critical slope itself. That question remains open here.

The scan is in [records.py](records.py). Its floating-point results are
exploratory and are not part of any claim above.

## Consequence for descent thresholds

Equation (4) of RESULTS.md says a first contraction of length `j` descends
when `n > C/(2^j - 3^q)`. Theorem 1 replaces `C <= 3^q (q/4 + 11/108)` with
`C <= 3^q (s q + beta(s))` for any `s >= s*`. At `s = s*`, the leading
coefficient of the threshold falls by 3.8% relative to the quarter-slope
bound. Intermediate slopes trade leading coefficient against intercept
exactly as the table shows.

## Scope and related work

The Denjoy–Koksma inequality, continued-fraction discrepancy bounds, and the
affine parity formalism are classical. Terras, and Rozier–Terracol, studied
the additive remainder. The contribution here is the complete sharp family on
`[s*, infinity)`, its certified ladder structure, and the observation that the
existing quarter-slope proof is the first rung of this family. Historical
priority is unresolved.
