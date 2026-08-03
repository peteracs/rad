# Collatz structural laboratory

`projects/dogfood/collatz-lab/` asks a narrower and more useful question than
"did a bounded search find a counterexample?":

> What exact arithmetic shape must any counterexample have, and which pieces
> of that shape can RAD eliminate in bulk with an independently checkable
> reason?

The Collatz conjecture remains open. Tao proved that almost every orbit, in
logarithmic density, reaches below any prescribed function tending to
infinity; this is much stronger than earlier almost-all results but is not an
all-integers proof. Current distributed computation has verified convergence
past `2^71`, providing a rigorous floor for a hypothetical least
counterexample. See [Tao's paper](https://arxiv.org/abs/1909.03562) and
[Barina's live verification project](https://pcbarina.fit.vutbr.cz/).

## Exact affine residue certificates

Use the shortcut map

```text
T(n) = n/2          if n is even
       (3n+1)/2     if n is odd.
```

One residue class modulo `2^d` fixes its first `d` parity decisions. Every
prefix is therefore an exact affine map

```text
T^j(n) = (3^a n + b) / 2^j.
```

When `3^a < 2^j`, every `n > b/(2^j-3^a)` descends. If this threshold is below
the verified `2^71` floor, the complete residue subtree is impossible for a
least counterexample. The project-owned `affine_residue_profile()` extension
exploits this proof: it expands only unproved nodes and counts all descendants
of a pruned node algebraically. RAD itself supplies only the generic
`load_extension()` boundary.

The depth-26 certificate is:

| Quantity | Exact value |
|---|---:|
| Residue classes | 67,108,864 |
| Classes pruned by certified descent | 66,071,490 |
| Survivor cylinders | 1,037,374 |
| Survivor fraction | 1.545807719% |
| Tree nodes actually expanded | 2,454,892 |

This replaced per-start interpreted tracing with a domain-general affine
parity kernel and whole-subtree pruning. Warm optimized end-to-end tests ran
the entire RAD study twice (record plus deterministic replay) in 1.1–1.6
seconds; that includes parsing, constraints, provenance, certificate
construction, and the 4.54-million-word cycle search, not just the residue
kernel.

## Why the survivors look dangerous

Multiplicative growth over a prefix is controlled by `3^a/2^j`. To avoid
contraction indefinitely, an orbit needs an asymptotic odd-step density of at
least

```text
1/log2(3) = 0.630929...
```

The geometric Syracuse heuristic instead gives mean valuation 2 after each
odd step, corresponding to negative average log drift. The certificate does
not assume independence: it counts the exact exceptional residue tree.

Its clearest branch is `2^26-1`, a finite positive shadow of 2-adic `-1`. It
takes 26 consecutive odd shortcut steps and peaks at `4,580,524,758,860`.
Nevertheless it drops below its start after 143 shortcut steps, with 90 odd
steps: density `90/143`, just under the critical threshold. This is the local
form a divergent counterexample would need to reproduce forever without the
eventual compensating divisions.

There is a crucial quantifier trap here. The compatible residues
`2^d-1 (mod 2^d)` survive the all-odd prefix at every finite depth, but their
inverse limit is the 2-adic integer `-1`, not a positive integer. The finite
witness changes from `2^d-1` to `2^(d+1)-1` as the depth grows. A genuine
positive counterexample would instead have to give one eventually constant
ordinary integer whose residue survives every deeper tree. Thus an infinite
2-adic survivor branch is necessary but not sufficient; the dogfood makes
that distinction explicit instead of mistaking a family of long excursions
for one divergent orbit. It also checks the boundary object directly:
`T(-1)=-1`. This is a real fixed point of the extended map, but it lies
outside the conjecture's positive-integer domain.

## Exact cycle certificates

For an odd-only valuation word `(a_0,...,a_{q-1})`, composition gives

```text
n = C(a_0,...,a_{q-1}) / (2^(sum a_i) - 3^q).
```

`affine_cycle_summary(3, 1, ...)` exhausts the finite word box, checks positivity and
divisibility, then replays the exact valuations. The dogfood checks 4,540,385
words with at most ten odd terms and 24 total divisions. Ten encode repeated
traversals of the trivial cycle at `1`; zero encode a nontrivial positive
cycle. Stronger cycle exclusions are known in the mathematical literature;
this finite box exists to exercise and certify the causal pipeline.

## RAD execution model

```text
project-owned affine extension via load_extension()
        ↓
eight fork_with() low-bit universes
        ↓
simulate_many() exact tree kernels
        ↓
eight residue findings + one cycle finding
        ↓
one Causal-Laws resolver
        ↓
Candidate Constraints audit the complete evidence
        ↓
atomic commit + why() fan-in + event ancestry
```

The same findings are submitted in forward and reverse order and must produce
identical components. `verify_certificate.py` then recomputes the residue
tree, all-odd escape, and all 4.54 million cycle words using Python big
integers and no RAD code.

## Scope

The result is exact for its finite depth and cycle box. It proves neither
that the survivor tree has no infinite positive-integer path nor that no much
larger cycle exists. What it adds is an executable structural statement:

```text
A counterexample must be an exponential-divisibility cycle,
or an infinite coherent high-odd-density path through every survivor tree.
```

That is the right interface for subsequent mathematics: extend the certified
pruning rules, rather than merely increase a trajectory counter.
