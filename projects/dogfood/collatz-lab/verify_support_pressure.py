#!/usr/bin/env python3
"""Independent verifier for RAD's bounded-input-support certificate.

This script imports neither RAD nor its native extension.  It reimplements the
exact affine-cylinder arithmetic with Python integers, exhaustively checks the
reported upper bounds through support seven, and checks the terminal witness
boundary for every reported support weight.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SCHEMA = "rad.affine-sparse-support-certificate.v1"
INDEPENDENT_EXACT_WEIGHT = 7


class VerificationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


@dataclass(frozen=True)
class Node:
    residue: int
    coefficient: int
    offset: int
    denominator: int
    probe: int
    weight: int


def extend(node: Node, bit: int) -> Node:
    """Extend one low-input-bit cylinder for T(n)=n/2 or (3n+1)/2."""

    residue = node.residue + bit * node.denominator
    source = node.probe + bit * node.coefficient
    if source & 1:
        return Node(
            residue,
            3 * node.coefficient,
            3 * node.offset + node.denominator,
            2 * node.denominator,
            (3 * source + 1) // 2,
            node.weight + bit,
        )
    return Node(
        residue,
        node.coefficient,
        node.offset,
        2 * node.denominator,
        source // 2,
        node.weight + bit,
    )


def prunable(node: Node, verified_bound: int) -> bool:
    """Whether every n >= bound in this cylinder has an earlier descent."""

    return (
        node.coefficient < node.denominator
        and node.offset
        < verified_bound * (node.denominator - node.coefficient)
    )


def exact_support_profile(
    max_depth: int, verified_power: int, max_weight: int
) -> tuple[list[int], list[int], list[int]]:
    """Exhaust the proof-anchor tree up to max_weight, independently."""

    bound = 1 << verified_power
    deepest = [0] * (max_weight + 1)
    witnesses = [0] * (max_weight + 1)
    anchors = [0] * (max_weight + 1)

    def record(node: Node, depth: int) -> None:
        weight = node.weight
        if depth > deepest[weight] or (
            depth == deepest[weight] and node.residue < witnesses[weight]
        ):
            deepest[weight] = depth
            witnesses[weight] = node.residue

    def explore(anchor: Node, depth: int) -> None:
        anchors[anchor.weight] += 1
        record(anchor, depth)
        zero_parent = anchor
        for next_depth in range(depth + 1, max_depth + 1):
            if zero_parent.weight < max_weight:
                one_child = extend(zero_parent, 1)
                if not prunable(one_child, bound):
                    explore(one_child, next_depth)
            zero_child = extend(zero_parent, 0)
            if prunable(zero_child, bound):
                break
            record(zero_child, next_depth)
            zero_parent = zero_child

    explore(Node(0, 1, 0, 1, 0, 0), 0)
    return deepest, witnesses, anchors


def verify_witness(residue: int, weight: int, deepest: int, bound: int) -> None:
    require(residue.bit_count() == weight, f"weight-{weight} witness has wrong support")
    node = Node(0, 1, 0, 1, 0, 0)
    for bit_index in range(deepest):
        node = extend(node, (residue >> bit_index) & 1)
        require(not prunable(node, bound), f"weight-{weight} witness prunes too early")
    require(node.residue == residue, f"weight-{weight} witness has high bits beyond its record")
    require(
        prunable(extend(node, 0), bound),
        f"weight-{weight} witness zero tail does not die at the reported boundary",
    )


def verify(document: dict[str, Any]) -> dict[str, Any]:
    before = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    certificate_sha256 = hashlib.sha256(before).hexdigest()
    require(document.get("schema") == SCHEMA, "unsupported certificate schema")
    require(document.get("criterion") == "descent_threshold", "wrong proof criterion")
    require(document.get("proposal_order_independent") is True, "proposal order leaked")
    require(
        document.get("speculative_worlds_left_live_state_unchanged") is True,
        "speculation mutated the live world",
    )
    floor = document.get("verified_convergence_floor", "")
    require(isinstance(floor, str) and floor.startswith("2^"), "invalid verified floor")
    verified_power = int(floor[2:])
    bound = 1 << verified_power
    maximum_weight = document["maximum_input_ones_excluded"]
    termination = document["termination_depth_by_budget"]
    deepest = document["deepest_survival_by_weight"]
    anchors = document["anchors_by_weight"]
    witnesses = {
        int(weight): int(value)
        for weight, value in document["deepest_witness_by_weight"].items()
    }
    positions = {
        int(weight): value
        for weight, value in document[
            "deepest_witness_one_positions_by_weight"
        ].items()
    }
    maximum_positions = document["maximum_witness_bit_positions"]
    deadline_slacks = document["renewal_deadline_slacks"]
    require(maximum_weight + 1 == len(termination), "termination profile shape mismatch")
    require(len(deepest) == len(termination), "deepest profile shape mismatch")
    require(len(anchors) == len(termination), "anchor profile shape mismatch")
    require(set(witnesses) == set(range(maximum_weight + 1)), "witness keys are incomplete")
    require(set(positions) == set(witnesses), "witness-position keys are incomplete")
    require(len(maximum_positions) == maximum_weight + 1, "maximum-position shape mismatch")
    require(len(deadline_slacks) == maximum_weight + 1, "deadline-slack shape mismatch")
    require(
        document["minimum_required_input_ones"] == maximum_weight + 1,
        "minimum support conclusion is inconsistent",
    )
    running_deepest = 0
    for weight in range(maximum_weight + 1):
        running_deepest = max(running_deepest, deepest[weight])
        require(termination[weight] == running_deepest + 1, "termination/deepest mismatch")
        require(
            termination[weight] < 1 << (weight + 3),
            f"weight-{weight} violates the observed exponential envelope",
        )
        require(anchors[weight] > 0, f"weight-{weight} anchor set is empty")
        require(
            positions[weight]
            == [bit for bit in range(witnesses[weight].bit_length()) if witnesses[weight] >> bit & 1],
            f"weight-{weight} witness positions disagree",
        )
        expected_maximum = positions[weight][-1] if positions[weight] else -1
        require(maximum_positions[weight] == expected_maximum, "maximum bit position mismatch")
        if weight:
            require(
                maximum_positions[weight] < termination[weight - 1],
                f"weight-{weight} witness missed its renewal deadline",
            )
            require(
                deadline_slacks[weight]
                == termination[weight - 1] - maximum_positions[weight],
                f"weight-{weight} renewal slack mismatch",
            )
        if weight >= 6:
            require(
                termination[weight] < 2 * termination[weight - 1],
                f"weight-{weight} violates the observed renewal recurrence",
            )
        verify_witness(witnesses[weight], weight, deepest[weight], bound)

    exact_weight = min(INDEPENDENT_EXACT_WEIGHT, maximum_weight)
    exact_depth = termination[exact_weight]
    exact_deepest, exact_witnesses, exact_anchors = exact_support_profile(
        exact_depth, verified_power, exact_weight
    )
    require(
        exact_deepest == deepest[: exact_weight + 1],
        "independent deepest-support profile disagrees",
    )
    require(
        exact_witnesses == [witnesses[w] for w in range(exact_weight + 1)],
        "independent witness profile disagrees",
    )
    require(
        exact_anchors == anchors[: exact_weight + 1],
        "independent anchor profile disagrees",
    )

    after = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    require(after == before, "verifier mutated its input")
    return {
        "schema": "rad.affine-sparse-support-independent-verification.v1",
        "certificate_sha256": certificate_sha256,
        "independently_exhausted_through_weight": exact_weight,
        "independently_exhausted_through_depth": exact_depth,
        "all_reported_terminal_witnesses_checked": True,
        "excluded_input_support_budgets": list(range(maximum_weight + 1)),
        "minimum_counterexample_input_ones": maximum_weight + 1,
        "observed_exponential_envelope_checked": True,
        "renewal_deadlines_checked": True,
        "observed_renewal_recurrence_checked": True,
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
