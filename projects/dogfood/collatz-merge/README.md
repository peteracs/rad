# Coalescence against the trapped set

Descent asks whether a trajectory drops below its own start. Coalescence is
weaker: if the trajectory of `n` meets the trajectory of any smaller number,
then `n` converges whenever that number does. This workload asks whether
coalescence can remove residues that descent cannot.

## Exact residue-uniform merge criterion

Let `w` and `w'` be parity words with the same length `a` and the same odd
count `q`, with offsets `C` and `C'` (built by `C <- 3^b C + b 2^t`). Suppose

```text
C' - C = 3^q d,   d > 0.
```

Then every `n` following `w` satisfies `T^a(n) = T^a(n - d)`, and `n - d`
follows `w'`. By Terras uniqueness, the partner's residue is automatic. The
simplest instance is the classical pair `8k+5`, `8k+4`: `C = 1`, `C' = 4`,
`d = 1`.

## The first family (proved)

The word `o^k e e` (k odd steps, then two even steps) has
`C = 3^k - 2^k`. The word `e o^(k-1) e o` has `C' = 2 * 3^k - 2^k`. So
`C' - C = 3^k` and `d = 1` for every `k`. Every `n` that begins with an odd
run of length `k` followed by two even steps merges after `k + 2` steps with
`n - 1`. The word is trapped (never contracting) once `k >= 4`, which is
why it first appears at length 6. `family.rad` identifies it at every length
from 6 to 14 and checks the merge on 199 integers per length. All merge.

## The merge sieve on the trapped set (`merge.rad`, `local.rad`)

`merge.rad` searches every word of each `(a, q)` class for a larger offset in
the same residue class mod `3^q`. It is exact, but its persistent map copies
on every insert, so depth 18 took 93 s. `local.rad` instead tests each trapped
prefix directly against partners `n - d` for small `d`. It reproduces the
exact counts at depths 14, 16 and 18, and runs depth 20 in 2.7 s.

| depth | trapped `|S_K|` | survive merging | fraction | dominated prefixes first appearing at lengths 18, 19, 20 |
|---|---|---|---|---|
| 14 | 734 | 593 | 0.808 | - |
| 16 | 2114 | 1720 | 0.814 | - |
| 18 | 7495 | 6104 | 0.814 | 7 |
| 20 | 27328 | 22246 | 0.814 | 7, 13, 27 |

Findings:

- Every merge that acts on a trapped residue uses `d = 1`: offsets up to 64
  were searched through depth 18. In the trapped regime, coalescence means
  consecutive integers merging.
- Up to length 17 the only dominated trapped prefix is the `o^k e e` family.
  From length 18 new consecutive-merge families appear, and their number
  roughly doubles per level (7, 13, 27).
- The surviving fraction settles at about 0.814. Coalescence removes a
  constant 18.6% of the trapped set. It does not change the exponential
  decay rate of `|S_K|`.

## What this does and does not give

Coalescence is a genuine extra mechanism: it certifies convergence for
trapped residues that descent cannot touch. But within the depths computed,
it keeps pace with the trapped set rather than outgrowing it. It improves a
constant factor, not the exponent. So it does not close the gap between
`S ∩ N` and the empty set. The `o^k e e` family generalizes the classical
`8k+4 / 8k+5` pair. Consecutive-integer coalescence is well studied (for
example Garner's work on consecutive numbers with equal stopping time), so
novelty is not claimed.
