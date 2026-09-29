"""Check the universal algebraic obligations used by the handwritten induction.

Every obligation is quantifier-free integer arithmetic: a free variable is a
potential counterexample, and UNSAT rules out all integer counterexamples.
The interval rules are linear; the two new counting cases use products.
This checks the lemmas, not the surrounding mathematical induction in a proof
assistant. The emitted Z3 proof objects are not independently proof-checked.
"""

import argparse
import hashlib
import json
from pathlib import Path

import z3


def obligations():
    n, m, q, y, r, lo, hi, target = z3.Ints("n m q y r lo hi target")

    def a(x):
        return z3.If(x == n - 1, 0, x + 1)

    def b(x):
        return z3.If(x == n - 1, 0, x)

    def back(x, distance):
        return z3.If(x >= distance, x - distance, x + n - distance)

    def prefix(x, end):
        return z3.And(x >= 0, x <= end)

    def zero(x, end):
        return z3.Or(prefix(x, end), x == n - 1)

    def one(x, end):
        return z3.Or(zero(x, end), x == n - 2)

    def block(x):
        return back(b(a(b(x))), 2)

    domain = [n >= 3, m >= 0, m <= n - 3, q >= 0, q < n]
    yield "prefix_zero_inclusion", [n >= 3, q >= 0, q < n], zero(back(b(q), 1), n - 3)
    witness = z3.If(y == n - 1, 0, y + 1)
    yield "prefix_zero_surjection", [n >= 3, zero(y, n - 3)], z3.And(
        witness >= 0, witness < n, back(b(witness), 1) == y)
    yield "block_zero_inclusion", domain + [zero(q, m)], zero(block(q), m - 1)
    yield "block_one_inclusion", domain + [one(q, m)], one(block(q), m - 1)
    witness = z3.If(y == n - 1, n - 1, y + 1)
    yield "block_zero_surjection", domain[:3] + [zero(y, m - 1)], z3.And(
        zero(witness, m), block(witness) == y)
    witness = z3.If(y >= n - 2, y, y + 1)
    yield "block_one_surjection", domain[:3] + [one(y, m - 1)], z3.And(
        one(witness, m), block(witness) == y)
    for name, transformation in (
        ("delete_first_b", back(b(a(q)), 2)),
        ("delete_middle_a", back(b(q), 2)),
        ("delete_second_b", back(b(q), 1)),
        ("delete_tail_a", back(b(a(b(q))), 3)),
    ):
        yield name, domain + [zero(q, m)], one(transformation, m - 1)
    yield "last_a_intersection", [n >= 3, q >= 0, q < n, target >= 0, target < n], (
        z3.And(b(q) == target, b(a(q)) == target)
        == z3.And(target == 0, q == n - 1))

    def br(x):
        return z3.If(x >= n - r, 0, x)

    arc_domain = [n >= 3, r >= 1, r <= n - 2, q >= 0, q < n]
    yield "general_arc_intersection", arc_domain + [target >= 0, target < n], (
        z3.And(br(q) == target, br(a(q)) == target)
        == z3.And(target == 0, q >= n - r))
    yield "reset_image_avoids_arc", arc_domain, z3.Or(br(q) < n - r, br(q) >= n)
    interval = [n >= 3, 1 <= lo, lo <= hi, hi < n, q >= 0, q < n]
    yield "interval_erosion", interval, (
        z3.And(lo <= q, q <= hi, lo <= a(q), a(q) <= hi)
        == z3.And(lo <= q, q <= hi - 1))

    # Backward-interval lemmas for the exact rotation lower bound.
    left, length = z3.Ints("left length")
    allowed = z3.Bool("allowed")

    def circular(x, start, size):
        distance = z3.If(x >= start, x - start, x + n - start)
        return z3.And(distance >= 0, distance < size)

    previous_left = z3.If(left == 0, n - 1, left - 1)
    interval_domain = [n >= 3, 0 <= left, left < n,
                       1 <= length, length < n, 0 <= q, q < n]
    inner = circular(q, left, length)
    # X contains Y. Only the new possible endpoint's membership in X matters.
    a_intersection = z3.And(circular(a(q), left, length),
                            z3.Or(inner, z3.And(q == previous_left, allowed)))
    yield "backward_rotation_interval", interval_domain, a_intersection == z3.If(
        allowed, circular(q, previous_left, length), circular(q, left, length - 1))
    has_zero = circular(0, left, length)
    has_top = circular(n - 1, left, length)
    grows = z3.And(has_zero, z3.Not(has_top), allowed)
    shrinks = z3.And(z3.Not(has_zero), has_top)
    b_intersection = z3.And(circular(b(q), left, length),
                            z3.Or(inner, z3.And(q == n - 1, allowed)))
    yield "backward_merge_interval", interval_domain, b_intersection == z3.If(
        grows, circular(q, n - 1, length + 1),
        z3.If(shrinks, circular(q, left, length - 1), inner))
    yield "backward_growth_endpoint", interval_domain + [has_zero, z3.Not(has_top)], left == 0
    yield "backward_shrink_endpoint", interval_domain + [z3.Not(has_zero), has_top], (
        (left + length - 1) == n - 1)
    initial_pair = z3.Or(q == n - 1, q == 0)
    initial_preimage = z3.Or(a(q) == n - 1, a(q) == 0)
    yield "immediate_growth_forces_stationary_rotation", [n >= 3, 0 <= q, q < n], (
        z3.And(initial_pair, initial_preimage) == (q == n - 1))
    rotations, growths, stationary, before_growth = z3.Ints("rotations growths stationary before_growth")
    counting = [n >= 3, stationary >= 0,
                rotations >= before_growth + (growths - 1) * (n - 1) + stationary]
    yield "rotation_bound_delayed_growth", counting + [before_growth >= n, growths >= n - 1], (
        rotations >= (n - 1) * (n - 1) + 1)
    yield "rotation_bound_immediate_growth", counting + [before_growth == 0, growths >= n, stationary >= 1], (
        rotations >= (n - 1) * (n - 1) + 1)
    yield "rotation_mutation_control", counting + [before_growth >= n, growths >= n - 1], (
        rotations >= (n - 1) * (n - 1) + 2)
    # Sanity: a deliberately overstrong shrink claim must produce a witness.
    yield "mutation_control", domain + [zero(q, m)], zero(block(q), m - 2)


def verify(output):
    z3.set_param(proof=True)
    output.mkdir(parents=True, exist_ok=True)
    receipts = []
    for name, assumptions, conclusion in obligations():
        solver = z3.Solver()
        solver.set(timeout=5000)
        solver.add(*assumptions, z3.Not(conclusion))
        query = solver.to_smt2()
        (output / f"{name}.smt2").write_text(query, encoding="utf-8", newline="\n")
        result = solver.check()
        expected = z3.sat if name.endswith("mutation_control") else z3.unsat
        if result != expected:
            detail = solver.model() if result == z3.sat else solver.reason_unknown()
            raise RuntimeError(f"{name}: expected {expected}, got {result}: {detail}")
        receipt = {"name": name, "result": str(result),
                   "query_sha256": hashlib.sha256(query.encode()).hexdigest()}
        if result == z3.unsat:
            proof = solver.proof().sexpr()
            (output / f"{name}.proof").write_text(proof, encoding="utf-8", newline="\n")
            receipt["proof_sha256"] = hashlib.sha256(proof.encode()).hexdigest()
        else:
            receipt["counterexample"] = str(solver.model())
        receipts.append(receipt)
    report = {"z3_version": z3.get_version_string(), "obligations": receipts,
              "scope": "symbolic algebraic lemmas; full arguments are in RESULTS.md and OPTIMALITY.md"}
    check_artifacts(report, output)
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


def check_artifacts(report, output):
    """Check bytes on disk, including platform newline handling."""
    for receipt in report["obligations"]:
        for suffix, key in (("smt2", "query_sha256"), ("proof", "proof_sha256")):
            if key in receipt:
                artifact = output / f"{receipt['name']}.{suffix}"
                if hashlib.sha256(artifact.read_bytes()).hexdigest() != receipt[key]:
                    raise ValueError(f"symbolic artifact hash mismatch: {artifact.name}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=Path(__file__).parent / "out/symbolic")
    args = parser.parse_args()
    report = verify(args.output)
    print(json.dumps(report, indent=2))
