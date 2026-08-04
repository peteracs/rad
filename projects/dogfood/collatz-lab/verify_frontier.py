#!/usr/bin/env python3
"""Independently verify exact witnesses emitted by frontier_probe.rad.

The beam is intentionally heuristic, so this verifier never upgrades absence
from a retained frontier into a proof.  It checks every positive statement:
binary support, prefix-slope survival, coefficient stop, first descent, and
eventual convergence for the selected unknown-layer witness.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


KNOWN_DEATH_BOUNDARIES = [1, 2, 4, 7, 59, 137, 214, 365, 552, 634, 818]


def read_first_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8-sig") as handle:
        line = handle.readline()
    value = json.loads(line)
    if not isinstance(value, dict):
        raise ValueError("frontier report root must be an object")
    return value


def bit_positions(value: int) -> list[int]:
    return [bit for bit in range(value.bit_length()) if (value >> bit) & 1]


def verify_prefix_slope(value: int, depth: int) -> tuple[int, int]:
    coefficient = 1
    denominator = 1
    current = value
    odd_steps = 0
    for step in range(1, depth + 1):
        if current & 1:
            current = (3 * current + 1) // 2
            coefficient *= 3
            odd_steps += 1
        else:
            current //= 2
        denominator *= 2
        if coefficient < denominator:
            raise AssertionError(f"coefficient contracted at step {step}, before claimed depth {depth}")
    return odd_steps, current


def trace_after_boundary(value: int, start_step: int, limit: int = 200_000) -> dict[str, int]:
    coefficient = 1
    denominator = 1
    current = value
    peak = value
    coefficient_stop = 0
    first_descent = 0
    reached_one = 0
    odd_steps = 0
    for step in range(1, limit + 1):
        if current & 1:
            current = (3 * current + 1) // 2
            coefficient *= 3
            odd_steps += 1
        else:
            current //= 2
        denominator *= 2
        peak = max(peak, current)
        if coefficient_stop == 0 and coefficient < denominator:
            coefficient_stop = step
        if first_descent == 0 and current < value:
            first_descent = step
        if current == 1:
            reached_one = step
            break
    if coefficient_stop <= start_step:
        raise AssertionError("reported survivor did not survive its full claimed slope depth")
    if first_descent == 0 or reached_one == 0:
        raise AssertionError("selected witness did not descend and converge inside verifier limit")
    return {
        "coefficient_stop": coefficient_stop,
        "first_descent": first_descent,
        "steps_to_one": reached_one,
        "odd_steps_to_one": odd_steps,
        "peak_bits": peak.bit_length(),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("report", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = read_first_json(args.report)

    assert report["schema"] == "rad.affine-frontier-study.v1"
    assert report["certificate"] is False
    assert report["speculative_worlds_left_authoritative_state_unchanged"] is True
    assert report["known_exact_boundaries_reconstructed"] == len(KNOWN_DEATH_BOUNDARIES)

    selected_plan = report["best_plan"]
    selected_universe = next(item for item in report["universes"] if item["plan"] == selected_plan)
    curve = selected_universe["record_curve"]
    for index, boundary in enumerate(KNOWN_DEATH_BOUNDARIES):
        record = curve[index]
        assert record["minimum_input_ones"] == index + 1
        assert record["depth"] == boundary

    checked_records = 0
    selected_unknown: dict[str, Any] | None = None
    for universe in report["universes"]:
        for record in universe["deepest_retained_by_support"]:
            value = int(record["witness"])
            positions = bit_positions(value)
            assert positions == record["one_positions"]
            assert len(positions) == record["input_ones"]
            odd_steps, _terminal = verify_prefix_slope(value, record["depth"])
            assert odd_steps == record["odd_steps"]
            checked_records += 1
            if universe["plan"] == selected_plan and record["input_ones"] == 11:
                selected_unknown = record

    if selected_unknown is None:
        raise AssertionError("selected frontier has no support-11 witness")
    unknown_value = int(selected_unknown["witness"])
    trajectory = trace_after_boundary(unknown_value, selected_unknown["depth"])
    assert trajectory["coefficient_stop"] == selected_unknown["depth"] + 1
    assert trajectory["first_descent"] == selected_unknown["depth"] + 1

    result = {
        "schema": "rad.affine-frontier-independent-verification.v1",
        "source_report_sha256": hashlib.sha256(args.report.read_bytes()).hexdigest(),
        "checked_exact_witness_records": checked_records,
        "selected_plan": selected_plan,
        "selected_support": selected_unknown["input_ones"],
        "selected_survival_depth": selected_unknown["depth"],
        "selected_witness_sha256": hashlib.sha256(str(unknown_value).encode()).hexdigest(),
        "selected_one_positions": selected_unknown["one_positions"],
        "certified_conclusion": (
            "an exact support-11 natural-number witness remains prefix-noncontracting "
            f"through step {selected_unknown['depth']}; this is a lower bound, not exhaustion"
        ),
        "trajectory": trajectory,
    }
    encoded = json.dumps(result, sort_keys=True, separators=(",", ":"))
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)


if __name__ == "__main__":
    main()
