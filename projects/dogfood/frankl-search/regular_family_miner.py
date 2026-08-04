"""Exact theorem miner for cyclic-invariant union-closed families.

The search space is the set of unions of rotation orbits, rather than the
entire powerset of the Boolean cube.  Every accepted family is regular: each
coordinate has the same frequency.  This makes Frankl's conclusion equivalent
to the non-negativity of one integer margin

    2 * total_rank / width - family_size.

The miner is deliberately independent of RAD's VM implementation.  Its JSON
output is consumed by the RAD dogfood as an exact, reproducible certificate.
"""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from pathlib import Path
from time import perf_counter


def rotate(mask: int, width: int, amount: int) -> int:
    amount %= width
    full = (1 << width) - 1
    if amount == 0:
        return mask
    return ((mask << amount) | (mask >> (width - amount))) & full


def rotation_orbits(width: int) -> list[tuple[int, ...]]:
    seen: set[int] = set()
    result: list[tuple[int, ...]] = []
    for mask in range(1 << width):
        if mask in seen:
            continue
        orbit = tuple(sorted({rotate(mask, width, amount) for amount in range(width)}))
        seen.update(orbit)
        result.append(orbit)
    return result


def is_union_closed(members: int, cube_size: int) -> bool:
    family = [mask for mask in range(cube_size) if members >> mask & 1]
    for left_index, left in enumerate(family):
        for right in family[left_index:]:
            if not (members >> (left | right)) & 1:
                return False
    return True


def is_separating(members: int, width: int) -> bool:
    signatures = []
    for bit in range(width):
        signature = 0
        for mask in range(1 << width):
            if members >> mask & 1 and mask >> bit & 1:
                signature |= 1 << mask
        signatures.append(signature)
    return len(set(signatures)) == width


@dataclass(frozen=True)
class ExtremalFamily:
    orbit_selection: int
    members: tuple[int, ...]
    frequency: int
    margin: int


def classify(width: int) -> dict[str, object]:
    cube_size = 1 << width
    orbits = rotation_orbits(width)
    orbit_member_bits = [sum(1 << mask for mask in orbit) for orbit in orbits]
    full_orbit = next(index for index, orbit in enumerate(orbits) if orbit == (cube_size - 1,))

    accepted = 0
    minimum_margin: int | None = None
    extremals: list[ExtremalFamily] = []
    proper_minimum_margin: int | None = None
    proper_extremals: list[ExtremalFamily] = []
    started = perf_counter()

    # A nonempty rotation-invariant family whose ground set is [width] must
    # contain the full set after taking the union of all of its members.
    for selection in range(1 << len(orbits)):
        if not selection >> full_orbit & 1:
            continue
        member_bits = 0
        for orbit_index, bits in enumerate(orbit_member_bits):
            if selection >> orbit_index & 1:
                member_bits |= bits
        if not is_union_closed(member_bits, cube_size):
            continue
        if not is_separating(member_bits, width):
            continue

        family = tuple(mask for mask in range(cube_size) if member_bits >> mask & 1)
        rank_sum = sum(mask.bit_count() for mask in family)
        assert rank_sum % width == 0
        frequency = rank_sum // width
        margin = 2 * frequency - len(family)
        accepted += 1

        candidate = ExtremalFamily(selection, family, frequency, margin)
        if minimum_margin is None or margin < minimum_margin:
            minimum_margin = margin
            extremals = [candidate]
        elif margin == minimum_margin:
            extremals.append(candidate)
        if len(family) < cube_size:
            if proper_minimum_margin is None or margin < proper_minimum_margin:
                proper_minimum_margin = margin
                proper_extremals = [candidate]
            elif margin == proper_minimum_margin:
                proper_extremals.append(candidate)

    def encode(family: ExtremalFamily) -> dict[str, object]:
        return {
            "orbit_selection": family.orbit_selection,
            "family_size": len(family.members),
            "frequency": family.frequency,
            "margin": family.margin,
            "members": list(family.members),
            "missing": [mask for mask in range(cube_size) if mask not in family.members],
        }

    return {
        "schema": "rad.boolean-lattice.regular-family-classification.v1",
        "width": width,
        "orbit_count": len(orbits),
        "candidate_count": 1 << (len(orbits) - 1),
        "accepted_count": accepted,
        "minimum_margin": minimum_margin,
        "proper_minimum_margin": proper_minimum_margin,
        "elapsed_seconds": perf_counter() - started,
        "extremal_count": len(extremals),
        "extremals": [encode(family) for family in extremals],
        "proper_extremal_count": len(proper_extremals),
        "proper_extremals": [encode(family) for family in proper_extremals],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=6)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 2 <= args.width <= 7:
        raise ValueError("the exact reference miner supports widths 2 through 7")
    document = classify(args.width)
    encoded = json.dumps(document, indent=2, sort_keys=True)
    print(encoded)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
