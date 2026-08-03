# Collatz structural laboratory

This dogfood uses RAD to study what a Collatz counterexample would have to
look like. It does not present a finite search as a proof of an infinite
conjecture.

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
independent Python verifier is intentionally much slower: it re-enumerates the
complete residue cube and cycle box with Python big integers instead of
trusting RAD's pruning kernel. These are development measurements, not a
portable benchmark claim.

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
