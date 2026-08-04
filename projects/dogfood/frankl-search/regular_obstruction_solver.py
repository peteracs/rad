"""Exact lazy Horn solver for cyclic-invariant union-closed families.

Rotation invariance collapses one Boolean variable per subset to one variable
per binary-necklace orbit.  A selected orbit contributes uniformly to every
coordinate, so strict failure of Frankl is one pseudo-Boolean rank inequality.
Union closure is separated lazily as Horn clauses between orbit variables.

At width 13 this reduces 8,192 membership variables to 632 orbit variables.
The result is exact for the entire cyclic-invariant class, not a generator or
beam-search heuristic.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from time import perf_counter

from z3 import BitVecVal, Bool, If, Not, Or, PbLe, Solver, SolverFor, Sum, ULE, is_true, sat

from orbit_horn import orbit_owner
from regular_family_miner import rotation_orbits
from weighted_cnf import encode_weighted_at_most


def orbit_index(orbits: list[tuple[int, ...]], cube_size: int) -> list[int]:
    return orbit_owner(orbits, cube_size)


def violated_orbit_clauses(
    selected: set[int],
    orbits: list[tuple[int, ...]],
    owner: list[int],
    limit: int,
) -> list[tuple[int, int, int]]:
    """Return canonical missing-union clauses for one concrete orbit family."""

    found: set[tuple[int, int, int]] = set()
    chosen = sorted(selected)
    for left_pos, left_orbit in enumerate(chosen):
        for right_orbit in chosen[left_pos:]:
            for left in orbits[left_orbit]:
                for right in orbits[right_orbit]:
                    target_orbit = owner[left | right]
                    if target_orbit in selected:
                        continue
                    found.add((left_orbit, right_orbit, target_orbit))
                    if len(found) >= limit:
                        return sorted(found)
    return sorted(found)


def solve(
    width: int,
    batch: int,
    rounds: int,
    timeout_ms: int,
    engine: str,
    output: Path | None,
) -> int:
    cube_size = 1 << width
    orbits = rotation_orbits(width)
    owner = orbit_index(orbits, cube_size)
    z3_selected = [Bool(f"orbit_{index}") for index in range(len(orbits))]
    if engine == "cadical":
        from pysat.solvers import Solver as PySatSolver

        solver = None
    elif engine == "fd":
        solver = SolverFor("QF_FD")
    elif engine == "bv":
        solver = SolverFor("QF_BV")
    else:
        solver = Solver()
    if solver is not None:
        solver.set(timeout=timeout_ms)

    full_orbit = owner[cube_size - 1]

    # For a rotation orbit O of rank r, |O|*r/n is its contribution to each
    # coordinate frequency. Strict minority is
    #
    #   2 * total_rank / n <= family_size - 1.
    #
    # Orbit transitivity guarantees |O|*rank(O) is divisible by n, so we can
    # express the inequality directly in per-coordinate units.  This removes
    # a common factor of n from every coefficient and dramatically shrinks the
    # exact PB encoding at prime widths.
    counterexample_terms: list[tuple[object, int]] = []
    for value, orbit in zip(z3_selected, orbits):
        rank = orbit[0].bit_count()
        incidence = len(orbit) * rank
        assert incidence % width == 0
        coefficient = 2 * (incidence // width) - len(orbit)
        if coefficient:
            counterexample_terms.append((value, coefficient))
    if engine == "cadical":
        negative_total = sum(-coefficient for _, coefficient in counterexample_terms if coefficient < 0)
        # Build the DIMACS variable association directly from orbit indices;
        # zero coefficients intentionally contribute no circuit input.
        dimacs_terms = []
        for index, orbit in enumerate(orbits):
            incidence = len(orbit) * orbit[0].bit_count() // width
            coefficient = 2 * incidence - len(orbit)
            if coefficient:
                dimacs_terms.append(((index + 1) if coefficient > 0 else -(index + 1), abs(coefficient)))
        _, clauses = encode_weighted_at_most(len(orbits), dimacs_terms, negative_total - 1)
        clauses.append([full_orbit + 1])
        clauses.append(
            [index + 1 for index, orbit in enumerate(orbits) if orbit != (cube_size - 1,)]
        )
        solver = PySatSolver(name="cadical195", bootstrap_with=clauses)
    elif engine == "bv":
        # Rewrite signed coefficients as one nonnegative weighted sum.  The
        # bounded bit-vector encoding lets a pure SAT backend reason about the
        # rank obstruction without a separate integer theory.
        negative_total = sum(-coefficient for _, coefficient in counterexample_terms if coefficient < 0)
        bound = negative_total - 1
        # The accumulator must represent every possible weighted sum.  Sizing
        # it only from the bound would permit modular overflow and manufacture
        # false counterexamples.
        maximum_sum = sum(abs(coefficient) for _, coefficient in counterexample_terms)
        bit_width = max(1, maximum_sum.bit_length())
        weighted = [
            If(
                value if coefficient > 0 else Not(value),
                BitVecVal(abs(coefficient), bit_width),
                BitVecVal(0, bit_width),
            )
            for value, coefficient in counterexample_terms
        ]
        solver.add(ULE(Sum(weighted), BitVecVal(bound, bit_width)))
    else:
        solver.add(PbLe(counterexample_terms, -1))

    # Exclude the degenerate family {full}; it cannot satisfy the strict
    # minority inequality, but this makes the intended ground-set contract
    # explicit and produces a clearer model if the objective is edited.
    if engine != "cadical":
        solver.add(z3_selected[full_orbit])
        solver.add(Or(*[z3_selected[index] for index, orbit in enumerate(orbits) if orbit != (cube_size - 1,)]))

    learned: set[tuple[int, int, int]] = set()
    started = perf_counter()
    for round_index in range(rounds):
        checked = perf_counter()
        result = solver.solve() if engine == "cadical" else solver.check()
        check_seconds = perf_counter() - checked
        is_satisfiable = result is True if engine == "cadical" else result == sat
        if not is_satisfiable:
            rendered_result = "unsat" if result is False else str(result)
            print(
                f"round={round_index} result={rendered_result} width={width} "
                f"orbits={len(orbits)} clauses={len(learned)} "
                f"check_s={check_seconds:.3f} total_s={perf_counter() - started:.3f}"
            )
            return 0 if rendered_result == "unsat" else 2

        if engine == "cadical":
            positive = {literal for literal in solver.get_model() if literal > 0}
            chosen = {index for index in range(len(orbits)) if index + 1 in positive}
        else:
            model = solver.model()
            chosen = {index for index, value in enumerate(z3_selected) if is_true(model.eval(value))}
        violations = violated_orbit_clauses(chosen, orbits, owner, batch)
        family = sorted(mask for index in chosen for mask in orbits[index])
        rank_sum = sum(mask.bit_count() for mask in family)
        frequency = rank_sum // width
        margin = 2 * frequency - len(family)
        print(
            f"round={round_index} selected_orbits={len(chosen)} family={len(family)} "
            f"margin={margin} violations={len(violations)} clauses={len(learned)} "
            f"check_s={check_seconds:.3f} total_s={perf_counter() - started:.3f}"
        )

        if not violations:
            document = {
                "schema": "rad.boolean-lattice.regular-counterexample.v1",
                "width": width,
                "selected_orbits": sorted(chosen),
                "family": family,
                "family_size": len(family),
                "frequency": frequency,
                "margin": margin,
                "counterexample": margin < 0,
                "learned_clauses": len(learned),
            }
            encoded = json.dumps(document, indent=2, sort_keys=True)
            print(encoded)
            if output:
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text(encoded + "\n", encoding="utf-8")
            return 1

        inserted = 0
        for left, right, target in violations:
            key = (left, right, target)
            if key in learned:
                continue
            learned.add(key)
            if engine == "cadical":
                solver.add_clause([-(left + 1), -(right + 1), target + 1])
            else:
                solver.add(Or(Not(z3_selected[left]), Not(z3_selected[right]), z3_selected[target]))
            inserted += 1
        if inserted == 0:
            raise RuntimeError("orbit separator returned no new Horn clause")

    print(
        f"round limit reached width={width} orbits={len(orbits)} "
        f"clauses={len(learned)} total_s={perf_counter() - started:.3f}"
    )
    return 2


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--batch", type=int, default=20_000)
    parser.add_argument("--rounds", type=int, default=1_000)
    parser.add_argument("--timeout-ms", type=int, default=120_000)
    parser.add_argument("--engine", choices=("fd", "smt", "bv", "cadical"), default="cadical")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    return solve(args.width, args.batch, args.rounds, args.timeout_ms, args.engine, args.output)


if __name__ == "__main__":
    raise SystemExit(main())
