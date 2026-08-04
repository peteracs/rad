"""Explain the structure of an exact legal-deletion certificate.

This is deliberately independent of RAD and the native extension.  It
reconstructs the surviving family, audits closure, and reports generators,
coordinate implications, dominant-coordinate patterns, and union witnesses.
The report is diagnostic evidence, not a proof outside the audited family.
"""

from __future__ import annotations

import argparse
import collections
import json
from pathlib import Path


def members_from(document: dict[str, object]) -> tuple[int, list[int]]:
    width = int(document["width"])
    deleted = {int(member) for member in document["deleted"]}
    if len(deleted) != len(document["deleted"]):
        raise ValueError("deleted list contains duplicates")
    if any(member < 0 or member >= 1 << width for member in deleted):
        raise ValueError("deleted member lies outside the Boolean cube")
    return width, [member for member in range(1 << width) if member not in deleted]


def render(member: int, width: int) -> list[int]:
    return [bit for bit in range(width) if member & 1 << bit]


def analyze(document: dict[str, object]) -> dict[str, object]:
    width, family = members_from(document)
    present = set(family)
    for left in family:
        for right in family:
            if left | right not in present:
                raise ValueError(f"family is not union-closed: {left} | {right}")

    frequencies = [sum(bool(member & 1 << bit) for member in family) for bit in range(width)]
    maximum = max(frequencies)
    dominant = [bit for bit, frequency in enumerate(frequencies) if frequency == maximum]
    rank_histogram = collections.Counter(member.bit_count() for member in family)

    irreducibles: list[int] = []
    proper_union_witnesses: dict[int, int] = {}
    for union in family:
        subsets = [member for member in family if member != union and member & ~union == 0]
        witnesses = sum(
            left | right == union
            for left_index, left in enumerate(subsets)
            for right in subsets[left_index:]
        )
        proper_union_witnesses[union] = witnesses
        if witnesses == 0:
            irreducibles.append(union)

    implications: list[tuple[int, int]] = []
    for antecedent in range(width):
        for consequent in range(width):
            if antecedent == consequent:
                continue
            if all(
                not member & 1 << antecedent or member & 1 << consequent
                for member in family
            ):
                implications.append((antecedent, consequent))

    pattern_counts: collections.Counter[int] = collections.Counter()
    for member in family:
        pattern = sum(
            bool(member & 1 << bit) << index for index, bit in enumerate(dominant)
        )
        pattern_counts[pattern] += 1

    pair_pressures: list[dict[str, object]] = []
    for left in range(width):
        for right in range(left + 1, width):
            cells = [0, 0, 0, 0]
            left_only: list[int] = []
            right_only: list[int] = []
            for member in family:
                pattern = int(bool(member & 1 << left)) + 2 * int(
                    bool(member & 1 << right)
                )
                cells[pattern] += 1
                if pattern == 1:
                    left_only.append(member)
                elif pattern == 2:
                    right_only.append(member)
            cross_images = {a | b for a in left_only for b in right_only}
            if not cross_images <= present:
                raise ValueError("cross-coordinate union audit failed")
            pair_pressures.append(
                {
                    "coordinates": [left, right],
                    "neither": cells[0],
                    "left_only": cells[1],
                    "right_only": cells[2],
                    "both": cells[3],
                    "both_minus_neither": cells[3] - cells[0],
                    "cross_pairs": len(left_only) * len(right_only),
                    "distinct_cross_unions": len(cross_images),
                }
            )
    pair_pressures.sort(
        key=lambda item: (int(item["both_minus_neither"]), item["coordinates"]),
        reverse=True,
    )

    return {
        "schema": "rad.frankl.deletion-structure.v1",
        "width": width,
        "family_size": len(family),
        "frequencies": frequencies,
        "margin": 2 * maximum - len(family),
        "dominant_coordinates": dominant,
        "rank_histogram": dict(sorted(rank_histogram.items())),
        "join_irreducible_count": len(irreducibles),
        "join_irreducibles": [render(member, width) for member in irreducibles],
        "coordinate_implications": implications,
        "dominant_pattern_counts": {
            format(pattern, f"0{len(dominant)}b"): count
            for pattern, count in sorted(pattern_counts.items())
        },
        # If two coordinates are both strict minorities in an odd-sized
        # family, their 2x2 incidence table necessarily has neither > both.
        # Positive pressure is therefore a concrete pairwise obstruction.
        "pairs_failing_minority_necessary_condition": sum(
            int(item["both_minus_neither"]) >= 0 for item in pair_pressures
        ),
        "strongest_pair_pressures": pair_pressures[:10],
        "deletable_survivor_count": sum(
            witnesses == 0 for witnesses in proper_union_witnesses.values()
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = analyze(json.loads(args.certificate.read_text(encoding="utf-8")))
    encoded = json.dumps(report, indent=2, sort_keys=True)
    print(encoded)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
