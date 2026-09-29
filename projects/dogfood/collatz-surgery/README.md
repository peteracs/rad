# Map surgery: which residue classes carry convergence

Each forked universe runs a modified Collatz map in which one odd residue class
`r mod 2^k` uses `(3n-1)/2` instead of `(3n+1)/2`. Every universe scans
`n < 3000` for cycles and for escapes past `2^50`. `Outcome` intents feed
the `Map` resolver. The run covers 28 universes (`k = 3, 4, 5`) in 1.4 s.

| surgered class | outcome |
|---|---|
| every `r = 3 (mod 4)`, all k | harmless: no new cycles, no escapes |
| `r = 9 (mod 16)` (k = 4, 5) | harmless |
| k=4: r = 1, 13; k=5: r = 1, 5, 13, 17, 29 | new cycles (e.g. min 7, length 8; min 103, length 46) |
| `r = 5 (mod 8)` | 2987 of 2999 starts escape: near-total divergence |
| `r = 21 (mod 32)` | 125 escapes |

Every new cycle passes through its surgered class.

**Why the harmless classes are harmless.**

- For `n = 3 (mod 4)`, the flipped step `(3n-1)/2` is even, so the orbit
  reaches `(3n-1)/4 < n` within two steps.
- For `n = 16m + 9`, the flipped orbit runs `24m+13 -> 36m+20 -> 18m+10 ->
  9m+5 < n`, and none of the intermediate values lies in a surgered class.

Harmful classes are those where the flipped step turns a halving step into a
growth step.

**Robustness barrier.** Flipping the sign of `+1` on a single residue class
that holds only 1/16 of the odd numbers already creates cycles or mass
divergence. So any correct proof of Collatz must depend on the sign of the
`+1` on arbitrarily thin residue classes. Averaged, density-based or local
arguments that are insensitive to those signs cannot succeed. This
generalizes the single 3x-1 calibration into a family of nearby false
statements that any candidate argument must distinguish from 3x+1.
