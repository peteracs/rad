"""Independent lazy-SAT oracle for small union-closed counterexamples.

Unlike RAD's constructive deletion search, this fixes the family cardinality
and asks Z3 for arbitrary set families. Union-closure clauses are separated
from concrete models only when violated.
"""

from __future__ import annotations

import argparse
from time import perf_counter

from z3 import Bool, Not, Or, PbEq, PbGe, PbLe, Solver, is_true, sat


def closure_violations(family: set[int], limit: int) -> list[tuple[int, int, int]]:
    members = sorted(family)
    violations: list[tuple[int, int, int]] = []
    for index, left in enumerate(members):
        for right in members[index:]:
            union = left | right
            if union not in family:
                violations.append((left, right, union))
                if len(violations) >= limit:
                    return violations
    return violations


def solve(
    width: int,
    family_size: int,
    batch: int,
    rounds: int,
    timeout_ms: int,
    empty: str,
) -> int:
    cube_size = 1 << width
    full = cube_size - 1
    member = [Bool(f"member_{mask}") for mask in range(cube_size)]
    solver = Solver()
    solver.set(timeout=timeout_ms)
    solver.add(PbEq([(value, 1) for value in member], family_size))
    solver.add(member[full])
    if empty == "included":
        solver.add(member[0])
    elif empty == "excluded":
        solver.add(Not(member[0]))

    minority_bound = (family_size - 1) // 2
    frequency_terms: list[list[tuple]] = []
    for bit in range(width):
        terms = [(member[mask], 1) for mask in range(cube_size) if mask & (1 << bit)]
        frequency_terms.append(terms)
        solver.add(PbGe(terms, 1), PbLe(terms, minority_bound))

    # Any minimal labelled witness can be relabelled by nondecreasing
    # frequencies. Each pair of coordinates must remain distinguishable.
    for bit in range(width - 1):
        left_only = [
            (member[mask], 1)
            for mask in range(cube_size)
            if mask & (1 << bit) and not mask & (1 << (bit + 1))
        ]
        right_only = [
            (member[mask], 1)
            for mask in range(cube_size)
            if mask & (1 << (bit + 1)) and not mask & (1 << bit)
        ]
        solver.add(PbLe(left_only + [(term, -weight) for term, weight in right_only], 0))
    for left in range(width):
        for right in range(left + 1, width):
            solver.add(
                Or(
                    *[
                        member[mask]
                        for mask in range(cube_size)
                        if bool(mask & (1 << left)) != bool(mask & (1 << right))
                    ]
                )
            )

    # With coordinates ordered by nondecreasing frequency, separation implies
    # a staircase witness omitting coordinate i while containing every later
    # coordinate. These clauses are redundant but expose the theorem to the
    # solver instead of asking propagation to rediscover it.
    for bit in range(width - 1):
        later_mask = sum(1 << later for later in range(bit + 1, width))
        solver.add(
            Or(
                *[
                    member[mask]
                    for mask in range(cube_size)
                    if not mask & (1 << bit) and mask & later_mask == later_mask
                ]
            )
        )

    learned: set[tuple[int, int, int]] = set()
    started = perf_counter()
    for round_index in range(rounds):
        checked = perf_counter()
        result = solver.check()
        check_seconds = perf_counter() - checked
        if result != sat:
            print(
                f"round={round_index} result={result} clauses={len(learned)} "
                f"check_s={check_seconds:.3f} total_s={perf_counter() - started:.3f}"
            )
            return 0 if str(result) == "unsat" else 2

        model = solver.model()
        family = {mask for mask, value in enumerate(member) if is_true(model.eval(value))}
        frequencies = [sum(mask >> bit & 1 for mask in family) for bit in range(width)]
        violations = closure_violations(family, batch)
        print(
            f"round={round_index} size={len(family)} max_frequency={max(frequencies)} "
            f"violations={len(violations)} clauses={len(learned)} "
            f"check_s={check_seconds:.3f} total_s={perf_counter() - started:.3f}"
        )
        if not violations:
            print("COUNTEREXAMPLE_FAMILY", sorted(family))
            return 1
        inserted = 0
        for left, right, union in violations:
            key = (left, right, union)
            if key in learned:
                continue
            learned.add(key)
            solver.add(Or(Not(member[left]), Not(member[right]), member[union]))
            inserted += 1
        if inserted == 0:
            raise RuntimeError("separator returned no new closure clause")

    print(f"round limit reached after {rounds} rounds and {len(learned)} clauses")
    return 2


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--family-size", type=int, default=51)
    parser.add_argument("--batch", type=int, default=20_000)
    parser.add_argument("--rounds", type=int, default=100)
    parser.add_argument("--timeout-ms", type=int, default=30_000)
    parser.add_argument(
        "--empty",
        choices=("free", "included", "excluded"),
        default="free",
    )
    args = parser.parse_args()
    return solve(
        args.width,
        args.family_size,
        args.batch,
        args.rounds,
        args.timeout_ms,
        args.empty,
    )


if __name__ == "__main__":
    raise SystemExit(main())
