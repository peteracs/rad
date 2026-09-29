# Epochs and counterfactual null worlds

The shadow fork proved that large-value Collatz dynamics is 2-adic up to a
feedback of `O(q/n)`. That makes an epoch the natural unit of time: from `x`,
run exactly `L(x) = bitlength(x)` steps.

```text
E(x) = T^L(x) = (3^q x + C) / 2^L,   with bit length about 1.585 q.
```

Within one epoch, Terras's bijection says the `L` parity bits are a
scrambling of `x`'s own `L` bits. Over all `L`-bit `x`, the odd count `q` is
therefore exactly `Binomial(L, 1/2)`. An epoch multiplies length by about
`1.585 q / L`; the average factor is `0.79`, and divergence needs a sustained
run of epochs with `q/L > log_3 2 = 0.631`.

**The question.** Are consecutive epochs independent? If an expanding epoch
forced a contracting one, that would be a pointwise self-correction
mechanism.

## The RAD trick: seeded counterfactual null worlds

`epoch_worlds.rad` enumerates every `L`-bit number for `L = 12, 14, 16`, one
forked world per `L`. Each world computes the real pairs (epoch 1, epoch 2)
and a counterfactual null that pairs each first epoch with a second epoch of
the same length from a different start, using a seeded permutation. `Reading`
intents go to the `Judge` resolver, and `why(verdict, Verdict)` records which
worlds produced the verdict. The run takes 4.6 s.

## Results

Densities are in thousandths. "Both expand" counts starts whose two
consecutive epochs both exceed 0.631.

| L | both expand: real | null, unstratified | null, length-matched | largest next density after `d1 >= 0.8`: real / unstratified / matched | correlation: real / matched |
|---|---|---|---|---|---|
| 12 | 65 | 88 | 58 | 0.750 / 0.833 / 0.750 | -0.029 / -0.047 |
| 14 | 293 | 360 | 293 | 0.700 / 0.875 / 0.700 | -0.029 / -0.043 |
| 16 | 387 | 604 | 389 | 0.761 / 0.909 / 0.761 | -0.023 / -0.040 |

**A false discovery, caught.** Against a naive shuffle, the real dynamics
looked self-correcting: 36% fewer back-to-back expansions at `L = 16`, and a
visibly lower ceiling after strong expansions. The cause was length. An
expanding epoch produces a longer number, and a longer epoch's density is
more concentrated around 1/2. Once the null keeps the second epoch's length
fixed, real and counterfactual match: 387 against 389, and identical
ceilings. It is also not a mechanism in principle, because divergence depends
on absolute growth in bits, and a longer epoch at the same excess density
grows more.

**Conclusion.** Conditioned on length, consecutive epochs behave
independently. There is no hidden self-correction at the epoch scale for a
Lyapunov function to exploit.

**Run-time history.** The first stratified version took 8.3 s, then 10 s and
12 s while I fixed the wrong bottleneck. Timing individual worlds showed the
real cost: every null world recomputed the epochs. The final version computes
each length's data once and summarizes it twice.
