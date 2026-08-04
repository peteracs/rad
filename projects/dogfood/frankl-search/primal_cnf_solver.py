"""Exact lazy-CNF search at a fixed family size.

One Boolean variable records membership of each subset of [n].  Cardinality
circuits impose the family size and strict coordinate minorities.  Pairwise
union closure is separated lazily from concrete SAT models, avoiding the
roughly 33 million non-tautological Horn clauses at width 13.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from time import perf_counter

from cardinality_solver import make_cardinality_solver
from frontier_seed import sampled_seed


def closure_violations(family: list[int], present: set[int], limit: int) -> list[tuple[int, int, int]]:
    result: list[tuple[int, int, int]] = []
    for left_index, left in enumerate(family):
        for right in family[left_index:]:
            union = left | right
            if union not in present:
                result.append((left, right, union))
                if len(result) >= limit:
                    return result
    return result


def solve(
    width: int,
    family_size: int,
    batch: int,
    rounds: int,
    empty: str,
    engine: str,
    minimal_overlap: int | None,
    size_split: int | None,
    seed_phases: bool,
    frequency_floor: int,
    output: Path | None,
) -> int:
    cube_size = 1 << width
    if not 2 <= family_size <= cube_size:
        raise ValueError("family size must lie between 2 and the cube size")
    minority_bound = (family_size - 1) // 2
    membership = [mask + 1 for mask in range(cube_size)]
    full = cube_size - 1
    clauses: list[list[int]] = [[full + 1]]
    if empty == "included":
        clauses.append([1])
    elif empty == "excluded":
        clauses.append([-1])

    tight_coordinates: tuple[int, ...] = ()
    co_singleton_coordinates: tuple[int, ...] = ()
    if minimal_overlap is not None:
        if width < 6:
            raise ValueError("minimal-counterexample structure requires width at least 6")
        if minimal_overlap not in range(4):
            raise ValueError("minimal overlap must be one of 0, 1, 2, 3")
        # Any smallest counterexample has at least three elements of frequency
        # (m-1)/2 and at least three elements x for which U-{x} belongs to the
        # family.  Coordinate relabeling reduces the relative placement of
        # two witness triples to their intersection size.
        tight_coordinates = (0, 1, 2)
        co_singleton_coordinates = tuple(range(minimal_overlap)) + tuple(
            range(3, 6 - minimal_overlap)
        )
        for bit in co_singleton_coordinates:
            clauses.append([((cube_size - 1) ^ (1 << bit)) + 1])
        # A smallest counterexample contains no one- or two-element member,
        # and Lo Faro's lower bound d_min > 9 applies to every coordinate.
        for mask in range(cube_size):
            if mask.bit_count() in (1, 2):
                clauses.append([-(mask + 1)])

    if size_split is not None:
        if minimal_overlap is None:
            raise ValueError("size split is a minimal-counterexample restriction")
        if size_split not in range(3, 10):
            raise ValueError("the width-13 frontier size split must lie in 3..9")

    learned: set[tuple[int, int, int]] = set()
    learned_separation: set[tuple[int, int]] = set()
    started = perf_counter()
    solver = make_cardinality_solver(engine, cube_size)
    try:
        for clause in clauses:
            solver.add_clause(clause)
        # Exactly m selected masks, represented without lowering either side to
        # an auxiliary-variable CNF.  The second inequality is the at-least
        # half over the complemented membership literals.
        solver.add_exactly(membership, family_size)
        for bit in range(width):
            carriers = [mask + 1 for mask in range(cube_size) if mask >> bit & 1]
            solver.add_atmost(carriers, minority_bound)
            if minimal_overlap is not None and frequency_floor > 0:
                solver.add_atleast(carriers, frequency_floor)
            if bit in tight_coordinates:
                solver.add_atleast(carriers, minority_bound)
        if size_split is not None:
            low_ten = [
                mask + 1 for mask in range(cube_size) if mask.bit_count() <= size_split
            ]
            low_twenty_seven = [
                mask + 1
                for mask in range(cube_size)
                if mask.bit_count() <= 12 - size_split
            ]
            solver.add_atleast(low_ten, 10)
            solver.add_atleast(low_twenty_seven, 27)
        if seed_phases and minimal_overlap is not None and size_split is not None:
            seed = sampled_seed(width, family_size, minimal_overlap, size_split)
            if seed is None:
                raise RuntimeError("failed to construct a theorem-compatible phase seed")
            solver.prefer({mask + 1 for mask in seed})
            print(
                f"phase-seed family={len(seed)} rank_range="
                f"{min(mask.bit_count() for mask in seed)}.."
                f"{max(mask.bit_count() for mask in seed)}",
                flush=True,
            )
        for round_index in range(rounds):
            checked = perf_counter()
            satisfiable = solver.solve()
            check_seconds = perf_counter() - checked
            if not satisfiable:
                print(
                    f"round={round_index} result=unsat width={width} family={family_size} "
                    f"empty={empty} overlap={minimal_overlap} variables={cube_size} "
                    f"size_split={size_split} base_clauses={len(clauses)} "
                    f"learned={len(learned)} check_s={check_seconds:.3f} "
                    f"total_s={perf_counter() - started:.3f}",
                    flush=True,
                )
                return 0

            positive = solver.positive_model()
            family = [mask for mask in range(cube_size) if mask + 1 in positive]
            present = set(family)
            missing_separation = [
                (left, right)
                for left in range(width)
                for right in range(left + 1, width)
                if not any(
                    ((mask >> left) & 1) != ((mask >> right) & 1) for mask in family
                )
            ]
            for left, right in missing_separation:
                if (left, right) in learned_separation:
                    continue
                learned_separation.add((left, right))
                solver.add_clause(
                    [
                        mask + 1
                        for mask in range(cube_size)
                        if ((mask >> left) & 1) != ((mask >> right) & 1)
                    ]
                )
            violations = closure_violations(family, present, batch)
            frequencies = [sum(mask >> bit & 1 for mask in family) for bit in range(width)]
            print(
                f"round={round_index} result=model family={len(family)} "
                f"frequency_range={min(frequencies)}..{max(frequencies)} "
                f"overlap={minimal_overlap} "
                f"size_split={size_split} "
                f"separation_violations={len(missing_separation)} "
                f"violations={len(violations)} learned={len(learned)} "
                f"check_s={check_seconds:.3f} total_s={perf_counter() - started:.3f}",
                flush=True,
            )
            if not violations and not missing_separation:
                document = {
                    "schema": "rad.boolean-lattice.fixed-size-counterexample.v1",
                    "width": width,
                    "family_size": family_size,
                    "family": family,
                    "frequencies": frequencies,
                    "counterexample": max(frequencies) * 2 < family_size,
                    "empty": empty,
                    "learned_clauses": len(learned),
                }
                encoded = json.dumps(document, indent=2, sort_keys=True)
                print(encoded, flush=True)
                if output:
                    output.parent.mkdir(parents=True, exist_ok=True)
                    output.write_text(encoded + "\n", encoding="utf-8")
                return 1

            inserted = 0
            for left, right, union in violations:
                clause = (left, right, union)
                if clause in learned:
                    continue
                learned.add(clause)
                solver.add_clause([-(left + 1), -(right + 1), union + 1])
                inserted += 1
            if inserted == 0 and not missing_separation:
                raise RuntimeError("closure separator produced no new clause")

        print(
            f"round-limit width={width} family={family_size} empty={empty} "
            f"learned={len(learned)} total_s={perf_counter() - started:.3f}",
            flush=True,
        )
        return 2
    finally:
        solver.close()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--family-size", type=int, default=51)
    parser.add_argument("--batch", type=int, default=10_000)
    parser.add_argument("--rounds", type=int, default=10_000)
    parser.add_argument("--empty", choices=("free", "included", "excluded"), default="free")
    parser.add_argument(
        "--engine",
        choices=("minicard", "z3", "cadical"),
        default="z3",
    )
    parser.add_argument(
        "--minimal-overlap",
        type=int,
        choices=range(4),
        help="use one canonical overlap case for the two minimal-counterexample witness triples",
    )
    parser.add_argument("--no-seed-phases", action="store_true")
    parser.add_argument(
        "--frequency-floor",
        type=int,
        default=10,
        help="minimal-counterexample coordinate-frequency lower bound; 0 omits the redundant theorem cut",
    )
    parser.add_argument(
        "--size-split",
        type=int,
        choices=range(3, 10),
        help="Lo Faro rank-profile case w_10<=r and w_27<=12-r",
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    return solve(
        args.width,
        args.family_size,
        args.batch,
        args.rounds,
        args.empty,
        args.engine,
        args.minimal_overlap,
        args.size_split,
        not args.no_seed_phases,
        args.frequency_floor,
        args.output,
    )


if __name__ == "__main__":
    raise SystemExit(main())
