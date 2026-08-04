# Collatz structural laboratory

`projects/dogfood/collatz-lab/` asks a narrower and more useful question than
"did a bounded search find a counterexample?":

> What exact arithmetic shape must any counterexample have, and which pieces
> of that shape can RAD eliminate in bulk with an independently checkable
> reason?

The repository's
[`bounded-support proof program`](https://github.com/peteracs/rad/blob/main/projects/dogfood/collatz-lab/PROOF_PROGRAM.md)
separates proved reductions, the exact computed base, and the remaining
uniform renewal lemma.

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
parity kernel and whole-subtree pruning. On the Windows development host, one
debug-CLI study completed in about 0.9 seconds; the parallel residue kernel
itself took roughly 1–2 ms and the exact 4.54-million-word cycle kernel about
0.28 seconds. Parsing, constraints, provenance, and certificate generation
make up the remainder. These are development observations, not portable
benchmark guarantees.

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

## Natural tails: finite integers versus 2-adic paths

An arbitrary branch of the parity tree may keep choosing new high input bits
forever. A positive integer cannot: beyond its bit length every input bit is
zero. `natural_tail.rad` makes that boundary condition explicit and follows
every surviving residue after its input has stopped changing.

The RAD program also computes an exact ballot model in ordinary pure RAD.
There are two progressively stronger ways for a length-`d` parity word to
look dangerous:

```text
terminal test:  3^q >= 2^d only at the final step
prefix test:    3^q_j >= 2^j at every prefix j
```

At depth 28 the terminal test retains `24,821,333` words, while the prefix
test retains `3,524,586`. The generic affine residue kernel independently
produces exactly the same prefix count. Thus the possible divergent shape is
not merely a word with many odd steps; it is an irrational-slope
ballot/meander path that never crosses the multiplicative boundary.

Each of the `3,524,586` finite representatives then receives a zero high-bit
tail. All acquire coefficient contraction and strict descent by shortcut step
395. The latest record is `217,740,015`; the largest peak is
`3,202,398,580,560,632`, reached from `210,964,383`.

The program also constructs the greedy boundary meander directly in pure
RAD: take an even parity whenever the coefficient can remain noncontracting,
and an odd parity only when required. Inverting that parity word produces a
compatible 2-adic input residue. At depth 61 its finite shadow is
`937,101,304,038,054,907`; input bit 59 is set, and bit 61 is forced to be set
next if the meander is to continue. If that forced bit is omitted, the finite
shadow descends immediately at step 62. This is an executable view of the
quantifier obstruction: the infinite boundary path survives by changing its
high input bits, while one fixed natural number eventually has no such bits
left.

This equality is a finite certificate, not a hidden proof assumption. For
general `n`, first actual descent is known to occur no earlier than first
coefficient contraction. Equality for every `n` is Terras's
coefficient-stopping-time conjecture. Recent work on paradoxical sequences
states this distinction explicitly and shows why a contracting coefficient
can still be defeated temporarily by the affine remainder; see the current
published analysis by
[Rozier--Terracol](https://arxiv.org/abs/2502.00948).

The dogfood therefore isolates the remaining alternatives without claiming
to settle them:

```text
a counterexample keeps one infinite prefix-noncontracting meander,
or it crosses the coefficient boundary paradoxically and never descends.
```

Seven scales (`8, 12, 16, 20, 24, 26, 28`) are settled through typed intents,
one resolver, Candidate Constraints, forward/reverse producer order, event
ancestry, `why()`, and record/replay. `verify_natural_tail.py` rebuilds all
reported counts and tail records using Python big integers without importing
RAD or the extension.

Dogfooding also found a semantic-payload cost outside the arithmetic loop.
The first version transported 1,024-slot histograms through every fork,
proposal, resolution, and provenance record. Since the exact latest stop is
395, the checked horizon is now 512; an unresolved tail still rejects the
candidate. On the development host that reduced the complete seven-scale RAD
run from roughly 4.3 seconds to about 1.8 seconds. This is a local measurement,
not a portable benchmark guarantee. The separate Python verifier was also
changed from one breadth-first frontier (about 85 seconds and high peak memory)
to 64 streaming low-bit lanes scheduled over a bounded process pool (about 32
seconds on the same host).

## Bounded binary support: excluding an infinite class

The residue tree still contains arbitrary 2-adic inputs whose high bits can
keep changing forever. `support_pressure.rad` exploits a fact unique to
ordinary integers: each has a finite number of set binary digits.

For each support budget `w`, the generic affine kernel follows only unpruned
proof anchors where a new input bit is set and streams the zero tail between
anchors. With the verified `2^71` convergence floor, the bounded-support
trees become empty at these exact depths:

| Maximum input one-bits | First empty depth |
|---:|---:|
| 0 | 1 |
| 1 | 2 |
| 2 | 4 |
| 3 | 7 |
| 4 | 59 |
| 5 | 137 |
| 6 | 214 |
| 7 | 365 |
| 8 | 552 |
| 9 | 634 |
| 10 | 818 |

Thus a hypothetical least counterexample must have at least eleven `1` bits.
This is an infinite-class exclusion, not another largest-checked-integer
claim. For any fixed integer in one of those classes, emptiness supplies an
exact affine prefix below the start; strong induction and the verified floor
then finish that integer.

The deepest weight-ten witness has set-bit positions

```text
0, 1, 2, 6, 39, 47, 98, 339, 530, 592.
```

It survives through depth 817 and is pruned when its zero tail reaches depth
818. The pattern makes the obstruction legible: widely separated new high
bits can repeatedly renew a counterexample-like 2-adic shadow, but every
finite sequence of such renewals measured here eventually expires. Proving
termination for every finite support budget would prove descent for every
positive integer and hence the conjecture. The computed budgets do not prove
that universal statement; they identify a precise sufficient theorem.

### Exact-state invariant synthesis

`frontier_probe.rad` forks six deterministic ranking laws and evaluates them
with `simulate_many()`. A typed settlement, one resolver, a Candidate
Constraint, and `why()` select only a ranking law that reconstructs all eleven
known exact support boundaries. The beam is explicitly a witness generator,
not an exhaustion certificate.

The selected zero-runway law finds an eleven-one input with set positions

```text
0, 1, 2, 6, 39, 47, 98, 339, 516, 553, 715
```

that remains prefix-noncontracting through step 944. It contracts and first
descends at step 945, then reaches 1 after 4,375 shortcut steps. An independent
Python verifier checks every prefix with big integers. This proves a new lower
bound for the support-eleven slope boundary; it does not prove the matching
upper bound because a beam may discard a deeper state.

The experiment also dogfooded performance honestly: replacing six serial
speculative simulations with `simulate_many()` reduced the depth-1,024 run
from about 28.6 seconds to 13.9 seconds on the development host without
changing the causal result.

The first exhaustive support-eleven closure attempt exceeded a ten-minute
development run. The project now exposes deterministic, mergeable exact
affine lanes at the weight-six seed boundary, with unit coverage showing that
four lanes merge to the monolithic support-seven certificate. This turns the
next long proof run into checkpointable partitions without adding any
conjecture-specific behavior to the VM.

The RAD layer forks one COW universe per budget, simulates and audits them,
round-trips every fork through the authenticated wire format, submits both
forward and reversed event batches as typed proposals, constrains the complete
candidate, and records `why()`/`why_resource()` ancestry. At support ten, the
exact kernel visits 235,435,908 renewal anchors and accounts for 5,772,157,901
logical transitions in about 117.3 seconds on the 16-logical-core development
host. Specializing exhausted leaves as fixed naturals reduced the earlier
224.5-second cylinder-only run by almost half. At support nine, proof-anchor
compression reduced the first repeated breadth-first run from about 123.5
seconds to about 13.5 seconds. `verify_support_pressure.py` is independent of
RAD and the extension: it exhaustively agrees through support seven/depth 365
and checks every reported terminal witness through support ten.

The Candidate Constraint also checks two observed theory invariants: every new
record bit occurs before the preceding budget's death deadline, and
`H(w) < 2^(w+3)` through support ten. They are explicitly finite evidence, not
an extrapolated theorem. Proving any finite envelope for all `w` would settle
the conjecture because every natural number has finite binary support.

The companion `slope_probe.rad` removes the verified floor and all affine
remainder thresholds, retaining a prefix only while its multiplicative
coefficient stays noncontracting. Through support ten it returns exactly the
same death depths, record witnesses, bit positions, and anchor counts as the
full descent certificate. The weight-ten record reaches depth 818 with 516
odd shortcut steps, so `3^516 < 2^818`; coefficient contraction and actual
descent occur together. Thus the measured extremal frontier belongs entirely
to the irrational-slope meander branch rather than a paradoxical additive
tail. This does not eliminate paradoxical counterexamples globally, but it
lets the next proof attack focus on a simpler combinatorial transducer.

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

## Kernel boundary and dogfood findings

No Collatz rule is implemented in `core/vm`. The reusable arithmetic lives in
the project-owned `native-math-kernels` extension and is parameterized by an
odd affine map `(multiplier, addend)`. The VM changes discovered by this
dogfood are domain-neutral:

- extensionless plugin paths resolve to a platform-suffixed binary, preventing
  Windows, Linux, and macOS artifacts from overwriting one another in a shared
  workspace;
- replay reconstructs the authenticated module graph, including dependency
  initialization order and private module state, instead of flattening imports
  into one source file;
- replay verifies terminal success or failure independently of the final world
  digest, so an early replay crash cannot masquerade as success merely because
  neither execution wrote world state.

The repository gates `core/` against problem-specific names; the Collatz and
Frankl vocabulary belongs only to documentation, dogfood, and project-owned
adapters.

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
