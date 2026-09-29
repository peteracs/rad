# RAD-owned matrix repair search

No Collatz proof was found. This campaign extends the direct termination
search with a generic native repair kernel, RAD speculative execution, and
a mathematical constraint on any claim of a complete proof. The target is
the all-size argument in [PROOF_TARGET.md](PROOF_TARGET.md), based on the
mixed-base rewriting system of
[Yolcu, Aaronson, and Heule](https://emreyolcu.com/research/rewriting-collatz.pdf).
The numerical objective is an optimization score, not a measure of distance
to a proof.

## Search and arithmetic

Each symbol has a nonnegative affine matrix of dimension `d + 1`, with the
last row fixed to `[0, ..., 0, 1]`. A word is interpreted by multiplying its
symbol matrices, in either the original or reversed orientation. Every
active rule contributes the sum of positive coefficient deficits in
`right - left`. Selected boundary rules additionally require the first
constant coefficient to decrease by at least one. The objective is
`weak_deficits + strict_weight * strict_deficits`.

The generic `rewrite_rank_search_json` export accepts caller-supplied rules,
initial matrices, active and strict masks, grounding, seed, temperature, and
trial count. It changes one coefficient per trial and recomputes only rules
that contain that symbol. Fixed matrix storage, skipping zero products, and
starting from the first symbol avoid allocations and redundant arithmetic
in matrix multiplication. Every trial contributes to a deterministic
checksum, including rejected mutations. Replacing a coefficient by its
current value still counts as a trial; these are not counts of distinct
matrices or complete proof templates.

Input limits are part of the exactness argument: dimension at most eight,
coefficients at most 31, at most 31 rules, word length at most three, and
strict weight at most 1024. Every product entry is at most
`9^2 * 31^3 = 2,413,071`. Even the loose bound
`31 * (72 * 2,413,071 + 1024 * 2,413,072)` on the objective is below `2^63`.
The RNG and checksum products also fit signed 64-bit arithmetic. The Python
oracle uses arbitrary precision integers instead of relying on these bounds.

The kernel exposes `full_rescore` for a separate native implementation path.
The RAD reference recomputes all rule products after every trial; it shares
neither native matrix storage nor incremental dependency bookkeeping.
Differential checks compare the best matrix, all scores, acceptance count,
final RNG state, and checksum of trial scores. The only omitted comparison
field is the intentionally different number of rule evaluations.

## RAD's role

[repair.rad](repair.rad) owns successive generations. It validates the
canonical eleven rules and masks, forks every lane, executes the search with
`simulate_many`, recomputes each endpoint score in a checked pure RAD
function, verifies the write footprint, and round-trips the resulting world
through bytes. The live world must remain unchanged during speculation.

One isolated native generation serializes its calls. The adapter retains
four independently loaded generations so lanes can perform native work
concurrently. The existing 700 ms startup ceiling, caller-selected 850 ms
call deadline, and VM containment policy remain in force. A cold startup
failed the deadline after a rebuild during this session; the failure was
reported and subsequent launches passed. No automatic retry hides this in
the acceptance script.

Results enter a law/resolver transaction. The resolver chooses complete
proofs before ordinary observations, then lower scores and lexicographic
labels. Reversing proposal order produces the same result. The candidate
constraint rejects mismatched scores and calls
`certifies_complete_rank` on every selected claim of a complete proof.
That function checks the actual matrix domains and all eleven inequalities;
metadata alone cannot satisfy it. It uses constraint-safe arithmetic without
`assert` or native calls. Meter exhaustion rejects a proposal; it never
counts as a certificate. No full-system positive example exists in this run
with which to measure the maximum accepted proof-checking cost.

A zero objective first passes the existing `check_model` verifier as well.
Strictness of only some boundary rules establishes at most a relative
termination stage. A complete certificate must include all boundary rules
required by the written argument. Reduced positive controls remain labeled
and cannot win the complete-system observation selection.

## Results and performance

The first campaign used 70 lanes across dimensions 2, 3, 4, 6, and 8,
both orientations, seven strict-rule targets, and two search settings.
Sixteen rounds of 20,000 trials per lane evaluated 22.4 million mutations.
Repeating the exact campaign through the worker pool and through the final
driver reproduced all lane reports. Those repetitions are validation, not
additional exploration.

The diversified campaign used 210 lanes with zero, identity, low random,
upper-triangular, sparse, and dense starts. Coefficient bounds range from
one to 31; strictness weights and temperatures also vary. Its sixteen rounds
evaluated another 67.2 million mutations, for 89.6 million in total across
the two configurations. Neither campaign found a zero-score full-system
candidate. Twelve targeted SMT repairs of first-campaign endpoints reported
two UNSAT results and ten timeouts. A pinned reduced control was SAT; the
same pinned model with the missing rule restored was UNSAT. Those outcomes
apply only to the saved bounded formulas.

Some diversified candidates satisfy every selected strict decrease and miss
one weak coefficient by one. For four such candidates with all three forward
boundary rules strict, [context_probe.py](context_probe.py) reconstructs the
valid rewrite `cbd -> cgd`, representing the Collatz step `3 -> 5`.
[context_probe.rad](context_probe.rad) independently checks the rule, decoded
integers, and increasing matrix rank. Thus the failed inequality is reachable;
it cannot be dismissed as an artifact of requiring an overly large domain.
This rejects those candidates, not every context-sensitive interpretation.

The benchmark compares identical 20,000-trial requests against the native and
RAD implementations in alternating order over three pairs. Process wall time
includes startup, extension loading, JSON, and independent RAD endpoint
checking. Each measured pair must return the same exact report. The receipt
contains the observed medians and ratio; this is a speedup for this workload,
not a claim that the entire language is 100x faster. An earlier three-pair run
measured approximately 165x. The four-worker campaign reduced 51.2 seconds
with one shared generation to 24.6 seconds with separate generations.

RAD's `unique` ownership also reduced measured list-copy events in a separate
100-trial reference profile from 82,470 to 61,968, with identical output.
Only outer matrix buffers can be marked unique here: passing a unique inner
row to an ordinary list insertion violates the ownership contract.

## Reproduce and inspect

Run from the repository root with the release RAD CLI and FFI worker built:

```powershell
cargo build --release -p rad-cli -p rad-ffi-worker
powershell -NoProfile -ExecutionPolicy Bypass -File projects/dogfood/native-math-kernels/build.ps1
py projects/dogfood/collatz-termination/repair_campaign.py
py projects/dogfood/collatz-termination/repair_diverse.py
py projects/dogfood/collatz-termination/repair_oracle.py --seconds 30
py -O projects/dogfood/collatz-termination/accept_repair.py
py -O projects/dogfood/collatz-termination/accept.py
```

The kernel is project-owned and uses RAD's existing extension ABI. It adds
no Collatz-specific VM operation. Acceptance covers changed worker counts,
native-call record/replay, independent arithmetic, full-walk differential
checks, invalid request rejection, forged proof rejection, proposal ordering,
and concrete context counterexamples. Native library tests passed 56 cases
with one preexisting production-scale test ignored. Strict Clippy is still
blocked by six existing diagnostics in `affine_frontier`, `natural_tails`,
and `cyclic_pairs`; none is in the new repair kernel.

[repair-evidence.json](repair-evidence.json) binds sources, binaries,
benchmarks, and artifacts. Raw inputs, output, traces, and solver queries are
under ignored `out/`. Historical outputs have their own hashes; final source
hashes do not claim that earlier runs used identical source bytes. Replaying
and rechecking an execution is evidence about that execution. The all-size
termination implication still rests on the written mathematics, and no
complete certificate has discharged its hypotheses.
