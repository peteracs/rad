"""Independent verifier for a cyclic-invariant minimum-margin certificate."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from orbit_horn import iter_join_horn_clauses, orbit_owner
from regular_family_miner import is_separating, is_union_closed, rotation_orbits


def reconstruct_family(document: dict[str, object]) -> list[int]:
    width = int(document["width"])
    if document.get("family_encoding") == "all_nonempty_subsets":
        return list(range(1, 1 << width))
    return [int(mask) for mask in document["family"]]


def verify(document: dict[str, object]) -> dict[str, object]:
    width = int(document["width"])
    cube_size = 1 << width
    orbits = rotation_orbits(width)
    owner = orbit_owner(orbits, cube_size)
    horn_digest = hashlib.sha256()
    horn_clause_count = 0
    for left, right, target in iter_join_horn_clauses(orbits, owner):
        if horn_clause_count:
            horn_digest.update(b"\n")
        horn_digest.update(f"{left},{right}->{target}".encode())
        horn_clause_count += 1
    digest = horn_digest.hexdigest()
    if len(orbits) != int(document["orbit_count"]):
        raise ValueError("orbit count mismatch")
    if horn_clause_count != int(document["horn_clause_count"]):
        raise ValueError("Horn clause count mismatch")
    if digest != document["horn_digest"]:
        raise ValueError("Horn theory digest mismatch")
    coefficients = []
    for orbit in orbits:
        incidence = len(orbit) * orbit[0].bit_count()
        if incidence % width:
            raise ValueError("orbit incidence is not coordinate-regular")
        coefficients.append(2 * (incidence // width) - len(orbit))
    objective_bytes = "\n".join(
        f"{index}:{coefficient}" for index, coefficient in enumerate(coefficients)
    ).encode()
    objective_digest = hashlib.sha256(objective_bytes).hexdigest()
    if objective_digest != document["objective_digest"]:
        raise ValueError("weighted objective digest mismatch")
    expected_hard = horn_clause_count + 1
    expected_hard += bool(document["proper_family_required"])
    expected_hard += width // 2 if bool(document["separating_required"]) else 0
    if expected_hard != int(document["hard_clause_count"]):
        raise ValueError("hard clause count mismatch")
    if sum(coefficient != 0 for coefficient in coefficients) != int(document["soft_clause_count"]):
        raise ValueError("soft clause count mismatch")

    family = reconstruct_family(document)
    if len(family) != len(set(family)) or family != sorted(family):
        raise ValueError("extremal family must be sorted and duplicate-free")
    members = sum(1 << mask for mask in family)
    if not is_union_closed(members, cube_size):
        raise ValueError("extremal family is not union-closed")
    if bool(document["separating_required"]) and not is_separating(members, width):
        raise ValueError("extremal family is not separating")
    frequencies = [sum(mask >> bit & 1 for mask in family) for bit in range(width)]
    if len(set(frequencies)) != 1:
        raise ValueError("cyclic extremal family is not regular")
    margin = 2 * frequencies[0] - len(family)
    if len(family) != int(document["family_size"]):
        raise ValueError("family size mismatch")
    if frequencies[0] != int(document["frequency"]):
        raise ValueError("frequency mismatch")
    if margin != int(document["minimum_margin"]):
        raise ValueError("margin mismatch")
    if bool(document["counterexample_exists"]) != (margin < 0):
        raise ValueError("counterexample verdict mismatch")
    return {
        "schema": "rad.boolean-lattice.regular-margin-verification.v1",
        "width": width,
        "orbit_count": len(orbits),
        "horn_clause_count": horn_clause_count,
        "horn_digest": digest,
        "objective_digest": objective_digest,
        "hard_clause_count": expected_hard,
        "soft_clause_count": sum(coefficient != 0 for coefficient in coefficients),
        "family_size": len(family),
        "frequency": frequencies[0],
        "margin": margin,
        "closed": True,
        "regular": True,
        "separating": is_separating(members, width),
        "structural_certificate_valid": True,
        "optimization_claim": "requires replaying the weighted MaxSAT solve",
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
