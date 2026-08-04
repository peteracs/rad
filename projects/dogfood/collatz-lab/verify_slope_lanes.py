#!/usr/bin/env python3
"""Independently merge and verify exact sparse prefix-slope lane reports."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path


def shortcut_step(value: int) -> tuple[int, bool]:
    odd = value & 1 == 1
    return ((3 * value + 1) // 2 if odd else value // 2), odd


def verify_witness(value: int, support: int, depth: int) -> None:
    assert value > 0 or support == 0
    assert value.bit_count() == support
    coefficient = 1
    denominator = 1
    current = value
    for _ in range(depth):
        current, odd = shortcut_step(current)
        if odd:
            coefficient *= 3
        denominator *= 2
        assert coefficient >= denominator


def first_json_line(path: Path) -> dict:
    with path.open("r", encoding="utf-8-sig") as handle:
        line = handle.readline()
    return json.loads(line)


def main() -> int:
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: verify_slope_lanes.py DIRECTORY SUPPORT DEPTH LANE_COUNT"
        )
    directory = Path(sys.argv[1])
    support = int(sys.argv[2])
    depth = int(sys.argv[3])
    lane_count = int(sys.argv[4])
    reports = [first_json_line(directory / f"lane-{lane:04d}.json") for lane in range(lane_count)]

    seed_counts = {report["seed_count"] for report in reports}
    assert len(seed_counts) == 1
    seed_count = seed_counts.pop()
    deepest = [0] * (support + 1)
    witnesses = [0] * (support + 1)
    positions = [[] for _ in range(support + 1)]
    anchors = [0] * (support + 1)
    assigned = 0
    expanded = 0

    for lane, report in enumerate(reports):
        assert report["lane_index"] == lane
        assert report["lane_count"] == lane_count
        report_depth = report["max_depth"]
        assert report["max_input_ones"] == support
        expected_assigned = 0 if lane >= seed_count else 1 + (seed_count - 1 - lane) // lane_count
        assert report["assigned_seed_count"] == expected_assigned
        assert report["deepest_survival_by_weight"][support] < report_depth
        assert report["deepest_survival_by_weight"][support] < depth
        assigned += expected_assigned
        expanded += report["expanded_nodes"]
        for weight in range(support + 1):
            anchors[weight] += report["anchors_by_weight"][weight]
            lane_depth = report["deepest_survival_by_weight"][weight]
            lane_witness = int(report["deepest_witness_by_weight"][weight])
            lane_positions = report["deepest_witness_one_positions_by_weight"][weight]
            assert lane_positions == [bit for bit in range(lane_witness.bit_length()) if lane_witness >> bit & 1]
            if lane_depth > deepest[weight] or (
                lane_depth == deepest[weight] and lane_witness < witnesses[weight]
            ):
                deepest[weight] = lane_depth
                witnesses[weight] = lane_witness
                positions[weight] = lane_positions

    assert assigned == seed_count
    for weight in range(1, support + 1):
        verify_witness(witnesses[weight], weight, deepest[weight])
    assert deepest[support] < depth

    canonical_reports = json.dumps(reports, sort_keys=True, separators=(",", ":")).encode()
    certificate = {
        "schema": "rad.affine-slope-lane-certificate.v1",
        "certificate": True,
        "criterion": "prefix_slope",
        "multiplier": 3,
        "addend": 1,
        "max_support": support,
        "max_depth": depth,
        "lane_count": lane_count,
        "seed_count": seed_count,
        "assigned_seed_count": assigned,
        "deepest_survival_by_weight": deepest,
        "termination_depth_by_weight": [value + 1 for value in deepest],
        "deepest_witness_by_weight": [str(value) for value in witnesses],
        "deepest_witness_one_positions_by_weight": positions,
        "anchors_by_weight": anchors,
        "expanded_nodes": expanded,
        "lane_manifest_sha256": hashlib.sha256(canonical_reports).hexdigest(),
    }
    target = directory / "certificate.json"
    target.write_text(json.dumps(certificate, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(certificate, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
