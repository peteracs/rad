#!/usr/bin/env python3
"""Independent verifier for RAD legal-deletion Frankl certificates."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def zeta(values: list[int], width: int) -> None:
    for bit in range(width):
        step = 1 << bit
        for mask in range(1 << width):
            if mask & step:
                values[mask] += values[mask ^ step]


def mobius(values: list[int], width: int) -> None:
    for bit in range(width):
        step = 1 << bit
        for mask in range(1 << width):
            if mask & step:
                values[mask] -= values[mask ^ step]


def verify(document: dict) -> dict:
    width = int(document["width"])
    cube_size = 1 << width
    deleted = [int(mask) for mask in document["deleted"]]
    if len(deleted) != len(set(deleted)):
        raise ValueError("deleted masks are not unique")
    if any(mask < 0 or mask >= cube_size for mask in deleted):
        raise ValueError("deleted mask lies outside the declared cube")

    present = [True] * cube_size
    for mask in deleted:
        present[mask] = False
    family = [mask for mask, exists in enumerate(present) if exists]
    if len(family) != int(document["family_size"]):
        raise ValueError("family size does not match deleted complement")

    subset_counts = [int(exists) for exists in present]
    zeta(subset_counts, width)
    union_counts = [count * count for count in subset_counts]
    mobius(union_counts, width)
    missing_unions = [
        mask for mask, count in enumerate(union_counts) if count and not present[mask]
    ]
    if missing_unions:
        raise ValueError(f"family is not union-closed; missing union {missing_unions[0]}")

    frequencies = [sum((mask >> bit) & 1 for mask in family) for bit in range(width)]
    deletion_frequencies = [
        sum((mask >> bit) & 1 for mask in deleted) for bit in range(width)
    ]
    surpluses = [2 * count - len(deleted) for count in deletion_frequencies]
    margin = max(2 * count - len(family) for count in frequencies)

    witnesses = [[0] * width for _ in range(width)]
    for mask in family:
        for left in range(width):
            for right in range(left + 1, width):
                if ((mask >> left) & 1) != ((mask >> right) & 1):
                    witnesses[left][right] += 1
    separating = all(
        witnesses[left][right] > 0
        for left in range(width)
        for right in range(left + 1, width)
    )

    deletable = []
    effective = []
    for mask in family:
        # Every surviving union witness involving `mask` disappears with it.
        # Thus exactly 2*q(mask)-1 ordered witnesses characterize deletability.
        if union_counts[mask] != 2 * subset_counts[mask] - 1:
            continue
        deletable.append(mask)
        keeps_coverage = all(
            frequencies[bit] > 1 for bit in range(width) if mask & (1 << bit)
        )
        keeps_separation = all(
            witnesses[left][right] > 1
            for left in range(width)
            for right in range(left + 1, width)
            if ((mask >> left) & 1) != ((mask >> right) & 1)
        )
        if keeps_coverage and keeps_separation:
            effective.append(mask)

    expected_frontier = [int(mask) for mask in document["frontier"]]
    if expected_frontier != effective:
        raise ValueError("effective deletion frontier does not match certificate")
    if frequencies != [int(value) for value in document["frequencies"]]:
        raise ValueError("frequency vector does not match certificate")
    if surpluses != [int(value) for value in document["deletion_surpluses"]]:
        raise ValueError("deletion-surplus vector does not match certificate")
    if margin != int(document["margin"]):
        raise ValueError("Frankl margin does not match certificate")
    if separating != bool(document["separating"]):
        raise ValueError("separation result does not match certificate")

    digest = hashlib.sha256(
        b"".join(mask.to_bytes(2, "little") for mask in family)
    ).hexdigest()
    return {
        "schema": "rad.frankl.deletion-verification.v1",
        "width": width,
        "family_size": len(family),
        "deleted_size": len(deleted),
        "frequencies": frequencies,
        "frankl_margin": margin,
        "union_closed": True,
        "separating": separating,
        "deletable_count": len(deletable),
        "effective_frontier_count": len(effective),
        "counterexample": margin < 0,
        "family_sha256": digest,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    report = verify(json.loads(args.certificate.read_text(encoding="utf-8")))
    encoded = json.dumps(report, indent=2, sort_keys=True)
    print(encoded)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(encoded + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
