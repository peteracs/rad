"""Independent verifier for regular cyclic-orbit search certificates."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def rotate(mask: int, width: int) -> int:
    return ((mask << 1) & ((1 << width) - 1)) | (mask >> (width - 1))


def orbit(mask: int, width: int) -> list[int]:
    values: set[int] = set()
    current = mask
    for _ in range(width):
        values.add(current)
        current = rotate(current, width)
    return sorted(values)


def closure(generators: list[int]) -> list[int]:
    family = [0]
    present = {0}
    for generator in sorted(set(generators), key=lambda value: (value.bit_count(), value)):
        if generator in present:
            continue
        before = list(family)
        for member in before:
            joined = member | generator
            if joined not in present:
                present.add(joined)
                family.append(joined)
    return sorted(family)


def verify(document: dict[str, object]) -> dict[str, object]:
    width = int(document["width"])
    cube_size = 1 << width
    basis = [int(value) for value in document["basis"]]
    generators = [int(value) for value in document["generators"]]
    expected_generators = sorted({member for value in basis for member in orbit(value, width)})
    if sorted(set(generators)) != expected_generators:
        raise ValueError("generator list is not the union of the declared cyclic orbits")

    family = closure(generators)
    present = set(family)
    if len(family) != int(document["family_size"]):
        raise ValueError("family size does not match regenerated closure")
    for member in family:
        if rotate(member, width) not in present:
            raise ValueError("regenerated family is not rotation invariant")
    for left in family:
        for right in family:
            if left | right not in present:
                raise ValueError("regenerated family is not union-closed")

    frequencies = [sum(bool(member & (1 << bit)) for member in family) for bit in range(width)]
    if frequencies != [int(value) for value in document["frequencies"]]:
        raise ValueError("frequency vector mismatch")
    if len(set(frequencies)) != 1 or not bool(document["uniform"]):
        raise ValueError("certificate incorrectly claims coordinate regularity")
    margin = 2 * frequencies[0] - len(family)
    if margin != int(document["margin"]):
        raise ValueError("margin mismatch")

    deleted = [member for member in range(cube_size) if member not in present]
    deleted_rank_sum = sum(member.bit_count() for member in deleted)
    if len(deleted) != int(document["deleted_sets"]):
        raise ValueError("deleted set count mismatch")
    if deleted_rank_sum != int(document["deleted_rank_sum"]):
        raise ValueError("deleted rank sum mismatch")
    if deleted_rank_sum % width:
        raise ValueError("deleted incidence is not uniform")
    if margin != len(deleted) - 2 * (deleted_rank_sum // width):
        raise ValueError("regular dual margin identity failed")
    pressure = 2 * deleted_rank_sum - width * len(deleted)
    if pressure != int(document["dual_counterexample_pressure"]):
        raise ValueError("dual counterexample pressure mismatch")

    missing_orbits = sum(
        representative not in present
        for representative in range(1, cube_size - 1)
        if min(orbit(representative, width)) == representative
    )
    if missing_orbits != int(document["missing_orbits"]):
        raise ValueError("missing orbit count mismatch")

    digest = hashlib.sha256()
    for member in family:
        digest.update(member.to_bytes(8, "little"))
    return {
        "schema": "rad.frankl.regular-orbit-verification.v1",
        "width": width,
        "family_size": len(family),
        "frequency": frequencies[0],
        "margin": margin,
        "deleted_sets": len(deleted),
        "deleted_rank_sum": deleted_rank_sum,
        "deleted_average_rank": deleted_rank_sum / len(deleted) if deleted else 0,
        "dual_counterexample_pressure": pressure,
        "counterexample": margin < 0,
        "family_sha256": digest.hexdigest(),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = verify(json.loads(args.certificate.read_text(encoding="utf-8")))
    encoded = json.dumps(result, indent=2, sort_keys=True)
    print(encoded)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
