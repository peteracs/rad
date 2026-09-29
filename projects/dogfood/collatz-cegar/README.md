# Deriving the missing quantity by counterexample-guided refinement

This workload searches for the missing quantity backwards. It proposes a
template for a Lyapunov function `V`, refutes it exactly, and uses the
refutation's witness to decide which feature must be added. Every refutation
is a `Refuted` intent. The `Chain` resolver keeps the requirement ledger, and
`why(spec, Requirement)` shows which witnesses forced which features.

## Template 1: `V = K log2 n + g(n mod 2^k)`

States are residues mod `2^k`. Each residue has two possible successors,
depending on the unknown next bit. An odd edge grows `log2(3/2)`; an even
edge grows `-1`. A suitable `g` exists, for some `K`, exactly when every
abstract cycle has negative mean growth. Karp's algorithm finds the
maximum-mean cycle exactly (`abstract.rad`). The witness's parity word has an
exact 2-adic fixed point `C/(2^j - 3^q)`.

At every `k` tested (2, 4, 6, 8), the witness is the self-loop at `2^k - 1`:
parity `o`, fixed point `-1`, growth `0.585` bits per step. It is a genuine
2-adic cycle, so adding residue bits never removes it.

**Requirement 1:** `V` must use information that residues cannot provide.

## Refinement: counters and positivity

The witness `-1` is broken by a counter for how long `n` agrees with `-1`,
which is its trailing ones. For a positive integer that counter is at most
`L(n) = bitlength(n)`, because the digits run into zeros. Following the `-1`
loop can therefore be paid for out of a `log n` term. This is exactly where
positivity (the tail property) enters.

`cegar.rad` repeats the step: remove the handled witness, then find the next
strongest expanding cycle.

| round | parity word | exact 2-adic point | growth per step (micro-bits) |
|---|---|---|---|
| 0 | `o` | -1 | 584963 |
| 1 | `ooooooeoo` | -6625/6049 | 408856 |
| 2..5 | four words | denominator 1675 | 232749 |
| 6..14 | nine words | -760/31 and denominator 217 | 56642 |
| 15 | `eoo` | -10 (the -5 cycle) | 56642 |

(k = 8, 6.4 s. The k = 7 rerun within the 5-second cap takes 0.6 s and finds
the same ordering: -1, then rational points by decreasing growth.)

**Caveat.** Removing a witness's edges also destroys every other cycle through
those edges. That is far more aggressive than a counter. So the loop's
eventual "template succeeds" is an artifact and is not reported as a result.
Only the witness order is meaningful.

## What the refinement tower requires

The witnesses are the expanding rational points of the trapped set `S`,
found automatically in order of growth. Each needs its own agreement counter,
and each such counter is bounded by `L(n)` for positive `n`. So each single
witness is handled through positivity. But there are infinitely many
expanding periodic points (58881 up to period 20 in `collatz-steer`). A
finite family of counters never closes the tower.

The uniform version of all these counters is the trapping depth: the number
of steps `n` spends shadowing some trapped 2-adic point, which is its
coefficient stopping time `tau(n)`. The tower closes exactly when

```text
tau(n) <= C * log2 n   for all n >= 2
```

holds for some constant `C`. Then `V(n) = K log2 n + A tau-bound` works on
`S ∩ N`. This is the conjecture that stopping times are `O(log n)`, and it
would settle the divergence half (`S ∩ N = ∅`). The measured ratios
`tau(n) / log2 n` on record holders are 12.4 (27), 8.6 (703), 7.9 (10087),
8.9 (35655), 9.1 (270271) and 14.5 (63728127). They are bounded so far, as
the conjecture predicts, but not provably.
