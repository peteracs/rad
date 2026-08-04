# Bounded-support proof program

This note separates the proved reductions, the exact computed lemmas, and the
one uniform statement that would turn the dogfood into a proof of Collatz.

Use the shortcut map

```text
T(n) = n/2          (n even)
       (3n+1)/2     (n odd).
```

For a depth-`d` input cylinder `n = r (mod 2^d)`, its first `d` parity
decisions are fixed, so

```text
T^d(n) = (A_d n + C_d) / 2^d,
```

with `A_d` a power of three and `C_d >= 0`.

## Proved reductions

### Cylinder descent

If `A_d < 2^d`, every member of the cylinder above

```text
C_d / (2^d - A_d)
```

falls below its start at depth `d`. When that threshold is below the verified
`2^71` convergence floor, the complete cylinder is impossible for a least
counterexample. This is the exact pruning rule used by the generic affine
kernel.

### Support death depth

Let `H(w)` be a certified depth by which every least-counterexample cylinder
whose input has at most `w` set binary digits is dead. An exhausted support
leaf is one fixed natural number, not an unknown-high-bit cylinder, so the
kernel may also close it on direct exact descent.

### Renewal deadline

Write the set-bit positions of a hypothetical least counterexample as

```text
p_1 < p_2 < ... < p_W.
```

If `H(w)` is finite, then

```text
p_(w+1) < H(w).
```

Before bit `p_(w+1)` is read, the prefix contains at most `w` ones. If its
position were at least `H(w)`, that prefix would already belong to a dead
cylinder. This lemma is purely combinatorial and does not extrapolate the
computed data.

### Why finite support is sufficient

Every positive integer has a finite number `W` of set binary digits. Hence:

```text
H(w) finite for every finite w
    => every positive integer has a certified first descent
    => strong induction gives convergence to 1.
```

The first implication uses the cylinder rule above for starts at or above the
verified floor; starts below it are already verified.

## Exact computed base

The current deep certificate gives

```text
w      0  1  2  3   4    5    6    7    8    9   10
H(w)   1  2  4  7  59  137  214  365  552  634  818
```

It follows rigorously that a least counterexample must contain at least eleven
set input bits. The record renewal positions are

```text
0, 1, 2, 6, 39, 47, 98, 339, 530, 592,
```

each strictly before the preceding support deadline. The independent Python
implementation exhausts the same tree through `w=7` and validates every
reported terminal witness through `w=10`.

The prefix-slope audit gives the same complete arrays through `w=10`. The
current records are therefore governed by multiplicative meanders, not by a
contracting coefficient rescued by the additive remainder.

## Exact support-eleven witness lower bound

The counterexample-guided frontier portfolio ranks retained exact states by
their zero-tail survival, while competing against headroom, probe-size, and
seeded deterministic objectives in separate RAD universes. It reconstructs
the complete `H(0)..H(10)` boundary sequence before entering an unknown layer.

Its best support-eleven witness has set positions

```text
0, 1, 2, 6, 39, 47, 98, 339, 516, 553, 715.
```

Independent exact arithmetic proves

```text
3^q_j >= 2^j       for every 1 <= j <= 944,
3^q_945 < 2^945,
T^j(n) >= n        for every 1 <= j <= 944,
T^945(n) < n.
```

Thus the slope death boundary at support eleven is at least 945. The witness
then reaches 1 after 4,375 shortcut steps. This does not prove that 945 is the
first empty support-eleven depth: the beam proves survival of retained states,
not exhaustion of discarded states. An exhaustive depth-945 attempt exceeded
the current ten-minute development budget, exposing the need for resumable
exact proof partitions rather than licensing an extrapolation.

## The uniform lemma to prove

The data satisfy

```text
H(w+1) < 2 H(w)    for 5 <= w < 10.
```

A sufficient theorem is the following renewal bound.

> For all sufficiently large `w`, every legal `(w+1)`-st one-bit placed before
> `H(w)` produces a fixed zero-tail representative that is excluded before
> depth `2 H(w)`.

The renewal-deadline lemma then gives `H(w+1) <= 2 H(w)`. Together with the
finite exact base, induction makes every `H(w)` finite and proves Collatz.

This is the next target for invariant synthesis. A useful invariant must be
local to the generic affine anchor state—coefficient, denominator, terminal
probe, residue, and remaining renewal credit—so RAD can check it on every
transition. Merely fitting the finite sequence is not a proof.

## What computation should do next

1. Search for a transition potential whose value bounds the remaining
   zero-tail lifetime after one renewal.
2. Use counterexample-guided synthesis: candidate potentials live in forked
   RAD worlds; an exact native anchor supplies a violating state; `why()`
   records which state invalidated which coefficient choice.
3. Require the final potential to reduce to checked integer/rational
   inequalities on both affine branches. No floating-point or sampled-state
   conclusion is admissible.
4. Keep the paradoxical branch separate. Equality of the slope and full
   profiles through support ten is evidence, not permission to delete it from
   a universal proof.
