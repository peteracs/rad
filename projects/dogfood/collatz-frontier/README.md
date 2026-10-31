# The Collatz proof frontier as a RAD world

`frontier.rad` encodes this session's results as a proof-dependency graph.
Statements are entities with a `Claim` status. Reductions and refutations are
proposals, resolvers derive each statement's status, and settlements run
bottom-up so every level reads the committed status of the level below.
`why(collatz, Claim)` shows exactly which open statement blocks the root. The
run takes 0.06 s.

## Status

| statement | status | basis |
|---|---|---|
| ladder envelope | proved | `collatz-barrier/ladder` |
| feedback bound | proved | `collatz-shadow` |
| mirror law | proved | `collatz-automatic` |
| cycle length >= 72,057,431,991 odd steps | proved | `collatz-steer/cycle_bound` |
| Baker-type bound for log2 3 | cited | Baker; explicit bounds by Rhin and successors |
| convergence below 2^71 | cited | Barina |
| descent at `tau` for `q_tau(n) <= q*(n)`, `q*(2^71) = 72,057,431,990` | proved | `collatz-descent` (no Baker below 2^111) |
| `sigma(n) = tau(n)` for `n < 2^36` and whenever `tau(n) <= 301,993` | proved | `collatz-descent/cst.rad` |
| resonant block structure of a minimal counterexample | proved | `collatz-descent/blocks_main.rad` |
| `descent_from_tau` | open | needs `tau_sqrt` (below 2^111) or `tau(n) <= n^eps` |
| `tau_sqrt`: `tau(n) <= ceil(q*(n) log2 3) ~ sqrt(n)` | open | the sharpened frontier |
| **`tau_poly`: tau(n) <= n^eps for large n** | **open: the frontier** | |
| `collatz` | open | reduces to `descent_from_tau` plus verification |

## Branches closed, with evidence

- **Automatic certificates.** Stopping-time Hankel rank grows about 1.75x per
  digit, equally for 3x-1; digit-weight counting is contradictory.
- **Residue templates.** The exact Karp witness is the 2-adic point -1 at
  every depth.
- **Statistical bias.** Carry transport, epoch correlations and post-window
  trapping all match their exact nulls; the small-L repulsion vanished at
  L = 24..30.
- **Coalescence.** It removes a constant fraction of trapped residues, not an
  exponent.
- **Raising the cycle bound through the envelope.** At `N = 2^71` the
  record behind result 9 is dangerous even at the limit slope
  (`N g / q ≈ 0.1806 < 1/(6 ln 2)`), so the bound stays 72,057,431,991.
- **Excluding cycles analytically.** On 3x+d, 302 real cycles pass every
  envelope, resonance and drift condition. Primitive cycles there exist only
  through `gcd(d, 2^j - 3^q) > 1`, so only divisibility separates 3x+1.
- **Attempted sub-reductions of the frontier.** Counting is circular; the
  mirror and exit laws stop at the exit; residue classes cover only finitely
  many depths.

## Reading

Every explored route ends at one statement: a sub-polynomial bound on the
coefficient stopping time. It holds by a wide heuristic margin. The random
model predicts `tau` about `20 log2 n`, measured record ratios stay at or
below 14.5, and only `n^eps` is required. No explored branch reduces it to
anything closed.
