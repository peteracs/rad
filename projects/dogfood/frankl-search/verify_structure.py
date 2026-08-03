#!/usr/bin/env python3
"""Independent verifier for cyclic-universe Frankl certificates."""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
from pathlib import Path
from typing import Any


class VerificationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


def rotate(mask: int, width: int) -> int:
    full = (1 << width) - 1
    return ((mask << 1) & full) | (mask >> (width - 1))


def orbit(mask: int, width: int) -> tuple[int, ...]:
    members: set[int] = set()
    for _ in range(width):
        members.add(mask)
        mask = rotate(mask, width)
    return tuple(sorted(members))


def representatives(width: int) -> list[int]:
    full = (1 << width) - 1
    return [mask for mask in range(1, full) if min(orbit(mask, width)) == mask]


def closure(generators: tuple[int, ...] | list[int]) -> list[int]:
    family = [0]
    present = {0}
    for generator in sorted(set(generators), key=lambda value: (value.bit_count(), value)):
        if generator in present:
            continue
        before = len(family)
        for index in range(before):
            joined = family[index] | generator
            if joined not in present:
                present.add(joined)
                family.append(joined)
    return sorted(family)


def frequencies(family: list[int], width: int) -> list[int]:
    return [sum((member >> bit) & 1 for member in family) for bit in range(width)]


def or_violation_count(family: list[int], width: int) -> int:
    size = 1 << width
    present = [False] * size
    subset_counts = [0] * size
    for member in family:
        require(0 <= member < size, f"mask {member} is outside width {width}")
        require(not present[member], f"duplicate mask {member}")
        present[member] = True
        subset_counts[member] = 1
    for bit in range(width):
        step = 1 << bit
        for mask in range(size):
            if mask & step:
                subset_counts[mask] += subset_counts[mask ^ step]
    subset_counts = [count * count for count in subset_counts]
    for bit in range(width):
        step = 1 << bit
        for mask in range(size):
            if mask & step:
                subset_counts[mask] -= subset_counts[mask ^ step]
    return sum(count for mask, count in enumerate(subset_counts) if not present[mask])


def high_rank_family(width: int, minimum_rank: int) -> list[int]:
    return [mask for mask in range(1 << width) if mask.bit_count() >= minimum_rank]


def complement(family: list[int], width: int) -> list[int]:
    present = set(family)
    return [mask for mask in range(1 << width) if mask not in present]


def deletion_surpluses(deleted: list[int], width: int) -> list[int]:
    return [2 * count - len(deleted) for count in frequencies(deleted, width)]


def enumerate_lane(payload: tuple[int, list[int], int, int]) -> dict[str, Any]:
    width, reps, lane_index, lane_count = payload
    orbit_cache = {rep: orbit(rep, width) for rep in reps}
    counts = {
        "negative": 0,
        "equality": 0,
        "positive": 0,
        "diagonal_equality": 0,
        "off_diagonal_equality": 0,
        "full_cube": 0,
        "equality_non_full": 0,
    }
    evaluated = 0
    best_frequency = 0
    best_size = 0
    for left_index in range(lane_index, len(reps), lane_count):
        left = reps[left_index]
        left_orbit = orbit_cache[left]
        for right in reps[left_index:]:
            family = closure(left_orbit + orbit_cache[right])
            frequency = sum(member & 1 != 0 for member in family)
            margin = 2 * frequency - len(family)
            counts["negative" if margin < 0 else "equality" if margin == 0 else "positive"] += 1
            if margin == 0:
                counts["diagonal_equality" if left == right else "off_diagonal_equality"] += 1
                counts["full_cube" if len(family) == 1 << width else "equality_non_full"] += 1
            evaluated += 1
            if best_size == 0 or frequency * best_size < best_frequency * len(family) or (
                frequency * best_size == best_frequency * len(family) and len(family) > best_size
            ):
                best_frequency = frequency
                best_size = len(family)
    return {
        "counts": counts,
        "evaluated": evaluated,
        "best_frequency": best_frequency,
        "best_size": best_size,
    }


def verify_certificate(
    data: dict[str, Any], full_enumeration: bool = True, jobs: int = 1
) -> dict[str, Any]:
    width = data.get("universe_size")
    require(width == 13, "this certificate profile requires width 13")
    reps = representatives(width)
    require(len(reps) == 630, "incorrect binary-necklace representative count")
    require(data.get("necklace_representatives") == len(reps), "representative claim mismatch")
    expected = len(reps) * (len(reps) + 1) // 2
    require(data.get("families_evaluated") == expected, "pair-class cardinality mismatch")

    best_generators = data.get("best_generators")
    best_family = data.get("best_family")
    require(isinstance(best_generators, list), "best_generators must be a list")
    require(isinstance(best_family, list), "best_family must be a list")
    require(closure(best_generators) == best_family, "best generators do not regenerate family")
    best_frequencies = frequencies(best_family, width)
    require(best_frequencies == data.get("best_frequencies"), "best frequencies mismatch")
    require(len(best_family) == data.get("best_members"), "best member count mismatch")
    maximum = max(best_frequencies)
    require(maximum == data.get("best_max_frequency"), "best maximum mismatch")
    require(2 * maximum - len(best_family) == data.get("best_margin"), "best margin mismatch")
    require(or_violation_count(best_family, width) == 0, "best family is not union-closed")

    deleted = complement(best_family, width)
    require(len(deleted) == data.get("best_deleted_size"), "best deleted size mismatch")
    require(
        deletion_surpluses(deleted, width) == data.get("best_deleted_majority_surpluses"),
        "best dual surpluses mismatch",
    )

    tempting_deleted = [member for member in high_rank_family(width, 7) if member != (1 << width) - 1]
    tempting_remaining = complement(tempting_deleted, width)
    tempting_surpluses = deletion_surpluses(tempting_deleted, width)
    tempting_violations = or_violation_count(tempting_remaining, width)
    require(min(tempting_surpluses) > 0, "tempting deletion is not element-majority")
    require(tempting_violations > 0, "tempting remainder unexpectedly union-closed")
    require(data.get("tempting_deleted_size") == len(tempting_deleted), "tempting size mismatch")
    require(
        data.get("tempting_deleted_majority_surpluses") == tempting_surpluses,
        "tempting surpluses mismatch",
    )
    require(data.get("tempting_union_violations") == tempting_violations, "violation count mismatch")
    repaired = closure(tempting_remaining)
    repaired_margin = 2 * max(frequencies(repaired, width)) - len(repaired)
    require(data.get("repaired_size") == len(repaired), "repair size mismatch")
    require(data.get("repaired_margin") == repaired_margin == 0, "repair margin mismatch")

    exact = None
    if full_enumeration:
        counts = {
            "negative": 0,
            "equality": 0,
            "positive": 0,
            "diagonal_equality": 0,
            "off_diagonal_equality": 0,
            "full_cube": 0,
            "equality_non_full": 0,
        }
        evaluated = 0
        best_frequency = 0
        best_size = 0
        jobs = max(1, min(jobs, len(reps)))
        payloads = [(width, reps, lane, jobs) for lane in range(jobs)]
        if jobs == 1:
            lane_results = [enumerate_lane(payloads[0])]
        else:
            with concurrent.futures.ProcessPoolExecutor(max_workers=jobs) as pool:
                lane_results = list(pool.map(enumerate_lane, payloads))
        for lane in lane_results:
            evaluated += lane["evaluated"]
            for key in counts:
                counts[key] += lane["counts"][key]
            frequency = lane["best_frequency"]
            size = lane["best_size"]
            if best_size == 0 or frequency * best_size < best_frequency * size or (
                frequency * best_size == best_frequency * size and size > best_size
            ):
                best_frequency = frequency
                best_size = size
        require(evaluated == expected, "independent enumeration cardinality mismatch")
        require(counts["negative"] == data.get("negative_families"), "negative count mismatch")
        require(counts["equality"] == data.get("equality_families"), "equality count mismatch")
        require(counts["positive"] == data.get("positive_families"), "positive count mismatch")
        require(
            counts["diagonal_equality"] == data.get("diagonal_equality_families"),
            "diagonal equality count mismatch",
        )
        require(
            counts["off_diagonal_equality"] == data.get("off_diagonal_equality_families"),
            "off-diagonal equality count mismatch",
        )
        require(counts["full_cube"] == data.get("full_cube_families"), "full-cube count mismatch")
        require(
            counts["equality_non_full"] == data.get("equality_non_full_families"),
            "non-full equality count mismatch",
        )
        require(best_size > 0, "empty exact search")
        require(2 * best_frequency - best_size == data.get("best_margin"), "best exact margin mismatch")
        exact = counts

    return {
        "verified": True,
        "full_enumeration": full_enumeration,
        "families": expected,
        "counts": exact,
        "best_margin": data.get("best_margin"),
        "tempting_union_violations": tempting_violations,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--quick", action="store_true", help="skip independent enumeration")
    parser.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    args = parser.parse_args()
    data = json.loads(args.certificate.read_text(encoding="utf-8"))
    print(json.dumps(verify_certificate(data, not args.quick, args.jobs), sort_keys=True))


if __name__ == "__main__":
    main()
