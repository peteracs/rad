# Direct Collatz termination search

This campaign searches for a uniform termination proof, rather than a bound
on finite Collatz trajectories. **No complete proof has been found.** The
current [receipt](evidence.json) records the bounded full-system searches
and their solver-reported rejections or timeouts. Positive models belong to explicitly weakened
control systems and are never reported as Collatz proofs.

The saved SMT campaigns contain 93 full-system queries, with 20 UNSAT reports
and 73 timeouts. Seventeen concrete control models pass both independent
checkers; none is a complete-system proof. Search timeouts leave those
templates unresolved.

The [RAD repair campaign](REPAIR.md) also runs the candidate search itself
through isolated RAD worlds, an incremental native kernel, and checked law
settlement. It explored 89.6 million mutation trials across 280 lanes without
finding a complete certificate. A same-input comparison measures over 100x
acceleration against a checked RAD reference walk; the exact process timings,
source hashes, and validation results are in [repair-evidence.json](repair-evidence.json).

The target is the eleven-rule mixed binary/ternary system of Yolcu, Aaronson,
and Heule, [An Automated Approach to the Collatz Conjecture
(2023)](https://emreyolcu.com/research/rewriting-collatz.pdf). Its termination
is equivalent to Collatz convergence. The exact implication used by this
campaign, including all domain and strictness conditions, is written in
[PROOF_TARGET.md](PROOF_TARGET.md).

The search tests nonnegative integer matrices, nonnegative rational matrices,
max-plus matrices with negative weights and minus infinity, and nonlinear
scalar polynomials, including signed integer-valued polynomials with exact
finite-difference tail certificates. Both orientations of the rewriting system are searched.
Every full-system query retains every rule. Only separately labeled positive
controls omit a rule.

From the repository root:

```powershell
# Integer and max-plus matrices, through dimension eight.
py projects/dogfood/collatz-termination/search.py --seconds 90 --dimensions 3 5 8 --maximum 2

# Rational matrices whose entries have denominator two.
py projects/dogfood/collatz-termination/search.py --seconds 90 --dimensions 1 2 3 --maximum 3 --denominator 2 --semirings natural

# Quadratic, cubic, and quartic scalar interpretations.
py projects/dogfood/collatz-termination/polynomial.py --seconds 30 --engine integer

# Integer-valued functions, with signed coefficients and certified monotonicity.
py projects/dogfood/collatz-termination/newton.py --seconds 90 --degrees 3 --maximum 7 --tails 2 8 --signed

# Targeted quadratic binary / quartic ternary interpretations.
py projects/dogfood/collatz-termination/newton.py --seconds 120 --degrees 4 --maximum 7 --tails 0 2 8 --signed --profile quadratic-digits --engine bitvector --workers 3

# Matrix search with the terminal boundary grounded to a constant.
py projects/dogfood/collatz-termination/search.py --seconds 90 --dimensions 3 4 --maximum 3 --ground

# RAD owns the speculative search and accepts only independently checked results.
powershell -NoProfile -ExecutionPolicy Bypass -File projects/dogfood/native-math-kernels/build.ps1
py projects/dogfood/collatz-termination/repair_campaign.py
py projects/dogfood/collatz-termination/repair_diverse.py
py -O projects/dogfood/collatz-termination/accept_repair.py

# Verify every saved SAT model independently and through RAD.
py -O projects/dogfood/collatz-termination/accept.py
```

The Python search backends require `z3-solver`. They generate finite symbolic
constraints; they do not run sampled Collatz trajectories. The matrix search
uses at most four operating-system workers by default. The polynomial search
also has a `--engine bitvector` backend. Wide polynomial bit-vector circuits
used substantial memory in this campaign, so the integer backend is the
default. Solver limits are requested limits; preprocessing can make elapsed
wall time greater.

RAD recomputes certificate arithmetic in checked pure functions, places each
candidate in an isolated fork, checks write footprints, serializes and restores
every result, and replays the verification under changed worker counts.
`matrix.rad`, `polynomial.rad`, and `newton.rad` add the arithmetic needed for these proof
certificates without adding Collatz-specific operations to the VM.

The [Newton proof target](NEWTON.md) gives the all-integer tail identity,
the necessary degree inequalities, and the argument that grounding the
terminal boundary loses no certificate. This is the mathematical basis of
the expanded search, including the separate closure and monotonicity checks
needed when coefficients may be negative.

Acceptance independently checks natural, rational, max-plus, and nonlinear
positive controls. Twenty-nine Python tests target proof hypotheses. Eleven RAD
mutation runs reject a reinserted missing rule, a falsely complete reduced
proof, a bad rational normalization, escape from the max-plus domain, an
invalid polynomial proof, invalid Newton domains, and a timeout falsely
presented as success. A RAD arithmetic probe additionally checks a 286-bit
composition and rejects positive finite samples with an invalid infinite tail.

[evidence.json](evidence.json) hashes the current source, saved queries,
concrete models, verification output, and execution traces. Raw artifacts
live in ignored `out/`. The encoder evolved during the campaign, so the saved
SMT query bytes define each historical search; the receipt does not claim
that every old query was emitted by the final source version. Acceptance
does not rerun the searches or independently prove their UNSAT reports.

A model from a full-system query would establish relative termination of
at least one boundary rule. The remaining rules would then need to be
discharged by further checked rule-removal stages, or by a separately checked
proof. A model that makes every selected boundary rule strict already closes
the written termination argument. None of the current models does that for
the full system.

The matrix-interpretation framework and the rewriting system are prior work.
The repository contribution here is the concrete search and independent
RAD certificate path, not a claimed new theorem about Collatz convergence.
