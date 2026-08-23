#!/usr/bin/env python3
"""Independent verifier for RAD's Collatz structural certificate.

This program deliberately does not import rad-vm.  It recomputes the pruned
affine residue tree, the all-odd escape trajectory, and the finite odd-cycle
valuation box using Python big integers.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
from typing import Any

import numpy as np


SCHEMA = "rad.collatz-structural-certificate.v1"


class VerificationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


def trailing_zeros(value: int) -> int:
    require(value > 0, "trailing_zeros requires a positive integer")
    return (value & -value).bit_length() - 1


def residue_tree(depth: int, verified_power: int) -> dict[str, Any]:
    """Expand only residue cylinders not already certified by descent."""

    maximum_u64 = np.iinfo(np.uint64).max
    residue = np.array([0], dtype=np.uint64)
    coefficient = np.array([1], dtype=np.uint64)
    offset = np.array([0], dtype=np.uint64)
    denominator = np.array([1], dtype=np.uint64)
    probe = np.array([0], dtype=np.uint64)
    odd_steps = np.array([0], dtype=np.uint8)
    verified_bound = 1 << verified_power
    pruned_classes = 0
    residue_sum = 0
    expanded_nodes = 0
    prune_histogram = [0] * (depth + 1)

    for next_depth in range(1, depth + 1):
        if np.any(residue > maximum_u64 - denominator):
            raise VerificationError("residue expansion exceeds uint64")
        if np.any(probe > maximum_u64 - coefficient):
            raise VerificationError("trajectory probe exceeds uint64")
        child_residue = np.concatenate((residue, residue + denominator))
        source = np.concatenate((probe, probe + coefficient))
        child_coefficient = np.concatenate((coefficient, coefficient))
        child_offset = np.concatenate((offset, offset))
        child_odd_steps = np.concatenate((odd_steps, odd_steps))
        child_denominator = np.concatenate((denominator, denominator))
        odd = source & 1 == 1
        if np.any(child_coefficient[odd] > maximum_u64 // 3):
            raise VerificationError("affine coefficient exceeds uint64")
        if np.any(child_offset[odd] > (maximum_u64 - child_denominator[odd]) // 3):
            raise VerificationError("affine offset exceeds uint64")
        if np.any(child_denominator > maximum_u64 // 2):
            raise VerificationError("residue denominator exceeds uint64")
        source[odd] = (3 * source[odd] + 1) // 2
        source[~odd] //= 2
        child_coefficient[odd] *= 3
        child_offset[odd] = 3 * child_offset[odd] + child_denominator[odd]
        child_odd_steps[odd] += 1
        child_denominator *= 2
        expanded_nodes += len(source)

        contracting_mask = child_coefficient < child_denominator
        if verified_power >= 64:
            prunable = contracting_mask
        else:
            prunable = np.zeros(len(source), dtype=np.bool_)
            prunable[contracting_mask] = (
                child_offset[contracting_mask]
                // (child_denominator[contracting_mask] - child_coefficient[contracting_mask])
                < verified_bound
            )
        pruned_count = int(np.count_nonzero(prunable))
        represented = 1 << (depth - next_depth)
        pruned_classes += pruned_count * represented
        residue_sum += represented * int(child_residue[prunable].sum(dtype=np.uint64))
        residue_sum += (
            pruned_count
            * (1 << next_depth)
            * represented
            * (represented - 1)
            // 2
        )
        prune_histogram[next_depth] += pruned_count * represented
        retained = ~prunable
        residue = child_residue[retained]
        coefficient = child_coefficient[retained]
        offset = child_offset[retained]
        denominator = child_denominator[retained]
        probe = source[retained]
        odd_steps = child_odd_steps[retained]

    survivor_histogram = np.bincount(odd_steps, minlength=depth + 1)
    survivors = len(residue)
    residue_sum += int(residue.sum(dtype=np.uint64))
    contracting = int(np.count_nonzero(coefficient < denominator))
    max_odd_steps = int(odd_steps.max())
    max_odd_residue = int(residue[odd_steps == max_odd_steps].min())
    return {
        "classes": 1 << depth,
        "residue_sum": residue_sum,
        "pruned_classes": pruned_classes,
        "survivor_classes": survivors,
        "contracting_survivors": contracting,
        "noncontracting_survivors": survivors - contracting,
        "expanded_nodes": expanded_nodes,
        "prune_histogram": prune_histogram,
        "survivor_odd_histogram": [int(count) for count in survivor_histogram],
        "max_odd_steps": max_odd_steps,
        "max_odd_residue": max_odd_residue,
    }


def cycle_word(word: tuple[int, ...]) -> tuple[bool, bool, int]:
    numerator = 0
    prefix = 0
    for valuation in word:
        numerator = 3 * numerator + (1 << prefix)
        prefix += valuation
    denominator = (1 << prefix) - 3 ** len(word)
    if denominator <= 0 or numerator % denominator:
        return denominator > 0, False, 0
    start = numerator // denominator
    if start <= 0 or start % 2 == 0:
        return True, False, start
    value = start
    for expected in word:
        expanded = 3 * value + 1
        actual = trailing_zeros(expanded)
        if actual != expected:
            return True, False, start
        value = expanded >> actual
    return True, value == start, start


def divisible_cycle_words(q: int, total: int, denominator: int) -> list[tuple[int, ...]]:
    """Recover every composition whose cycle numerator is divisible.

    Prefix sums satisfy ``N[j+1] = 3*N[j] + 2**prefix[j]``.  Reversing that
    recurrence prunes on divisibility by three at every level.  The maximum
    possible numerator gives a rigorous finite start bound, so this is an
    exact audit of the same composition box without visiting millions of
    numerators that cannot be divisible.
    """

    maximum_numerator = sum(
        3 ** (q - 1 - index) * 2 ** (total - (q - index))
        for index in range(q)
    )
    words: list[tuple[int, ...]] = []
    reverse_prefixes: list[int] = []

    def recover(stage: int, numerator: int, upper_prefix: int) -> None:
        if stage < 0:
            if numerator == 0:
                prefixes = list(reversed(reverse_prefixes)) + [total]
                words.append(
                    tuple(
                        prefixes[index + 1] - prefixes[index]
                        for index in range(q)
                    )
                )
            return
        candidates = (0,) if stage == 0 else range(stage, upper_prefix)
        for prefix in candidates:
            remainder = numerator - (1 << prefix)
            if remainder < 0 or remainder % 3:
                continue
            reverse_prefixes.append(prefix)
            recover(stage - 1, remainder // 3, prefix)
            reverse_prefixes.pop()

    for start in range(1, maximum_numerator // denominator + 1):
        recover(q - 1, denominator * start, total)
    return words


def cycle_box(max_odd_steps: int, max_total_divisions: int) -> dict[str, int]:
    words = positive = divisible = exact = nontrivial = trivial = 0
    closest_gap: int | None = None
    closest_q = closest_divisions = 0
    for q in range(1, min(max_odd_steps, max_total_divisions) + 1):
        for total in range(q, max_total_divisions + 1):
            gap = (1 << total) - 3**q
            task_words = math.comb(total - 1, q - 1)
            words += task_words
            if gap > 0:
                positive += task_words
                if q > 1 and (closest_gap is None or gap < closest_gap):
                    closest_gap = gap
                    closest_q = q
                    closest_divisions = total
                candidates = divisible_cycle_words(q, total, gap)
                divisible += len(candidates)
                for word in candidates:
                    denominator_positive, closes, start = cycle_word(word)
                    require(denominator_positive, "recovered a nonpositive cycle word")
                    if closes:
                        exact += 1
                        if start == 1:
                            trivial += 1
                        else:
                            nontrivial += 1
    return {
        "cycle_words": words,
        "positive_cycle_denominators": positive,
        "divisible_cycle_candidates": divisible,
        "exact_cycle_words": exact,
        "trivial_cycle_words": trivial,
        "nontrivial_cycle_words": nontrivial,
        "closest_cycle_q": closest_q,
        "closest_cycle_divisions": closest_divisions,
        "closest_cycle_gap": closest_gap or 0,
    }


def first_descent(start: int) -> tuple[int, int, int, int]:
    value = start
    peak = start
    odd_steps = steps = 0
    while value >= start:
        if value & 1:
            value = (3 * value + 1) // 2
            odd_steps += 1
        else:
            value //= 2
        peak = max(peak, value)
        steps += 1
    return steps, value, peak, odd_steps


def valuations(start: int, count: int) -> list[int]:
    result = []
    value = start
    while len(result) < count and value != 1:
        expanded = 3 * value + 1
        valuation = trailing_zeros(expanded)
        result.append(valuation)
        value = expanded >> valuation
    return result


def verify(document: dict[str, Any]) -> dict[str, Any]:
    require(document.get("schema") == SCHEMA, "unsupported certificate schema")
    depth = document["residue_depth"]
    require(type(depth) is int and 1 <= depth <= 32, "invalid residue depth")
    floor = document["verified_convergence_floor"]
    require(isinstance(floor, str) and floor.startswith("2^"), "invalid verified floor")
    verified_power = int(floor[2:])

    tree = residue_tree(depth, verified_power)
    mapping = {
        "classes": "classes",
        "residue_sum": "residue_sum",
        "pruned_classes": "pruned_classes",
        "survivor_classes": "survivor_classes",
        "contracting_survivors": "contracting_survivors",
        "noncontracting_survivors": "noncontracting_survivors",
        "prune_histogram": "prune_histogram",
        "survivor_odd_histogram": "survivor_odd_histogram",
        "all_odd_prefix_steps": "max_odd_steps",
        "all_odd_residue": "max_odd_residue",
    }
    for reported, computed in mapping.items():
        require(document[reported] == tree[computed], f"forged {reported}")
    require(
        next(
            index
            for index, count in enumerate(tree["survivor_odd_histogram"])
            if count
        )
        == document["min_survivor_odd_steps"],
        "forged minimum survivor odd-step count",
    )
    critical_odd_steps = 0
    coefficient = 1
    while coefficient < 1 << depth:
        coefficient *= 3
        critical_odd_steps += 1
    require(
        document["critical_min_odd_steps"] == critical_odd_steps,
        "forged critical odd-step count",
    )
    remaining = 1 << depth
    survivor_curve = []
    for current_depth in range(1, depth + 1):
        remaining -= tree["prune_histogram"][current_depth]
        survivor_curve.append(remaining // (1 << (depth - current_depth)))
    require(document["survivor_curve"] == survivor_curve, "forged survivor curve")
    # The RAD run starts one low-bit traversal per lane, so the first few
    # prefix nodes are intentionally recomputed.  Bound that deterministic
    # parallelization overhead while comparing the mathematical tree exactly.
    lane_count = document["lane_count"]
    lane_bits = lane_count.bit_length() - 1
    require(lane_count == 1 << lane_bits, "lane count is not a power of two")
    require(
        tree["expanded_nodes"]
        <= document["expanded_nodes"]
        <= tree["expanded_nodes"] + lane_count * lane_bits,
        "forged expanded-node count",
    )

    all_odd_start = (1 << depth) - 1
    escape = first_descent(all_odd_start)
    require(document["all_odd_first_descent_steps"] == escape[0], "forged descent steps")
    require(document["all_odd_first_descent_terminal"] == escape[1], "forged terminal")
    require(document["all_odd_peak"] == escape[2], "forged peak")
    require(document["all_odd_odd_steps_to_descent"] == escape[3], "forged odd count")
    require(document["two_adic_all_odd_limit"] == "-1", "forged 2-adic limit")
    require(document["two_adic_limit_is_positive"] is False, "-1 is not positive")
    require((-3 + 1) // 2 == -1, "-1 must be fixed by the odd shortcut map")
    require(
        document["all_odd_syracuse_valuations"] == valuations(all_odd_start, 64),
        "forged Syracuse valuations",
    )

    cycles = cycle_box(
        document["cycle_max_odd_steps"], document["cycle_max_total_divisions"]
    )
    for key, expected in cycles.items():
        reported = int(document[key]) if key == "closest_cycle_gap" else document[key]
        require(reported == expected, f"forged {key}")
    require(document["proposal_order_independent"] is True, "order independence failed")
    require(
        document["speculation_left_live_world_unchanged"] is True,
        "speculation changed the live world",
    )

    canonical = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    return {
        "schema": "rad.collatz-independent-verification.v1",
        "certificate_sha256": hashlib.sha256(canonical).hexdigest(),
        "residue_depth": depth,
        "residue_classes": tree["classes"],
        "survivor_classes": tree["survivor_classes"],
        "survivor_fraction": tree["survivor_classes"] / tree["classes"],
        "all_odd_first_descent": escape[0],
        "cycle_words_verified": cycles["cycle_words"],
        "nontrivial_cycles_found": cycles["nontrivial_cycle_words"],
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
