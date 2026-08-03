#!/usr/bin/env python3
"""Independent verifier for RAD Frankl-search certificates.

This checker deliberately knows nothing about RAD worlds, forks, or bitsets.
It recomputes the mathematical claims using ordinary Python integers and sets.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


SCHEMA = "rad.frankl-search-certificate.v1"


class VerificationError(ValueError):
    """Certificate is malformed or one of its mathematical claims is false."""


def require(condition: bool, message: str) -> None:
    # Never use Python `assert` at a certificate boundary: `python -O` removes
    # assertions and would otherwise turn the verifier into an accept-all.
    if not condition:
        raise VerificationError(message)


def closure(generators: list[int]) -> list[int]:
    family = {0}
    for generator in generators:
        family |= {member | generator for member in tuple(family)}
    return sorted(family)


def require_union_closed(family: list[int], universe: int) -> None:
    """Verify closure with exact OR convolution in O(n 2^n).

    The zeta transform counts family members below every mask. Squaring those
    counts and applying Möbius inversion gives the exact number of ordered
    pairs whose union is each mask. Any absent mask with a nonzero count is a
    missing union witness. This is independent of RAD and replaces an O(m²)
    Python loop for dense families.
    """

    size = 1 << universe
    present = [0] * size
    for member in family:
        present[member] = 1
    transformed = present.copy()
    for bit_index in range(universe):
        bit = 1 << bit_index
        for mask in range(size):
            if mask & bit:
                transformed[mask] += transformed[mask ^ bit]
    pair_unions = [count * count for count in transformed]
    for bit_index in range(universe):
        bit = 1 << bit_index
        for mask in range(size):
            if mask & bit:
                pair_unions[mask] -= pair_unions[mask ^ bit]
    missing = next(
        (mask for mask, count in enumerate(pair_unions) if count and not present[mask]),
        None,
    )
    require(missing is None, f"missing pairwise union {missing}")


def bit_frequencies(family: list[int], universe: int) -> list[int]:
    return [
        sum(bool(member & (1 << element)) for member in family)
        for element in range(universe)
    ]


def verify(document: dict[str, Any]) -> dict[str, Any]:
    require(document.get("schema") == SCHEMA, "unsupported certificate schema")
    universe = document["universe_size"]
    require(isinstance(universe, int) and 1 <= universe <= 20, "invalid universe size")
    full_set = (1 << universe) - 1
    require(document["full_set_mask"] == full_set, "full-set mask does not match universe")

    generators = document["generators"]
    family = document["family"]
    require(isinstance(generators, list) and bool(generators), "missing generator basis")
    require(isinstance(family, list) and bool(family), "missing family")
    require(
        all(type(value) is int and 0 <= value <= full_set for value in generators),
        "generator outside the claimed universe",
    )
    require(
        all(type(value) is int and 0 <= value <= full_set for value in family),
        "family member outside the claimed universe",
    )
    require(family == sorted(set(family)), "family must be sorted and duplicate-free")
    require(closure(generators) == family, "generator closure does not match family")
    for dropped in range(len(generators)):
        smaller = generators[:dropped] + generators[dropped + 1 :]
        require(closure(smaller) != family, "generator basis is not irredundant")

    require_union_closed(family, universe)

    union = 0
    for member in family:
        union |= member
    require(union == full_set, "effective ground set differs from claimed universe")

    frequencies = bit_frequencies(family, universe)
    maximum = max(frequencies)
    frankl_holds = maximum * 2 >= len(family)
    counterexample = not frankl_holds
    separating = all(
        any(bool(member & (1 << left)) != bool(member & (1 << right)) for member in family)
        for left in range(universe)
        for right in range(left + 1, universe)
    )

    require(document["member_sets"] == len(family), "member count is forged")
    require(document["frequencies"] == frequencies, "frequency vector is forged")
    require(document["max_frequency"] == maximum, "maximum frequency is forged")
    require(document["frankl_holds"] is frankl_holds, "Frankl verdict is forged")
    require(document["counterexample"] is counterexample, "counterexample verdict is forged")
    require(document["union_closed"] is True, "RAD did not report a union-closed family")
    require(document["covers_universe"] is True, "RAD did not cover the universe")
    require(document["basis_irredundant"] is True, "RAD did not report an irredundant basis")
    require(document["separating"] is separating, "separation claim is forged")
    require(document["live_world_unchanged"] is True, "speculative search mutated the live world")

    closest = document.get("closest_positive_family")
    closest_verified = False
    if closest is not None:
        closest_generators = document["closest_positive_generators"]
        require(closest == sorted(set(closest)), "closest positive family is not canonical")
        require(closure(closest_generators) == closest, "closest positive basis mismatch")
        require_union_closed(closest, universe)
        closest_frequencies = bit_frequencies(closest, universe)
        closest_maximum = max(closest_frequencies)
        closest_margin = 2 * closest_maximum - len(closest)
        require(closest_margin > 0, "closest-positive family is not strictly positive")
        require(
            closest_margin == document["minimum_positive_margin"],
            "closest-positive margin mismatch",
        )
        require(
            closest_frequencies == document["closest_positive_frequencies"],
            "closest-positive frequencies mismatch",
        )
        require(len(closest) == document["closest_positive_members"], "closest size mismatch")
        closest_members = set(closest)
        deleted = [mask for mask in range(1 << universe) if mask not in closest_members]
        deleted_surpluses = [
            2 * count - len(deleted) for count in bit_frequencies(deleted, universe)
        ]
        require(
            deleted_surpluses == document["closest_positive_deleted_surpluses"],
            "closest-positive dual surplus mismatch",
        )
        require(min(deleted_surpluses) == -closest_margin, "Frankl/dual margin identity failed")
        closest_verified = True

    if "candidates_evaluated" in document:
        require(
            document.get("negative_candidates", 0)
            + document.get("equality_candidates", 0)
            + document.get("positive_candidates", 0)
            == document["candidates_evaluated"],
            "landscape classification does not cover every candidate",
        )
    digest = document["result_world_digest"]
    require(isinstance(digest, str) and len(digest) == 64, "invalid RAD world digest")
    try:
        int(digest, 16)
    except ValueError as error:
        raise VerificationError("RAD world digest is not hexadecimal") from error

    canonical = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    return {
        "schema": "rad.frankl-independent-verification.v1",
        "certificate_sha256": hashlib.sha256(canonical).hexdigest(),
        "universe_size": universe,
        "member_sets": len(family),
        "frequencies": frequencies,
        "max_frequency": maximum,
        "frankl_holds": frankl_holds,
        "counterexample": counterexample,
        "union_closed": True,
        "generator_closure_verified": True,
        "basis_irredundant": True,
        "separating": separating,
        "reported_world_digest": digest,
        "closest_positive_verified": closest_verified,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()

    document = json.loads(args.certificate.read_text(encoding="utf-8"))
    report = verify(document)
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(rendered, encoding="utf-8")
    print(rendered, end="")


if __name__ == "__main__":
    main()
