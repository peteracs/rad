#!/usr/bin/env python3
"""Independent verifier for the natural-tail affine certificate.

This script imports neither RAD nor its native extension.  It builds the
survivor prefix tree with Python integers, then continues every surviving
residue with zero high input bits and checks the reported stopping records.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
from typing import Any
from concurrent.futures import ProcessPoolExecutor


SCHEMA = "rad.affine-natural-tail-certificate.v1"


class VerificationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


def step_node(
    node: tuple[int, int, int, int, int, int], extension: int
) -> tuple[int, int, int, int, int, int]:
    """Extend (residue, coefficient, offset, denominator, probe, peak)."""

    residue, coefficient, offset, denominator, probe, peak = node
    residue += extension * denominator
    source = probe + extension * coefficient
    if source & 1:
        next_probe = (3 * source + 1) // 2
        return (
            residue,
            3 * coefficient,
            3 * offset + denominator,
            2 * denominator,
            next_probe,
            max(peak, next_probe),
        )
    next_probe = source // 2
    return residue, coefficient, offset, 2 * denominator, next_probe, max(peak, next_probe)


def is_prunable(node: tuple[int, int, int, int, int, int], bound: int) -> bool:
    _, coefficient, offset, denominator, _, _ = node
    return coefficient < denominator and offset // (denominator - coefficient) < bound


def prefer_record(current_step: int, current_residue: int, step: int, residue: int) -> bool:
    return step > current_step or (step == current_step and residue < current_residue)


def empty_tail_report(depth: int, max_steps: int) -> dict[str, Any]:
    return {
        "depth": depth,
        "survivor_classes": 0,
        "coefficient_stops": 0,
        "descents": 0,
        "unresolved": 0,
        "max_coefficient_stop_step": 0,
        "max_coefficient_stop_residue": 0,
        "max_descent_step": 0,
        "max_descent_residue": 0,
        "max_additive_delay": 0,
        "max_peak": 0,
        "max_peak_residue": 0,
        "coefficient_stop_histogram": [0] * (max_steps + 1),
        "descent_histogram": [0] * (max_steps + 1),
    }


def add_natural_tail(
    report: dict[str, Any],
    node: tuple[int, int, int, int, int, int],
    max_steps: int,
) -> None:
    residue, coefficient, _offset, denominator, value, prefix_peak = node
    depth = report["depth"]
    require(residue > 0, "zero cannot survive the least-counterexample sieve")
    report["survivor_classes"] += 1
    peak = max(residue, prefix_peak)
    coefficient_stop = depth if coefficient < denominator else None
    descent = depth if value < residue else None
    for step in range(depth + 1, max_steps + 1):
        denominator *= 2
        if value & 1:
            value = (3 * value + 1) // 2
            coefficient *= 3
        else:
            value //= 2
        peak = max(peak, value)
        if coefficient_stop is None and coefficient < denominator:
            coefficient_stop = step
        if descent is None and value < residue:
            descent = step
        if coefficient_stop is not None and descent is not None:
            break

    if coefficient_stop is not None:
        report["coefficient_stops"] += 1
        report["coefficient_stop_histogram"][coefficient_stop] += 1
        if prefer_record(
            report["max_coefficient_stop_step"],
            report["max_coefficient_stop_residue"],
            coefficient_stop,
            residue,
        ):
            report["max_coefficient_stop_step"] = coefficient_stop
            report["max_coefficient_stop_residue"] = residue
    if descent is not None:
        report["descents"] += 1
        report["descent_histogram"][descent] += 1
        if prefer_record(
            report["max_descent_step"],
            report["max_descent_residue"],
            descent,
            residue,
        ):
            report["max_descent_step"] = descent
            report["max_descent_residue"] = residue
    if coefficient_stop is None or descent is None:
        report["unresolved"] += 1
    else:
        delay = descent - coefficient_stop
        if prefer_record(
            report["max_additive_delay"], 0, delay, residue
        ):
            report["max_additive_delay"] = delay
    if peak > report["max_peak"] or (
        peak == report["max_peak"] and residue < report["max_peak_residue"]
    ):
        report["max_peak"] = peak
        report["max_peak_residue"] = residue


def exact_lane(
    depths: list[int],
    verified_power: int,
    max_steps: int,
    lane_index: int,
    lane_count: int,
) -> list[dict[str, Any]]:
    requested = set(depths)
    reports = {depth: empty_tail_report(depth, max_steps) for depth in depths}
    bound = 1 << verified_power
    maximum_depth = max(depths)
    lane_bits = lane_count.bit_length() - 1
    node = (0, 1, 0, 1, 0, 0)
    for bit_index in range(lane_bits):
        node = step_node(node, (lane_index >> bit_index) & 1)
        if is_prunable(node, bound):
            return [reports[depth] for depth in depths]
    stack = [(lane_bits, node)]
    while stack:
        depth, node = stack.pop()
        if depth in requested:
            add_natural_tail(reports[depth], node, max_steps)
        if depth == maximum_depth:
            continue
        for extension in (1, 0):
            child = step_node(node, extension)
            if not is_prunable(child, bound):
                stack.append((depth + 1, child))
    return [reports[depth] for depth in depths]


def merge_tail_report(target: dict[str, Any], source: dict[str, Any]) -> None:
    for field in ("survivor_classes", "coefficient_stops", "descents", "unresolved"):
        target[field] += source[field]
    for target_histogram, source_histogram in (
        (target["coefficient_stop_histogram"], source["coefficient_stop_histogram"]),
        (target["descent_histogram"], source["descent_histogram"]),
    ):
        for index, count in enumerate(source_histogram):
            target_histogram[index] += count
    if prefer_record(
        target["max_coefficient_stop_step"],
        target["max_coefficient_stop_residue"],
        source["max_coefficient_stop_step"],
        source["max_coefficient_stop_residue"],
    ):
        target["max_coefficient_stop_step"] = source["max_coefficient_stop_step"]
        target["max_coefficient_stop_residue"] = source["max_coefficient_stop_residue"]
    if prefer_record(
        target["max_descent_step"],
        target["max_descent_residue"],
        source["max_descent_step"],
        source["max_descent_residue"],
    ):
        target["max_descent_step"] = source["max_descent_step"]
        target["max_descent_residue"] = source["max_descent_residue"]
    target["max_additive_delay"] = max(
        target["max_additive_delay"], source["max_additive_delay"]
    )
    if source["max_peak"] > target["max_peak"] or (
        source["max_peak"] == target["max_peak"]
        and source["max_peak_residue"] < target["max_peak_residue"]
    ):
        target["max_peak"] = source["max_peak"]
        target["max_peak_residue"] = source["max_peak_residue"]


def exact_scales(
    depths: list[int], verified_power: int, max_steps: int
) -> list[dict[str, Any]]:
    lane_count = 64 if min(depths) >= 6 else 1 << min(depths)
    workers = min(lane_count, os.cpu_count() or 1)
    arguments = [
        (depths, verified_power, max_steps, lane_index, lane_count)
        for lane_index in range(lane_count)
    ]
    with ProcessPoolExecutor(max_workers=workers) as executor:
        lane_reports = list(executor.map(_exact_lane_star, arguments))
    merged = [empty_tail_report(depth, max_steps) for depth in depths]
    for reports in lane_reports:
        for target, source in zip(merged, reports, strict=True):
            merge_tail_report(target, source)
    return merged


def _exact_lane_star(arguments: tuple[list[int], int, int, int, int]) -> list[dict[str, Any]]:
    return exact_lane(*arguments)


def critical_path_profile(depth: int) -> tuple[int, int]:
    unrestricted = [1] + [0] * depth
    meanders = [1] + [0] * depth
    for step in range(1, depth + 1):
        next_unrestricted = [0] * (depth + 1)
        next_meanders = [0] * (depth + 1)
        for odd_steps in range(step + 1):
            next_unrestricted[odd_steps] = unrestricted[odd_steps]
            count = meanders[odd_steps]
            if odd_steps:
                next_unrestricted[odd_steps] += unrestricted[odd_steps - 1]
                count += meanders[odd_steps - 1]
            if 3**odd_steps >= 1 << step:
                next_meanders[odd_steps] = count
        unrestricted, meanders = next_unrestricted, next_meanders
    terminal = sum(
        count
        for odd_steps, count in enumerate(unrestricted)
        if 3**odd_steps >= 1 << depth
    )
    return terminal, sum(meanders)


def first_descent(start: int) -> list[int]:
    value = peak = start
    steps = odd_steps = 0
    while value >= start:
        if value & 1:
            value = (3 * value + 1) // 2
            odd_steps += 1
        else:
            value //= 2
        peak = max(peak, value)
        steps += 1
    return [steps, value, peak, odd_steps]


def greedy_critical_shadow(depth: int) -> dict[str, Any]:
    residue = probe = odd_steps = 0
    coefficient = denominator = 1
    input_bits: list[int] = []
    parity_word: list[int] = []
    last_nonzero_input_bit = -1
    for step in range(1, depth + 1):
        parity = 0 if coefficient >= 1 << step else 1
        extension = (parity - probe) & 1
        residue += extension * denominator
        if extension:
            last_nonzero_input_bit = step - 1
        input_bits.append(extension)
        parity_word.append(parity)
        source = probe + extension * coefficient
        if parity:
            probe = (3 * source + 1) // 2
            coefficient *= 3
            odd_steps += 1
        else:
            probe = source // 2
        denominator *= 2
        require(coefficient >= denominator, "greedy parity word crossed its boundary")
    next_parity = 0 if coefficient >= 1 << (depth + 1) else 1
    next_forced_input_bit = (next_parity - probe) & 1
    return {
        "depth": depth,
        "residue": residue,
        "terminal": probe,
        "odd_steps": odd_steps,
        "input_bits": input_bits,
        "parity_word": parity_word,
        "last_nonzero_input_bit": last_nonzero_input_bit,
        "next_parity": next_parity,
        "next_forced_input_bit": next_forced_input_bit,
    }


def verify(document: dict[str, Any]) -> dict[str, Any]:
    canonical_input = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    certificate_sha256 = hashlib.sha256(canonical_input).hexdigest()
    require(document.get("schema") == SCHEMA, "unsupported certificate schema")
    require(document.get("zero_high_input_bits") is True, "certificate is not a natural-tail study")
    require(document.get("proposal_order_independent") is True, "producer order changed the settlement")
    require(
        document.get("speculative_worlds_left_live_state_unchanged") is True,
        "speculation changed live state",
    )
    floor = document.get("verified_convergence_floor", "")
    require(isinstance(floor, str) and floor.startswith("2^"), "invalid verified floor")
    verified_power = int(floor[2:])
    max_steps = document["max_steps"]
    reported_scales = document["scales"]
    depths = [scale["depth"] for scale in reported_scales]
    require(depths == sorted(set(depths)), "scales are not canonical and unique")
    require(all(depth < max_steps for depth in depths), "invalid natural-tail horizon")

    computed_scales = exact_scales(depths, verified_power, max_steps)
    checked_fields = [
        "depth",
        "survivor_classes",
        "coefficient_stops",
        "descents",
        "unresolved",
        "max_coefficient_stop_step",
        "max_coefficient_stop_residue",
        "max_descent_step",
        "max_descent_residue",
        "max_additive_delay",
        "max_peak",
        "max_peak_residue",
        "coefficient_stop_histogram",
        "descent_histogram",
    ]
    for reported, computed in zip(reported_scales, computed_scales, strict=True):
        for field in checked_fields:
            require(reported[field] == computed[field], f"forged depth {reported['depth']} {field}")
        require(
            len(reported["lane_signatures"]) == document["lane_count"],
            f"depth {reported['depth']} has the wrong lane-signature count",
        )
        terminal_words, meander_words = critical_path_profile(reported["depth"])
        require(
            reported["terminal_noncontracting_words"] == terminal_words,
            f"forged depth {reported['depth']} terminal word count",
        )
        require(
            reported["prefix_noncontracting_words"] == meander_words,
            f"forged depth {reported['depth']} prefix word count",
        )
        require(
            computed["survivor_classes"] == meander_words,
            f"depth {reported['depth']} residue and parity models disagree",
        )

    deepest = computed_scales[-1]
    record_trace = first_descent(deepest["max_descent_residue"])
    require(record_trace[0] == deepest["max_descent_step"], "record trace step mismatch")
    require(document["deepest_record_terminal"] == record_trace[1], "forged record terminal")
    require(document["deepest_record_odd_steps"] == record_trace[3], "forged record odd count")
    shadow_depth = document["greedy_critical_shadow"]["depth"]
    shadow = greedy_critical_shadow(shadow_depth)
    require(document["greedy_critical_shadow"] == shadow, "forged greedy critical shadow")
    require(
        document["greedy_shadow_first_descent"] == first_descent(shadow["residue"]),
        "forged greedy-shadow descent",
    )

    canonical_after = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    require(canonical_after == canonical_input, "independent verifier mutated its input certificate")
    return {
        "schema": "rad.affine-natural-tail-independent-verification.v1",
        "certificate_sha256": certificate_sha256,
        "verified_depths": depths,
        "deepest_survivors": computed_scales[-1]["survivor_classes"],
        "deepest_max_descent_step": computed_scales[-1]["max_descent_step"],
        "deepest_record_residue": computed_scales[-1]["max_descent_residue"],
        "status": "verified",
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    document = json.loads(args.certificate.read_text(encoding="utf-8"))
    report = verify(document)
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(encoded, encoding="utf-8")
    print(encoded, end="")


if __name__ == "__main__":
    main()
