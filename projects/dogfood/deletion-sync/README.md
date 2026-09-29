# Deletion-resilient synchronization laboratory

This experiment asks how to reset an automaton when commands may disappear.
It now gives an exact all-size length/cost classification for the Cerny family
and a sharp local counting theorem for arbitrary transformations. Read
[OPTIMALITY.md](OPTIMALITY.md) for the new n^2 lower bound,
[GENERAL_BOUND.md](GENERAL_BOUND.md) for the general theorem, and
[RESULTS.md](RESULTS.md) for the construction, obstruction, and related work.

The automaton has an n-state cycle a and one merging instruction b. Every
initial state, under every allowed deletion pattern, must reach the **same**
target. The number of deletions is a global budget, not a budget per step.

## Findings

For all n >= 3, one deletion costs exactly n^2 commands, while two
deletions are impossible regardless of word length. The construction is

    b a^(n-1) (b a b a^(n-2))^(n-2) a b.

The backward-interval proof establishes at least (n-1)^2+1 rotations and
2n-2 merging commands separately. This construction attains both, so it is
optimal for any nonnegative prices assigned to the two commands. The
exceptional two-state automaton tolerates any k deletions using b^(k+1).

For an arbitrary n-state transformation f, the exact maximum one-step decrease
of the k-deletion uncertainty potential is n-rank(f^(k+1)). Summing this
quantity over a resilient word gives at least (k+1)(n-1). This removes the
original permutation/idempotent restriction and includes a sharpness proof.

The saved [execution receipt](evidence.json) covers 27 forward size/budget
pairs, three verified replays, and 21 symbolic proof obligations with two
negative controls. The [weighted-search receipt](optimality-evidence.json)
covers 84 exact objectives for n=3..30. The
[general-capacity receipt](capacity-evidence.json) covers all transformations
on 2..4 states, budgets 0..3, and all nested chains. These support the written
proofs; none of the finite searches establishes historical novelty.

Dogfooding exposed and fixed a compiler performance bug: the eight-state,
two-loss search improved from a median 8.91 s to 0.155 s across three runs per
binary, with identical output. See [BUGS.md](BUGS.md) and the saved
[benchmark receipt](compiler-fix-benchmark.json).

## Run

From the repository root:

```powershell
cargo build -p rad-cli --release -j 2
python -m pip install -r projects/dogfood/deletion-sync/requirements.txt
python projects/dogfood/deletion-sync/accept.py --max-n 10 --weighted-max-n 30
python projects/dogfood/deletion-sync/test_verify.py
python projects/dogfood/deletion-sync/test_optimality.py
python projects/dogfood/deletion-sync/test_capacity.py
python projects/dogfood/deletion-sync/test_symbolic.py
```

For a smaller initial run:

```powershell
target/release/rad.exe projects/dogfood/deletion-sync/main.rad --strict-types --deny-warnings -- 6
target/release/rad.exe projects/dogfood/deletion-sync/proof_audit.rad --experimental-laws
```

The acceptance runner executes the exact search with one and four Rayon
workers, independently checks every result, replays each recording under the
other worker count, and records/replays the causal audit. It stores actual
commands, elapsed times, source hashes, binary hash, and verified instances in
`out/report.json` plus separate weighted-search and general-capacity receipts.
It also hashes the three mathematical notes. This is a research workload, not a claim that every matrix
or process completes in under one second.

The checked-in JSON receipts are snapshots of the completed local run.
Fresh recordings, solver queries, proof objects, and logs stay under ignored
`out/`; rerun acceptance to regenerate them for your build.

Python verifiers reject failures through explicit exceptions. Running them
under `python -O` checks that validation survives disabled Python assertions;
it is not a stronger mathematical proof. The negative tests verify rejection
of altered evidence, and the RAD assertions remain active.

## What RAD does

* `search.rad` implements the entire exact BFS as RAD bytecode: no FFI solver
  or precomputed answer table. It uses checked byte buffers for a queue and a
  dense encoding of nested uncertainty sets for visited states.
* `main.rad` creates one copy-on-write world per size and deletion budget,
  runs them with `simulate_many`, checks each branch with
  `assert_only_changed`, and verifies the live evidence and an unrelated
  anchor remain unchanged.
* `proof_audit.rad` combines three finite checks: the exact one-deletion
  optimum, explicit simulation of every deletion position and initial state,
  and exhaustive two-deletion rejection. Typed causal proposals and a
  candidate constraint require complete, consistent evidence. Reversing the
  proposal order must preserve the verdict; `why()` exposes its ancestry.
* `oracle.py` recomputes the finite graph using Python frozensets and a deque,
  independently of RAD's bit packing, dense rank, and byte buffers.
  `verify.py` additionally simulates individual corrupted trajectories.
* `test_verify.py` rejects mutated and incomplete evidence and compares the
  three-state optimum with literal enumeration of all shorter binary words.
* `backward.rad` and `optimality.rad` search the polynomial backward interval
  graph under three objectives, in isolated parallel worlds. A set-based
  Dijkstra oracle independently checks all distances and reachable-state counts.
* `rank_capacity.rad` exhausts the general theorem's local cases in RAD;
  `capacity_oracle.py` recomputes them with literal sets. Additional tests cover
  nonnested layers, permutation-valued faults, and whole-word sharpness.
* `symbolic_verify.py` checks 21 algebraic proof obligations for arbitrary
  integer n with Z3, saves SMT-LIB/proof artifacts, and rejects two deliberately
  false stronger claims. It does not formalize the general linear-algebra proof.
* `probe.py` is an exploratory derivation aid for interval invariants and
  weighted command counts; it is not an acceptance certificate.

The causal audit confirms agreement of finite evidence. It does not turn a
bounded experiment into a formal proof of a universally quantified theorem.
The general mathematical proofs are written in the three linked notes.
