"""Verify and apply the coprime-cycle layering reduction."""

from __future__ import annotations

import argparse
import json
import math
from functools import reduce
from pathlib import Path


def integer_partitions(total: int, maximum: int | None = None):
    maximum = total if maximum is None else min(maximum, total)
    if total == 0:
        yield ()
        return
    for first in range(maximum, 0, -1):
        for rest in integer_partitions(total - first, first):
            yield (first,) + rest


def coprime_cycle_witness(cycle_type: tuple[int, ...], available_widths: set[int]) -> int | None:
    for index, length in enumerate(cycle_type):
        if length < 2 or length not in available_widths:
            continue
        outside_period = reduce(math.lcm, cycle_type[:index] + cycle_type[index + 1 :], 1)
        if math.gcd(length, outside_period) == 1:
            return length
    return None


def verify(document: dict[str, object]) -> dict[str, object]:
    target_width = int(document["target_width"])
    cases = document["cyclic_cases"]
    available = set()
    width_mask = 0
    total_horn = 0
    for case in cases:
        width = int(case["width"])
        if int(case["minimum_margin"]) < 0:
            raise ValueError(f"cyclic theorem at width {width} admits a counterexample")
        if len(case["horn_digest"]) != 64 or len(case["objective_digest"]) != 64:
            raise ValueError(f"cyclic theorem at width {width} has a malformed digest")
        available.add(width)
        width_mask |= 1 << width
        total_horn += int(case["horn_clause_count"])
    expected = set(range(2, target_width))
    if available != expected:
        raise ValueError(f"cyclic width coverage mismatch: {available} != {expected}")
    if int(document["target_proper_separating_minimum"]) < 0:
        raise ValueError("target-width cyclic theorem admits a separating counterexample")
    if len(document["target_horn_digest"]) != 64 or len(document["target_objective_digest"]) != 64:
        raise ValueError("target-width cyclic theorem has a malformed digest")

    identity = (1,) * target_width
    nonidentity = [partition for partition in integer_partitions(target_width) if partition != identity]
    covered = []
    uncovered = []
    for partition in nonidentity:
        witness = coprime_cycle_witness(partition, available | {target_width})
        (covered if witness is not None else uncovered).append((partition, witness))
    if len(covered) != int(document["covered_nonidentity_cycle_types"]):
        raise ValueError("covered cycle-type count mismatch")
    if len(uncovered) != int(document["uncovered_nonidentity_cycle_types"]):
        raise ValueError("uncovered cycle-type count mismatch")
    return {
        "schema": "rad.boolean-lattice.coprime-cycle-layering-verification.v1",
        "target_width": target_width,
        "cyclic_width_mask": width_mask,
        "cyclic_case_count": len(cases),
        "cyclic_horn_clause_count": total_horn,
        "nonidentity_cycle_type_count": len(nonidentity),
        "covered_cycle_type_count": len(covered),
        "uncovered_cycle_type_count": len(uncovered),
        "covered": [
            {"cycle_type": list(partition), "witness_cycle": witness}
            for partition, witness in covered
        ],
        "uncovered": [list(partition) for partition, _ in uncovered],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    document = json.loads(args.certificate.read_text(encoding="utf-8"))
    report = verify(document)
    encoded = json.dumps(report, indent=2, sort_keys=True)
    print(encoded)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
