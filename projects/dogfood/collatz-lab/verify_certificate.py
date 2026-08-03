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
from collections import Counter
from pathlib import Path
from typing import Any, Iterator


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

    # residue, coefficient, offset, denominator, T^depth(residue), odd_steps
    frontier = [(0, 1, 0, 1, 0, 0)]
    verified_bound = 1 << verified_power
    pruned_classes = 0
    residue_sum = 0
    expanded_nodes = 0
    prune_histogram = [0] * (depth + 1)

    for next_depth in range(1, depth + 1):
        next_frontier = []
        for residue, coefficient, offset, denominator, probe, odd_steps in frontier:
            for extension in (0, 1):
                child_residue = residue + extension * denominator
                source = probe + extension * coefficient
                child_coefficient = coefficient
                child_offset = offset
                child_odd_steps = odd_steps
                if source & 1:
                    child_probe = (3 * source + 1) // 2
                    child_coefficient *= 3
                    child_offset = 3 * offset + denominator
                    child_odd_steps += 1
                else:
                    child_probe = source // 2
                child_denominator = denominator * 2
                expanded_nodes += 1

                prunable = False
                if child_coefficient < child_denominator:
                    threshold = child_offset // (child_denominator - child_coefficient)
                    prunable = threshold < verified_bound
                if prunable:
                    represented = 1 << (depth - next_depth)
                    pruned_classes += represented
                    residue_sum += (
                        represented * child_residue
                        + (1 << next_depth) * represented * (represented - 1) // 2
                    )
                    prune_histogram[next_depth] += represented
                else:
                    next_frontier.append(
                        (
                            child_residue,
                            child_coefficient,
                            child_offset,
                            child_denominator,
                            child_probe,
                            child_odd_steps,
                        )
                    )
        frontier = next_frontier

    survivor_histogram = Counter(node[5] for node in frontier)
    survivors = len(frontier)
    residue_sum += sum(node[0] for node in frontier)
    contracting = sum(node[1] < node[3] for node in frontier)
    max_odd_steps = max(node[5] for node in frontier)
    max_odd_residue = min(node[0] for node in frontier if node[5] == max_odd_steps)
    return {
        "classes": 1 << depth,
        "residue_sum": residue_sum,
        "pruned_classes": pruned_classes,
        "survivor_classes": survivors,
        "contracting_survivors": contracting,
        "noncontracting_survivors": survivors - contracting,
        "expanded_nodes": expanded_nodes,
        "prune_histogram": prune_histogram,
        "survivor_odd_histogram": [survivor_histogram[i] for i in range(depth + 1)],
        "max_odd_steps": max_odd_steps,
        "max_odd_residue": max_odd_residue,
    }


def compositions(total: int, slots: int, prefix: tuple[int, ...] = ()) -> Iterator[tuple[int, ...]]:
    if slots == 1:
        yield prefix + (total,)
        return
    for value in range(1, total - slots + 2):
        yield from compositions(total - value, slots - 1, prefix + (value,))


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


def cycle_box(max_odd_steps: int, max_total_divisions: int) -> dict[str, int]:
    words = positive = divisible = exact = nontrivial = trivial = 0
    closest_gap: int | None = None
    closest_q = closest_divisions = 0
    for q in range(1, min(max_odd_steps, max_total_divisions) + 1):
        for total in range(q, max_total_divisions + 1):
            gap = (1 << total) - 3**q
            for word in compositions(total, q):
                words += 1
                denominator_positive, closes, start = cycle_word(word)
                if denominator_positive:
                    positive += 1
                    if q > 1 and (closest_gap is None or gap < closest_gap):
                        closest_gap = gap
                        closest_q = q
                        closest_divisions = total
                if denominator_positive:
                    # Recompute divisibility without relying on closes.
                    numerator = 0
                    prefix = 0
                    for valuation in word:
                        numerator = 3 * numerator + (1 << prefix)
                        prefix += valuation
                    if numerator % ((1 << prefix) - 3**q) == 0:
                        divisible += 1
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
