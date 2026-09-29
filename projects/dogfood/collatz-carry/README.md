# Carry transport in 3x+1

This workload asks whether multiplying by 3 moves information from the low
end of a number (parity) to its high end (size) in a structured way that
could couple the two metrics.

## Exact carry provenance (`carry.rad`, `check.rad`)

For `3x = x + 2x`, the carry into bit `p` is fixed at the nearest `k < p`
with `x_k = x_(k-1)` (a generate or kill position). The causal depth of
output bit `p`, meaning how far below it its cause lies, is therefore the
length of the alternating run `...0101` of `x` just below `p`. At the bottom,
`nu2(3x+1)` equals the number of low bits where `x` agrees with the 2-adic
expansion of `-1/3 = ...010101`.

`check.rad` verifies this halving law for all 100000 odd `x < 200000` in
3.5 s. Both facts are elementary. Their consequence is that every long carry
chain, the only channel by which low bits reach high bits, is a local copy of
the digit pattern of `1/3`.

## Is the transport random? (`transport.rad`)

Three forked worlds measure carry-chain statistics. The results go through
`Observation` intents to the `Compare` resolver, and
`why(comparison, Comparison)` records the provenance. The run takes 2.2 s.

| source | samples | long alternating runs (>= 4) per bit | mean agreement with -1/3 |
|---|---|---|---|
| powers of 3 | 600 | 0.0617 | 1.50 (all bits, not odd-conditioned) |
| Collatz trajectory, odd steps | 600 | 0.0625 | 1.996 |
| random odd numbers | 600 | 0.0619 | 1.995 |

Carry-chain density agrees within about 1.3%, and the halving statistic along
the trajectory matches random odd numbers to three digits. At this
resolution, the transport by `x -> 3x+1` is statistically indistinguishable
from random, which is what the normality heuristics predict.

**Artifacts caught and removed.**

- The first run started at `2^1000 - 1`. Its first ~1000 odd steps are the
  all-ones climb `3^k 2^(1000-k) - 1`, which showed 23% fewer long
  alternating runs than random and a halving mean of 1.415. That was the
  start family, not dynamics. The corrected run uses a pseudo-random
  1200-bit start with 300 burn-in steps.
- The random baseline's largest excess (4.9, against about 10 for the other
  two sources) reflects short-range structure in the linear congruential
  generator. The maximum over 600 samples of about 1000 bits should be near
  `log2 600`. The Collatz and power-of-3 maxima are consistent with true
  randomness.
- The first version took 12.4 s; the sample sizes now fit the 5-second cap.

## Conclusion

The coupling between parity and size runs entirely through local copies of
the `1/3` digit pattern, and along real trajectories those copies occur with
random statistics. No hidden bias appears that a Lyapunov function could
exploit. This is consistent with the open normality questions: the transport
looks random, and proving that it is random, pointwise, is the open problem.
