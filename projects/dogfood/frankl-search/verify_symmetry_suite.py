"""Independent structural verifier for permutation-symmetry exclusions."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from orbit_horn import iter_join_horn_clauses, orbit_owner
from permutation_symmetry_solver import canonical_permutation, subset_orbits


def verify(document: dict[str, object]) -> dict[str, object]:
    width = int(document["width"])
    reports = []
    total_horn = 0
    for case in document["cases"]:
        cycles = tuple(int(length) for length in case["cycle_type"])
        if sum(cycles) != width:
            raise ValueError(f"cycle type {cycles} does not partition width {width}")
        permutation, coordinate_orbits = canonical_permutation(cycles)
        orbits = subset_orbits(permutation)
        owner = orbit_owner(orbits, 1 << width)
        horn_count = sum(1 for _ in iter_join_horn_clauses(orbits, owner))
        if len(orbits) != int(case["subset_orbits"]):
            raise ValueError(f"subset orbit count mismatch for {cycles}")
        if horn_count != int(case["horn_clauses"]):
            raise ValueError(f"Horn clause count mismatch for {cycles}")
        if bool(case["counterexample_exists"]):
            raise ValueError(f"suite contains a positive counterexample claim for {cycles}")
        total_horn += horn_count
        reports.append(
            {
                "cycle_type": list(cycles),
                "coordinate_orbits": len(coordinate_orbits),
                "subset_orbits": len(orbits),
                "horn_clauses": horn_count,
                "structural_theory_valid": True,
                "unsat_claim": "requires replaying the CNF/CaDiCaL solve",
            }
        )
    return {
        "schema": "rad.boolean-lattice.permutation-symmetry-verification.v1",
        "width": width,
        "case_count": len(reports),
        "horn_clause_count": total_horn,
        "cases": reports,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    document = json.loads(args.certificate.read_text(encoding="utf-8"))
    report = verify(document)
    encoded = json.dumps(report, indent=2, sort_keys=True)
    print(encoded)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
