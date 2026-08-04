"""Independent witness verifier for the eight-generator graph frontier."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from generator8_graph_frontier import EDGE_PAIRS, evaluate


def connected(edge_mask: int) -> bool:
    reached = 1
    previous = -1
    while reached != previous:
        previous = reached
        for index, (left, right) in enumerate(EDGE_PAIRS):
            if edge_mask >> index & 1 and reached & ((1 << left) | (1 << right)):
                reached |= (1 << left) | (1 << right)
    return reached == 255


def verify_witness(witness: dict[str, object]) -> dict[str, object]:
    loop_mask = int(witness["loop_mask"])
    edge_mask = int(witness["edge_mask"])
    if not connected(edge_mask):
        raise ValueError("witness graph is disconnected")
    family_size, frequencies, margin = evaluate(loop_mask, edge_mask)
    if family_size != int(witness["family_size"]):
        raise ValueError("witness family size does not regenerate")
    if frequencies != [int(value) for value in witness["frequencies"]]:
        raise ValueError("witness frequencies do not regenerate")
    if margin != int(witness["margin"]):
        raise ValueError("witness margin does not regenerate")
    if loop_mask.bit_count() + edge_mask.bit_count() != int(witness["column_count"]):
        raise ValueError("witness column count is inconsistent")
    return {"family_size": family_size, "margin": margin}


def verify(document: dict[str, object]) -> dict[str, object]:
    if document.get("schema") != "rad.boolean-lattice.coloured-graph-quotient.v1":
        raise ValueError("unexpected graph-frontier schema")
    expected = {
        "generator_count": 8,
        "minimum_column_count": 13,
        "maximum_column_count": 36,
        "maximum_column_weight": 2,
        "minimum_family_size": 51,
        "maximum_family_size": 127,
        "coloured_graph_orbits": 2_038_236,
        "scanned_orbits": 1_992_040,
        "frontier_orbits": 12_431,
        "smallest_family_size": 80,
        "minimum_margin": 20,
    }
    for key, value in expected.items():
        if int(document[key]) != value:
            raise ValueError(f"{key} mismatch")
    if document["counterexample"] is not None:
        raise ValueError("certificate contains a graph counterexample")
    best = verify_witness(document["best"])
    smallest = verify_witness(document["smallest_family"])
    if best["margin"] != expected["minimum_margin"]:
        raise ValueError("minimum-margin witness is inconsistent")
    if smallest["family_size"] != expected["smallest_family_size"]:
        raise ValueError("minimum-size witness is inconsistent")
    signature = str(document["signature"])
    if len(signature) != 64:
        raise ValueError("scan signature is malformed")
    return {
        "schema": "rad.boolean-lattice.generator8-graph-verification.v1",
        "coloured_graph_orbits": expected["coloured_graph_orbits"],
        "scanned_orbits": expected["scanned_orbits"],
        "frontier_orbits": expected["frontier_orbits"],
        "smallest_family_size": smallest["family_size"],
        "minimum_margin": best["margin"],
        "signature": signature,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    args = parser.parse_args()
    report = verify(json.loads(args.certificate.read_text(encoding="utf-8")))
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
