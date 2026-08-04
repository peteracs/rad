"""Exact minimum-margin solver for cyclic-invariant union-closed families."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from time import perf_counter

from pysat.examples.rc2 import RC2
from pysat.formula import WCNF

from orbit_horn import iter_join_horn_clauses, orbit_owner
from regular_family_miner import rotation_orbits


def solve(width: int, proper: bool, separating: bool, compact: bool, output: Path | None) -> int:
    cube_size = 1 << width
    orbits = rotation_orbits(width)
    owner = orbit_owner(orbits, cube_size)
    formula = WCNF()

    full_orbit = owner[cube_size - 1]
    formula.append([full_orbit + 1])
    if proper:
        formula.append([-(index + 1) for index in range(len(orbits))])
    if separating:
        # Rotation reduces coordinate-pair separation to one clause for every
        # cyclic distance. A minimal counterexample may always be quotiented to
        # this separating case.
        for distance in range(1, width // 2 + 1):
            distinguishers = []
            for index, orbit in enumerate(orbits):
                if any(((mask >> 0) & 1) != ((mask >> distance) & 1) for mask in orbit):
                    distinguishers.append(index + 1)
            formula.append(distinguishers)
    horn_digest = hashlib.sha256()
    horn_clause_count = 0
    for left, right, target in iter_join_horn_clauses(orbits, owner):
        formula.append([-(left + 1), -(right + 1), target + 1])
        if horn_clause_count:
            horn_digest.update(b"\n")
        horn_digest.update(f"{left},{right}->{target}".encode())
        horn_clause_count += 1

    coefficients: list[int] = []
    negative_constant = 0
    for index, orbit in enumerate(orbits):
        incidence = len(orbit) * orbit[0].bit_count()
        assert incidence % width == 0
        coefficient = 2 * (incidence // width) - len(orbit)
        coefficients.append(coefficient)
        if coefficient > 0:
            # Pay when the positive-margin orbit is selected.
            formula.append([-(index + 1)], weight=coefficient)
        elif coefficient < 0:
            # Pay when a negative-margin orbit is omitted.
            formula.append([index + 1], weight=-coefficient)
            negative_constant += coefficient
    objective_bytes = "\n".join(
        f"{index}:{coefficient}" for index, coefficient in enumerate(coefficients)
    ).encode()

    started = perf_counter()
    with RC2(formula, solver="cadical195", adapt=True, exhaust=True, incr=False) as optimizer:
        model = optimizer.compute()
        optimum_cost = optimizer.cost
    elapsed = perf_counter() - started
    if model is None:
        raise RuntimeError("hard orbit-closure theory is unexpectedly inconsistent")

    positive = {literal for literal in model if literal > 0}
    selected = {index for index in range(len(orbits)) if index + 1 in positive}
    family = sorted(mask for index in selected for mask in orbits[index])
    frequency = sum(mask.bit_count() for mask in family) // width
    margin = 2 * frequency - len(family)
    assert margin == negative_constant + optimum_cost

    document = {
        "schema": "rad.boolean-lattice.regular-margin-proof.v1",
        "width": width,
        "orbit_count": len(orbits),
        "horn_clause_count": horn_clause_count,
        "horn_digest": horn_digest.hexdigest(),
        "objective_digest": hashlib.sha256(objective_bytes).hexdigest(),
        "hard_clause_count": len(formula.hard),
        "soft_clause_count": len(formula.soft),
        "minimum_margin": margin,
        "proper_family_required": proper,
        "separating_required": separating,
        "counterexample_exists": margin < 0,
        "selected_orbits_encoding": (
            "all_except_empty"
            if selected == set(range(1, len(orbits)))
            else "explicit"
        ),
        "family_size": len(family),
        "frequency": frequency,
        "family_encoding": "all_nonempty_subsets" if family == list(range(1, cube_size)) else "explicit",
        "elapsed_seconds": elapsed,
        "optimizer": "RC2/cadical195",
    }
    if not compact or document["selected_orbits_encoding"] == "explicit":
        document["selected_orbits"] = sorted(selected)
    if not compact or document["family_encoding"] == "explicit":
        document["family"] = family
    encoded = json.dumps(document, indent=2, sort_keys=True)
    print(encoded)
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(encoded + "\n", encoding="utf-8")
    return 1 if margin < 0 else 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--proper", action="store_true", help="exclude the complete Boolean cube")
    parser.add_argument(
        "--allow-nonseparating",
        action="store_true",
        help="do not impose the standard coordinate-separation reduction",
    )
    parser.add_argument("--compact", action="store_true", help="use a canonical shorthand for known extremals")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    return solve(args.width, args.proper, not args.allow_nonseparating, args.compact, args.output)


if __name__ == "__main__":
    raise SystemExit(main())
