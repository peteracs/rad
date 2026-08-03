"""Lazy exact search for a Frankl counterexample on a labelled Boolean cube.

This is an independent oracle for the RAD dogfood.  It searches the dual
deleted family D.  Strict majority in D is the negation of Frankl's bound in
F = P([n]) \\ D, while each learned clause

    U in D  ->  A in D or B in D       (A union B = U)

is exactly one union-closure obstruction for F.  Clauses are separated lazily
from concrete models so the solver never materializes all O(4^n) decompositions
up front.
"""

from __future__ import annotations

import argparse
from itertools import product
from time import perf_counter

from z3 import Bool, Not, Or, PbGe, Solver, is_true, sat


def decompositions(union: int, width: int):
    bits = [1 << bit for bit in range(width) if union & (1 << bit)]
    for choices in product((0, 1, 2), repeat=len(bits)):
        left = 0
        right = 0
        for bit, choice in zip(bits, choices):
            if choice != 1:
                left |= bit
            if choice != 0:
                right |= bit
        yield left, right


def violated_obstructions(deleted: set[int], width: int, limit: int):
    found: list[tuple[int, int, int]] = []
    for union in sorted(deleted, key=lambda mask: (mask.bit_count(), mask)):
        for left, right in decompositions(union, width):
            if left not in deleted and right not in deleted:
                found.append((union, left, right))
                if len(found) == limit:
                    return found
    return found


def solve(width: int, batch: int, rounds: int, timeout_ms: int) -> int:
    cube_size = 1 << width
    full = cube_size - 1
    deleted = [Bool(f"deleted_{mask}") for mask in range(cube_size)]
    solver = Solver()
    solver.set(timeout=timeout_ms)

    solver.add(deleted[0] == False, deleted[full] == False)
    for bit in range(width):
        # 2*d_i - |D| >= 1.  Native signed PB constraints avoid routing the
        # Boolean search through integer arithmetic.
        solver.add(
            PbGe(
                [(deleted[mask], 1 if mask & (1 << bit) else -1) for mask in range(cube_size)],
                1,
            )
        )

    # Relabeling permits the frequency vector to be sorted.  This symmetry
    # break removes equivalent labelled models without excluding a witness.
    for bit in range(width - 1):
        terms = []
        for mask in range(cube_size):
            left = bool(mask & (1 << bit))
            right = bool(mask & (1 << (bit + 1)))
            if left != right:
                terms.append((deleted[mask], 1 if left else -1))
        solver.add(PbGe(terms, 0))

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
        family = {mask for mask, value in enumerate(deleted) if is_true(model.eval(value))}
        counts = [sum(bool(mask & (1 << bit)) for mask in family) for bit in range(width)]
        surplus = [2 * count - len(family) for count in counts]
        violations = violated_obstructions(family, width, batch)
        print(
            f"round={round_index} deleted={len(family)} min_surplus={min(surplus)} "
            f"violations={len(violations)} clauses={len(learned)} "
            f"check_s={check_seconds:.3f} total_s={perf_counter() - started:.3f}"
        )
        if not violations:
            print("COUNTEREXAMPLE_DELETED", sorted(family))
            return 1


        inserted = 0
        for union, left, right in violations:
            key = (union, min(left, right), max(left, right))
            if key in learned:
                continue
            learned.add(key)
            solver.add(Or(Not(deleted[union]), deleted[left], deleted[right]))
            inserted += 1
        if inserted == 0:
            raise RuntimeError("separator returned no new obstruction clause")

    print(f"round limit reached after {rounds} rounds and {len(learned)} clauses")
    return 2


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--batch", type=int, default=50_000)
    parser.add_argument("--rounds", type=int, default=100)
    parser.add_argument("--timeout-ms", type=int, default=60_000)
    args = parser.parse_args()
    return solve(args.width, args.batch, args.rounds, args.timeout_ms)


if __name__ == "__main__":
    raise SystemExit(main())
