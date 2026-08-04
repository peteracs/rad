"""Independent arithmetic verifier for the seven-generator frontier certificate."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path


def quotient_family(columns: list[int], generator_count: int) -> list[int]:
    return sorted(
        {
            sum(
                1 << coordinate
                for coordinate, column in enumerate(columns)
                if selected & column
            )
            for selected in range(1 << generator_count)
        }
    )


def verify(document: dict[str, object]) -> dict[str, object]:
    if document.get("schema") != "rad.boolean-lattice.generator-frontier.v2":
        raise ValueError("unexpected generator-frontier schema")
    generators = int(document["generator_count"])
    columns = [int(value) for value in document["best_columns"]]
    if generators != 7:
        raise ValueError("certificate is not the seven-generator frontier")
    patterns = [
        pattern
        for pattern in range(1, 1 << generators)
        if pattern.bit_count() <= int(document["maximum_column_weight"])
    ]
    if len(patterns) != int(document["pattern_count"]) or len(patterns) != 28:
        raise ValueError("column-pattern count mismatch")
    if len(set(columns)) != len(columns) or not set(columns) <= set(patterns):
        raise ValueError("best columns are not distinct legal patterns")
    minimum_columns = int(document["minimum_column_count"])
    maximum_columns = int(document["maximum_column_count"])
    if (minimum_columns, maximum_columns) != (13, 28):
        raise ValueError("certificate does not cover every unresolved column count")
    if not minimum_columns <= len(columns) <= maximum_columns:
        raise ValueError("best witness lies outside the scanned column-count interval")
    labelled = sum(
        math.comb(len(patterns), count)
        for count in range(minimum_columns, maximum_columns + 1)
    )
    if labelled != int(document["labelled_configurations"]):
        raise ValueError("labelled configuration count mismatch")
    if int(document["covered_labelled_configurations"]) != labelled:
        raise ValueError("symmetry orbits do not cover the labelled configuration space")
    family = quotient_family(columns, generators)
    frequencies = [
        sum(member >> coordinate & 1 for member in family)
        for coordinate in range(len(columns))
    ]
    if len(family) != int(document["best_family_size"]):
        raise ValueError("best family size does not regenerate")
    if frequencies != [int(value) for value in document["best_frequencies"]]:
        raise ValueError("best frequencies do not regenerate")
    margin = 2 * max(frequencies) - len(family)
    if margin != int(document["minimum_margin"]) or margin < 0:
        raise ValueError("frontier margin is inconsistent")
    if document["counterexample_columns"]:
        raise ValueError("certificate contains a counterexample witness")
    signature = str(document["signature"])
    if len(signature) != 64:
        raise ValueError("scan signature is malformed")
    return {
        "schema": "rad.boolean-lattice.generator-frontier-verification.v2",
        "labelled_configurations": labelled,
        "covered_labelled_configurations": labelled,
        "symmetry_orbits": int(document["symmetry_orbits"]),
        "frontier_orbits": int(document["frontier_orbits"]),
        "minimum_margin": margin,
        "best_family_size": len(family),
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
