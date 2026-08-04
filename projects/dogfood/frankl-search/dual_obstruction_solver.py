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
import json
from itertools import product
from pathlib import Path
from time import perf_counter

from z3 import Bool, Not, Or, PbEq, PbGe, PbLe, Solver, SolverFor, is_true, sat


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


def subcube_density_terms(deleted: list, union: int) -> list[tuple[object, int]]:
    rank = union.bit_count()
    terms: list[tuple[object, int]] = []
    subset = union
    while True:
        coefficient = 2
        if subset == union:
            coefficient -= 1 << rank
        if coefficient:
            terms.append((deleted[subset], coefficient))
        if subset == 0:
            break
        subset = (subset - 1) & union
    return terms


def add_subcube_density_cuts(solver: Solver, deleted: list, width: int) -> int:
    """Aggregate an exponential closure obstruction into one cut per union.

    If U is deleted, the surviving members contained in U cannot contain two
    sets whose union is U. Complementing inside U makes them pairwise
    intersecting, hence at most half of P(U) survive. Equivalently, at least
    half of P(U) is deleted. The inequality is necessary for every union-closed
    complement and is independent of the conjecture's frequency objective.
    """

    added = 0
    for union in range(1, 1 << width):
        solver.add(PbGe(subcube_density_terms(deleted, union), 0))
        added += 1
    return added


def violated_subcube_density(deleted: set[int], width: int, limit: int) -> list[int]:
    violations: list[int] = []
    for union in sorted(deleted, key=lambda mask: (mask.bit_count(), mask)):
        if union == 0:
            continue
        deleted_subsets = 0
        subset = union
        while True:
            deleted_subsets += subset in deleted
            if subset == 0:
                break
            subset = (subset - 1) & union
        if 2 * deleted_subsets < 1 << union.bit_count():
            violations.append(union)
            if len(violations) == limit:
                break
    return violations


def solve(
    width: int,
    batch: int,
    rounds: int,
    timeout_ms: int,
    aggregate_mode: str,
    engine: str,
    deleted_size: int | None,
    seed_deleted: set[int] | None,
    max_hamming: int | None,
) -> int:
    cube_size = 1 << width
    full = cube_size - 1
    deleted = [Bool(f"deleted_{mask}") for mask in range(cube_size)]
    solver = SolverFor("QF_FD") if engine == "sat" else Solver()
    solver.set(timeout=timeout_ms)

    solver.add(deleted[0] == False, deleted[full] == False)
    if deleted_size is not None:
        if not 0 <= deleted_size <= cube_size - 2:
            raise ValueError("deleted size must leave the empty and full sets present")
        solver.add(PbEq([(value, 1) for value in deleted], deleted_size))
    if seed_deleted is not None:
        if max_hamming is None:
            raise ValueError("a seed certificate requires --max-hamming")
        if any(mask < 0 or mask >= cube_size for mask in seed_deleted):
            raise ValueError("seed certificate contains a mask outside the cube")
        solver.add(
            PbLe(
                [
                    (Not(value) if mask in seed_deleted else value, 1)
                    for mask, value in enumerate(deleted)
                ],
                max_hamming,
            )
        )
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
    # Coordinate relabeling is a valid global symmetry, but a labelled Hamming
    # ball around a seed is not invariant under that relabeling.
    for bit in range(width - 1) if seed_deleted is None else range(0):
        terms = []
        for mask in range(cube_size):
            left = bool(mask & (1 << bit))
            right = bool(mask & (1 << (bit + 1)))
            if left != right:
                terms.append((deleted[mask], 1 if left else -1))
        solver.add(PbGe(terms, 0))

    aggregate_count = add_subcube_density_cuts(solver, deleted, width) if aggregate_mode == "eager" else 0
    print(f"aggregate_subcube_cuts={aggregate_count}")

    learned: set[tuple[int, int, int]] = set()
    learned_density: set[int] = set(range(1, cube_size)) if aggregate_mode == "eager" else set()
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
        density_violations = (
            violated_subcube_density(family, width, batch)
            if aggregate_mode == "lazy"
            else []
        )
        inserted_density = 0
        for union in density_violations:
            if union in learned_density:
                continue
            learned_density.add(union)
            solver.add(PbGe(subcube_density_terms(deleted, union), 0))
            inserted_density += 1
        if inserted_density:
            print(
                f"round={round_index} deleted={len(family)} min_surplus={min(surplus)} "
                f"density_violations={len(density_violations)} "
                f"density_cuts={len(learned_density)} clauses={len(learned)} "
                f"check_s={check_seconds:.3f} total_s={perf_counter() - started:.3f}"
            )
            continue
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
    parser.add_argument(
        "--aggregate-cuts",
        choices=("none", "lazy", "eager"),
        default="lazy",
        help="how to add exact subcube-density closure cuts",
    )
    parser.add_argument("--engine", choices=("sat", "smt"), default="sat")
    parser.add_argument(
        "--deleted-size",
        type=int,
        help="fix the dual deleted-family cardinality",
    )
    parser.add_argument("--seed-certificate", type=Path)
    parser.add_argument("--max-hamming", type=int)
    args = parser.parse_args()
    seed_deleted = None
    if args.seed_certificate:
        document = json.loads(args.seed_certificate.read_text(encoding="utf-8"))
        if int(document["width"]) != args.width:
            raise ValueError("seed certificate width mismatch")
        seed_deleted = {int(mask) for mask in document["deleted"]}
        if args.deleted_size is None:
            args.deleted_size = len(seed_deleted)
    return solve(
        args.width,
        args.batch,
        args.rounds,
        args.timeout_ms,
        args.aggregate_cuts,
        args.engine,
        args.deleted_size,
        seed_deleted,
        args.max_hamming,
    )


if __name__ == "__main__":
    raise SystemExit(main())
