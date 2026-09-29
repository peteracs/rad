# Does positivity push integers out of the trapped set?

## The target, sharpened

The coefficient-guided refinement in `../collatz-cegar` ended at a uniform
bound on the coefficient stopping time `tau(n)`. That bound can be weakened a
long way.

- At a first contraction, the sharp ladder envelope gives
  `C / 3^q <= 0.2405 q + 0.69`.
- `log2 3` has finite irrationality measure (Baker's theory; explicit bounds
  by Rhin and later authors). So `2^tau - 3^q >= 3^q tau^(-kappa)` for some
  constant `kappa`.
- Together, descent at step `tau(n)` holds whenever `n > poly(tau(n))`.

**Reduction (conditional on the cited Diophantine bound).** If
`tau(n) <= n^eps` for all `n >= N0`, for some `eps < 1/(kappa+1)`, then every
`n >= N0` descends. By strong induction, together with verification below
`N0`, that proves Collatz. It covers cycles and divergence alike. `N0` is
effective but may be very large.

By the tail property, `tau(n) > t` means the integer `n` itself lies in the
trapped cylinder `S_t`. So the bound says trapped cylinders avoid small
integers.

## Measurement: trapped counts against an exact fair-coin null

Terras's theorem makes `#{n < 2^L : tau(n) > t} = mu(t) 2^L` exact for
`t <= L`. Beyond the window, the null continues every exact trapped state at
step `L` with fair coins. Its survival probabilities come from an exact
backward recursion (`exact_null.rad`), so the null mean and variance are exact
and no random generator is involved. The real count follows the true
dynamics from the same states.

**Exactly-L-bit starts `n in [2^(L-1), 2^L)` (disjoint populations, so
independent across L).** Entries are `z x 100`:

| L | t = 2L | 3L | 4L | 5L | 6L |
|---|---|---|---|---|---|
| 16 | -7 | -24 | -11 | -212 | -65 |
| 18 | -128 | -59 | -70 | -51 | -361 |
| 20 | -272 | -378 | -170 | -142 | -57 |
| 22 | -37 | -28 | -155 | -174 | - |

**Calibration against 3x-1** (`signed.rad`, same exact test):

| map | L | z x 100 at 2L..6L |
|---|---|---|
| 3x+1 | 16 | -7, -24, -11, -212, -65 |
| 3x-1 | 16 | +22, +46, +177, +396, +345 |
| 3x-1 | 18 | +3, +84, +151, +27, -148 |

## Reading

- For 3x+1, all 19 z-scores are negative: real integers escape the trapped
  set faster than fair coins from identical states. For 3x-1, 8 of 10 are
  positive, as its trapped cycles (5, 7, 10 and the 17-cycle) require. The
  effect is sensitive to the sign of the offset, so it passes the
  calibration rule that every earlier statistical probe failed.
- The early-window deficit is not robust. At L = 20, 3L gave z = -3.78, but
  at L = 22 the same checkpoint gave -0.28. That is not the `sqrt(N)` growth
  of a real bias, so part of that signal was a fluctuation.
- The late deficit (t = 4L to 6L) is consistently negative across disjoint
  populations. Combining the four sizes gives about -2.9 at 5L and about
  -2.0 at 4L. That is suggestive, not decisive.

**Status at small L.** Up to `L = 22`, a sign-sensitive deficit appeared at
4 to 6 times the bit length, at about 3 sigma combined. Settling it needed
larger `L`, beyond what the VM could reach within the time cap.

## The 100x kernel settles it: no repulsion

`affine_trapped_survival(bits, d, marks)` is a native RAD builtin
(`core/vm/src/vm/builtins_impl/affine_walks.rs`). It enumerates the trapped
states by Terras's rule, continues the real dynamics in parallel, and
computes the exact fair-coin null by backward recursion, all in overflow-
checked `i128`. `native.rad` reproduces all 15 VM rows (L = 16, 18, 20)
exactly, in 0.08 s instead of about 5 s.

| map | L | z x 100 at 2L..6L | observed / expected x 1000 |
|---|---|---|---|
| 3x+1 | 22 | -37, -28, -155, -174, 103 | 997, 995, 951, 904, 1095 |
| 3x+1 | 24 | -176, 1, 25, 214, 20 | 991, 1000, 1004, 1074, 1012 |
| 3x+1 | 26 | 86, 145, 194, 318, 134 | 1002, 1008, 1022, 1070, 1051 |
| 3x+1 | 28 | -107, 11, 22, -93, 42 | 998, 1000, 1001, 987, 1010 |
| 3x+1 | 30 | 151, -3, -56, -317, -162 | 1001, 999, 997, 973, 974 |
| 3x-1 | 22..30 | mixed sign, -2.4 to +2.6 | 0.88 to 1.04 |

From `L = 24` on, 3x+1 shows no consistent deficit. Its z-scores change sign
(positive throughout at `L = 26`), and the ratios stay within a few percent
of 1. The isolated -3.17 at `L = 30` is what about 40 tests produce by
chance. The 3x-1 excess also disappears. For large starts, both maps match
the fair-coin null.

**Conclusion.** There is no repulsion of positive integers from the trapped
set beyond the Terras window. The small-L deficit came from small numbers,
where cycles and the `+1` feedback still act. For large starts, the
post-window trapping of integers is indistinguishable from fair coins, and
the reduction `tau(n) <= n^eps` receives no help from a hidden bias.

## Artifacts caught on the way

- A seeded linear congruential null produced implausibly tight spreads
  (2003 to 2012 at about 2000 survivors). It was replaced first by RAD's
  seeded `rand_bool` (which needs `io true` and must be called in the system
  body), and then by the exact backward recursion.
- Small starting values carry a sizable `+1` feedback. The test therefore
  uses exactly-L-bit starts, where the feedback bound is about `1e-4` bits.
