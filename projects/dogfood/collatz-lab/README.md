# Collatz structural laboratory

This dogfood uses RAD to study what a Collatz counterexample would have to
look like. It does not present a finite search as a proof of an infinite
conjecture.

[`PROOF_PROGRAM.md`](./PROOF_PROGRAM.md) separates the proved reductions,
exact computed base, and the uniform renewal lemma that would finish the
argument.

The program attacks the two exhaustive possibilities:

1. a nontrivial positive cycle, represented by an exact word of valuations
   `v2(3n+1)` and checked with its affine cycle equation;
2. a divergent orbit, represented at every depth by a coherent residue class
   that has not acquired a certified first descent.

## Run

```powershell
cargo build --release -p rad-vm
projects/dogfood/native-math-kernels/build.ps1
target/release/rad.exe projects/dogfood/collatz-lab/main.rad `
  --experimental-laws `
  --record projects/dogfood/collatz-lab/out/run.radr

python projects/dogfood/collatz-lab/verify_certificate.py `
  projects/dogfood/collatz-lab/out/certificate.json `
  --report projects/dogfood/collatz-lab/out/verification.json

target/release/rad.exe replay projects/dogfood/collatz-lab/out/run.radr

target/release/rad.exe projects/dogfood/collatz-lab/natural_tail.rad `
  --experimental-laws `
  --record projects/dogfood/collatz-lab/out/natural-tail.radr

python projects/dogfood/collatz-lab/verify_natural_tail.py `
  projects/dogfood/collatz-lab/out/natural-tail-certificate.json `
  --report projects/dogfood/collatz-lab/out/natural-tail-verification.json

target/release/rad.exe replay projects/dogfood/collatz-lab/out/natural-tail.radr

target/release/rad.exe projects/dogfood/collatz-lab/support_pressure.rad `
  --experimental-laws `
  --record projects/dogfood/collatz-lab/out/support-pressure.radr

python projects/dogfood/collatz-lab/verify_support_pressure.py `
  projects/dogfood/collatz-lab/out/support-pressure-certificate.json `
  --report projects/dogfood/collatz-lab/out/support-pressure-independent-verification.json

target/release/rad.exe replay projects/dogfood/collatz-lab/out/support-pressure.radr

# Optional: isolate only the prefix-noncontracting meander branch.
target/release/rad.exe projects/dogfood/collatz-lab/slope_probe.rad -- 10 2048

# Counterexample-guided exact-state portfolio (witnesses, not exhaustion).
target/release/rad.exe projects/dogfood/collatz-lab/frontier_probe.rad `
  --experimental-laws `
  --record projects/dogfood/collatz-lab/out/frontier-1024.radr `
  -- 1024 14 256 > projects/dogfood/collatz-lab/out/frontier-1024.json

python projects/dogfood/collatz-lab/verify_frontier.py `
  projects/dogfood/collatz-lab/out/frontier-1024.json `
  --output projects/dogfood/collatz-lab/out/frontier-1024-verification.json
```

Or use `run.ps1` after building the release binary.

## What RAD is dogfooding

- `fork_with()` creates eight disjoint low-bit universes without touching the
  live world.
- `simulate_many()` evaluates those universes concurrently.
- A project-owned native extension loaded through generic `load_extension()`
  expands only still-dangerous residue nodes. A certified descent prunes its
  entire descendant subtree; no Collatz operation exists in the VM.
- Nine typed proposals (eight lanes plus the cycle box) feed one resolver.
- Candidate Constraints audit class counts, residue sums, histograms, cycle
  partitions, and the visible all-odd branch before commit.
- Reversing producer order creates identical evidence.
- `why()` explains the complete fan-in; an event and `why_resource()` retain
  external ancestry.
- CLI record/replay checks deterministic execution.
- `verify_certificate.py` recomputes the mathematics without importing RAD.

On the Windows development host, the debug CLI completed one full RAD study
in about 0.9 seconds. Inside that run the parallel residue kernel took roughly
1–2 ms and the exact 4.54-million-word cycle kernel roughly 0.28 seconds. The
independent Python verifiers are intentionally much slower: they re-enumerate the
complete residue cube and cycle box with Python big integers instead of
trusting RAD's pruning kernel. These are development measurements, not a
portable benchmark claim.

## Natural tails and the actual remaining gap

`natural_tail.rad` distinguishes two objects that look identical in a bounded
parity search:

```text
an arbitrary infinite 2-adic survivor path
one fixed positive integer, whose high input bits are eventually zero
```

At seven exact scales through depth 28, RAD forks and audits the eight native
lanes, submits typed findings through an ordinary event, resolves them into a
single candidate, and constrains the candidate against an independently
computed irrational-slope ballot count.  Reversing all lane proposals gives
the same component and `why()` connects the fan-in to the event instance.

At depth 28, merely requiring the final parity weight to satisfy
`3^q >= 2^28` leaves 24,821,333 words. Requiring the inequality at every
prefix leaves only 3,524,586 meanders. Continuing each corresponding residue
with zero high input bits makes all 3,524,586 coefficients contract and all
3,524,586 trajectories descend by step 395. The latest is `217,740,015`; the
peak record is `3,202,398,580,560,632`, reached from `210,964,383`.

The equality between first coefficient contraction and first actual descent
is not assumed globally. It is Terras's coefficient-stopping-time conjecture,
verified here only for the finite certified set. This exposes the precise
infinite obstruction: a counterexample must keep an infinite parity meander,
or exhibit a genuinely paradoxical coefficient contraction whose additive
remainder still prevents descent. `verify_natural_tail.py` independently
rebuilds the prefix tree and every finite tail with Python big integers.

The depth-28 run originally carried 1,024-slot outcome histograms through
every fork, proposal, resolver, and causal record. The exact record stopped at
395, so the constrained horizon is now 512: any unresolved tail still aborts
the settlement. On the Windows development host this reduced the complete
seven-scale RAD run from roughly 4.3 seconds to about 1.8 seconds, with about
0.8 seconds in the generic arithmetic kernel. The independent streaming
Python verifier remains intentionally slower because it rederives the entire
certificate using a separate implementation. Its first breadth-first version
took about 85 seconds at depth 28 and retained a large frontier. The checked-in
verifier now streams 64 independent low-bit lanes through a bounded worker
pool; on the same host it completed in about 32 seconds with stable per-worker
memory.

## Support pressure: an infinite-class exclusion

`support_pressure.rad` uses a boundary condition that the earlier residue and
natural-tail studies did not exploit: every ordinary non-negative integer has
only finitely many `1` bits in its binary expansion.  For a support budget
`w`, the generic affine kernel explores only proof anchors where a new input
bit becomes one and scans the intervening zero tails without materializing a
full breadth-first frontier.

Assuming the published convergence floor `2^71`, the exact death depths for
least-counterexample cylinders having at most `w` input one-bits are:

```text
w                     0  1  2  3   4    5    6    7    8    9   10
first empty depth     1  2  4  7  59  137  214  365  552  634  818
```

Consequently, any hypothetical least counterexample has at least eleven set bits.
This excludes an infinite class of integers, rather than checking all starts
below another finite numeric bound.  The implication is exact: if the
bounded-support tree is empty, every integer above the verified floor with
that support has an affine prefix that falls below itself; strong induction
then supplies convergence.

The record witnesses also explain how the obstruction tries to form.  The
weight-ten record has one-bits at

```text
0, 1, 2, 6, 39, 47, 98, 339, 530, 592
```

and survives through depth 817, but its zero tail is certified dead at depth
818.  Sparse high-bit injections repeatedly renew a finite shadow of a 2-adic
survivor; once the injections stop, the ordinary integer loses.  A complete
proof would follow if one could prove this termination for every finite
support budget.  The data do not prove that universal step, but they turn it
into a concrete proof program rather than a trajectory-search slogan.

RAD dogfoods the complete transactional surface around this arithmetic:

- one forked universe per support budget, concurrent `simulate_many()`, and
  `assert_only_changed()` keep speculation isolated;
- every speculative world is serialized and restored with
  `fork_to_bytes()`/`fork_from_bytes()` before its finding is trusted;
- forward and reverse event batches submit typed laws to one resolver;
- a Candidate Constraint checks the complete support partition and exact
  death boundary before atomic commit;
- `why()` exposes proposal fan-in, `why_resource()` preserves the completion
  event, and record/replay checks the world digest.

The support-ten traversal visits 235,435,908 renewal anchors and accounts for
5,772,157,901 exact logical transitions in about 117.3 seconds on the
16-logical-core Windows development host. Exhausted support leaves are treated
as fixed naturals and their division runs are batched; the earlier conservative
cylinder-only version took about 224.5 seconds for the same support-ten result.
At support nine, proof-anchor compression reduced the first repeated
breadth-first implementation from about 123.5 seconds to about 13.5 seconds.
A separate Python implementation exhaustively agrees through support
seven/depth 365 and checks the terminal boundary of every reported witness
through support ten.

The exact observations also satisfy two candidate invariants checked inside
the settlement: the next renewal bit always arrives before the preceding
budget's death depth, and `H(w) < 2^(w+3)` for every computed `0 <= w <= 10`.
These are proof targets, not extrapolated theorems. A local transition lemma
that establishes any finite envelope for every `w` would close the conjecture.

`slope_probe.rad` removes the verified floor and every additive threshold from
the pruning rule: a prefix survives exactly while its affine coefficient is
noncontracting. Through support ten it produces the same death depths, record
witnesses, bit positions, and anchor counts as the full certificate. In
particular, the weight-ten record has 516 odd shortcut steps at depth 818:
`3^516 < 2^818`, and its first coefficient contraction is also its first
descent. The current extremal frontier is therefore the pure irrational-slope
meander branch, not a paradoxical additive-remainder branch. This simplifies
the next proof obligation, although the standalone deep slope audit is slower
(about 210 seconds) because it preserves coefficient/denominator big integers
through every exhausted leaf.

The same program constructs the greedy minimal-odd boundary meander and
inverts it back to input bits. Its depth-61 2-adic shadow is
`937,101,304,038,054,907`; bit 61 is forced to become one next. Leaving that
high bit at zero makes the finite representative descend at step 62. The
counterexample-like 2-adic path survives by changing its high input bits; the
fixed positive shadow does not.

## Counterexample-guided renewal synthesis

`frontier_probe.rad` turns the exact affine state into a deterministic beam
portfolio. Six forked universes compete using four domain-neutral ranking
objectives: zero-tail runway, multiplicative headroom, small terminal probe,
and three seeded deterministic mixtures. `simulate_many()` executes the
universes concurrently. Their results enter a typed settlement; one resolver
selects the explicit best score, a Candidate Constraint requires the selected
law to reconstruct every exact known boundary, and `why()` explains the
selection fan-in.

The beam is deliberately labeled `certificate: false`: discarded states make
absence claims invalid. Positive witnesses remain exact. With 256 retained
states per support, the zero-runway law independently reconstructs all exact
death boundaries through support ten, then finds an eleven-one integer with
set-bit positions

```text
0, 1, 2, 6, 39, 47, 98, 339, 516, 553, 715
```

whose affine coefficient remains noncontracting through step 944. At step 945
the coefficient contracts and the integer itself first descends; it reaches 1
after 4,375 shortcut steps. Therefore the prefix-slope death depth at support
eleven is at least 945. This is a new exact lower bound from one witness, not
the missing exhaustive upper bound.

At depth 2,048 the same ranking law retains successive supports 11 through 18
to depths `944, 1058, 1242, 1401, 1470, 1662, 1824, 1962`. Every record first
contracts and descends on the following step. These later values are heuristic
frontier records, useful for invariant synthesis but not claims about the
globally deepest witness at each support.

`verify_frontier.py` imports neither RAD nor the native extension. It checks
all emitted witnesses with Python big integers, including support, every
prefix coefficient inequality, the step-945 boundary, and eventual convergence.
Moving the six candidates from serial `simulate()` calls to `simulate_many()`
reduced the depth-1,024 portfolio from about 28.6 seconds to 13.9 seconds on
the development host with identical semantic output.

The exhaustive support-eleven upper-bound attempt did not finish inside a
ten-minute development run. Dogfooding therefore added
`affine_sparse_slope_support_lane()`: it deterministically partitions the
exact weight-six seed frontier and returns a mergeable partial summary with
lane identity, seed coverage, anchors, records, and a content signature. The
four-lane unit proof merges exactly to the monolithic support-seven
summary. This is generic affine-search infrastructure for checkpointing and
resuming the remaining exact support-eleven proof; it is not a new prune rule.

## Current exact certificate

At residue depth 26, assuming the independently established convergence of
all positive starts below `2^71`:

```text
67,108,864 residue classes
66,071,490 certified impossible for a least counterexample
 1,037,374 survivor cylinders (1.545807719%)
 2,454,892 tree nodes expanded by the eight RAD lanes
```

Every surviving cylinder has noncontracting affine slope at the observed
horizon. Their odd-step histogram is concentrated near the critical boundary
where `3^odd_steps` competes with `2^steps`.

The most legible obstruction is `2^26-1`. It shadows the 2-adic integer `-1`
and therefore takes 26 consecutive odd shortcut steps. But the positive
integer is not actually `-1`: it peaks at `4,580,524,758,860`, then falls
below its start after 143 shortcut steps, of which 90 are odd. Its empirical
odd density over that excursion is `90/143`, just below the critical
`1/log2(3)` density required to prevent multiplicative contraction.

The finite cycle box exhausts 4,540,385 positive valuation words with at most
10 odd terms and 24 total divisions. Ten words encode repetitions of the
trivial cycle at `1`; none encode a nontrivial positive cycle. This is an
explanatory kernel check, not the strongest published cycle bound.

## The structural answer

A least counterexample cannot enter any smaller positive integer. For one
parity prefix,

```text
T^j(n) = (3^a n + b) / 2^j.
```

If `3^a < 2^j`, the prefix descends above the exact threshold
`b/(2^j-3^a)`. When that threshold is below the verified `2^71` floor, the
entire residue cylinder is impossible for a least counterexample. What
remains is not random noise: it is a thin prefix tree forced to keep roughly
63.1% odd shortcut steps, or equivalently too few powers of two in its
Syracuse valuations.

A counterexample would therefore have to be either:

- an exact cycle word satisfying a severe exponential divisibility equation;
  or
- an infinite coherent path through every one of these shrinking survivor
  trees, repeatedly shadowing 2-adic growth obstructions without ever paying
  the compensating even-step debt seen in finite positive shadows.

The all-odd compatible path itself converges 2-adically to `-1`; its positive
representative changes at every depth. It is therefore a model of the local
obstruction, not a constructed positive counterexample. A real positive
counterexample would need one fixed integer to survive every deeper prefix.
RAD checks that the limiting boundary object satisfies `T(-1) = -1`, while
also recording that `-1` is outside the positive domain of the conjecture.

The computation establishes that form exactly through depth 26. It neither
constructs such an infinite path nor proves none exists.
