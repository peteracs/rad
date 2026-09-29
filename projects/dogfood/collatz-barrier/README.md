# Collatz first-contraction barriers

A RAD research workload that replaces parity enumeration with an exact
extremal formula and a finite rotation certificate. Read [RESULTS.md](RESULTS.md)
for the proofs, assumptions, and limits; [evidence.json](evidence.json) records
the executed campaign and source hashes.

A follow-up [coefficient-spread theorem](escape/PROOF.md) proves that every
positive integer orbit has unbounded multiplicative-coefficient spread,
with a quantitative exponent greater than `1/28`. Its
[RAD workload](escape/README.md) adds exact division, certified rational
logarithms, and causal selection among 21 proof candidates. The result
restricts possible orbit behavior; it does not prove eventual contraction.

A further [uniform correction bound](correction/PROOF.md) proves
`T^j(n) < 2048 * n * 3^(odd steps) / 2^j` on every segment with distinct
states, without a starting-value or length cutoff. Its
[RAD campaign](correction/README.md) certifies an infinite tail and exports
affine coalescence reductions for induction. These results control additive
error and eliminate additional cylinders; they do not yet force a reduction
for every positive integer.

The [envelope ladder](ladder/PROOF.md) extends the quarter-slope bound below
to the complete family of sharp linear bounds for every slope
`s >= 756421/3145728 ≈ 0.24045976`. That is within 0.0044% of the limit slope
`1/(6 ln 2)`. The optimal vertices are separated by the continued-fraction
denominators 2, 12, 41 and 665 of `log_2 3`. A Denjoy–Koksma argument shows
the certificate method reaches every slope above the limit. Its
[RAD workload](ladder/README.md) splits a 15601-block certificate across 16
isolated worlds and assembles it through a causal resolver.

The strongest new-to-this-repo result is

```text
At the first coefficient contraction, C / 3^q <= q/4 + 11/108.
```

The intercept is sharp, and 78 rational phase checks support a written
all-size proof. A separate exact recurrence computes the maximum offset
for every odd-step count. Neither statement proves Collatz; historical
priority is unresolved.

From the repository root, run the complete campaign:

```powershell
py -O projects/dogfood/collatz-barrier/accept.py
py -O -m unittest discover -s projects/dogfood/collatz-barrier -p test_verify.py
```

Or run the language program directly:

```powershell
.\target\release\rad.exe projects/dogfood/collatz-barrier/main.rad --strict-types --deny-warnings -- 4096
```

Use depth 64 for a quick run. Smaller horizons explicitly count trajectories
whose coefficient has not yet contracted as `censored`; they are not counted
as successful descents. They cannot violate the claimed implication about
first contractions *within* that horizon.

The default program proves a finite-horizon CST statement for **all** n >= 2:
if the coefficient first contracts by step 4096, the trajectory descends
at that step. It covers n >= 620859 by a proved envelope and checks the
620857 smaller starts in 16 isolated RAD worlds. Separate rational certificates
yield larger implications, including a 114208327603-step restriction on
least Collatz counterexamples when using Barina's published convergence floor.

The workload exercises:

- checked pure functions and a reusable arbitrary-natural arithmetic module;
- exact rational phase analysis without floating-point logarithms;
- forked resources, `simulate_many`, and component write-footprint assertions;
- snapshot serialization and restoration of mathematical evidence;
- isolation of the live world from every proof and trajectory job;
- recorded execution and replay under changed worker counts.

No Collatz-specific builtin, native extension, or FFI worker is needed.
The existing VM is unchanged by this workload. The natural-number library
uses base-2^15 limbs because RAD's built-in `int` is signed 64-bit, even when
its internal representation calls an out-of-inline-range value `BigInt`.
This is an ordinary language library, not a change to the built-in type.

`verify.py` uses Python arbitrary integers and `Fraction`, independently
checks every literal lane, checks 78 phase candidates, and runs a distinct
max-plus parity dynamic program through depth 256. Explicit exceptions keep
acceptance checks active under `python -O`. Mutation tests reject forged
Farey data, an off-by-one horizon, insufficient starting floors, altered
rotation maxima and incorrect exact-division boundaries.

Raw JSONL, stderr, and replay traces live in ignored `out/`. The compact
receipt hashes their actual bytes, source files, proof text, and RAD binary.
Replay verifies execution determinism; the written proofs justify the
universal mathematical statements.
