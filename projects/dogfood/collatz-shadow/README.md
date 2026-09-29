# The 2-adic shadow fork

This probe runs every orbit twice. The real world runs `x -> x/2` and
`(3x+1)/2`. The shadow world follows the same parity word without the `+1`,
so its size is exactly `log2 n - W_t`, where `W_t = t - q_t log2 3`. The
worlds differ only in the additive accumulator `C_t`, and that accumulator is
the only channel through which the Archimedean fact that `n` is a positive
integer feeds back into the dynamics. `shadow_fork.rad` runs one forked world
per start. `Gap` intents go to the `Gather` resolver, a constraint requires
the ladder bound to hold, and `why(feedback, Feedback)` records the
provenance. The run takes 0.1 s.

## Feedback bound (a corollary of the ladder theorem)

Since `T^t(n) = (3^q n + C_t) / 2^t`, the feedback is

```text
eta_t = log2 T^t(n) - (log2 n - W_t) = log2(1 + C_t / (3^q n)).
```

While the walk has not contracted, every prefix keeps `2^(e_i) <= 3^i`, so
`C_t / 3^q <= C_q / 3^q`, the extremal offset. The sharp envelope of
`../collatz-barrier/ladder` then gives, for every `n`:

```text
0 <= eta_t <= log2(1 + (0.24046 q + 0.6882) / n).
```

The probe checks this along every orbit and finds no violations (up to the
resolution of the logarithm table).

## Measurements on record holders

| start | steps | trapped steps | max `eta` while trapped | first step with `eta > 0.1` bit | value there |
|---|---|---|---|---|---|
| 27 | 70 | 58 | 0.125 | 27 | 251 |
| 703 | 108 | 80 | 0.0041 | 99 | 17 |
| 10087 | 142 | 104 | 0.0016 | 132 | 11 |
| 35655 | 204 | 134 | 0.00035 | 195 | 17 |
| 270271 | 256 | 163 | 0.0010 | 247 | 53 |
| 626331 | 319 | 175 | 0.00032 | 316 | 8 |
| 8400511 | 429 | 213 | 0.00060 | 404 | 4616 |
| 63728127 | 592 | 375 | 0.00058 | 584 | 26 |

## What it shows

- Throughout the entire excursion, climb and descent alike, a large orbit is
  its 2-adic shadow to within about `10^-3` bits. Positivity is dynamically
  invisible there, as the bound predicts: the feedback is `O(q/n)`.
- The feedback becomes visible (`> 0.1` bit) only in the last few steps,
  when the orbit is at values like 8, 11, 17 or 26. Positivity acts in the
  endgame.
- So for large values the dynamics is, provably and quantitatively, 2-adic
  dynamics. The only way a positive integer differs from the trapped 2-adic
  points (such as -1 = ...111) is its digit tail: a positive integer's 2-adic
  expansion ends in zeros, a negative one's in ones. Any mechanism that
  excludes divergence must therefore act on that symbolic tail property,
  which the walk reaches after about `log2 n` steps. It cannot come from size
  estimates during the excursion, because the feedback bound proves they
  carry almost no information there.

This relocates the missing mechanism rather than supplying it. The target
becomes a statement about 2-adic walks started from finite digit strings, not
about magnitudes.
