"""Stream an exact coloured-graph quotient frontier from nauty ``vcolg``.

The input is the ``-T`` output of ``geng -c 8 | vcolg -T -m2``.  A graph
edge is a weight-two incidence column and a colour-one vertex is a singleton
column.  Nauty supplies exactly one representative of every isomorphism
class; this program independently evaluates the induced Boolean quotient.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path


GENERATOR_COUNT = 8
EDGE_PAIRS = tuple(
    (left, right)
    for left in range(GENERATOR_COUNT)
    for right in range(left + 1, GENERATOR_COUNT)
)
EDGE_INDEX = {pair: index for index, pair in enumerate(EDGE_PAIRS)}
INDUCED_EDGES = tuple(
    sum(
        1 << index
        for index, (left, right) in enumerate(EDGE_PAIRS)
        if subset >> left & 1 and subset >> right & 1
    )
    for subset in range(1 << GENERATOR_COUNT)
)


def evaluate(loop_mask: int, edge_mask: int) -> tuple[int, list[int], int]:
    """Return quotient size, selected-column frequencies, and Frankl margin."""

    absence_signatures = {
        (loop_mask & subset) | ((edge_mask & INDUCED_EDGES[subset]) << 8)
        for subset in range(1 << GENERATOR_COUNT)
    }
    family_size = len(absence_signatures)
    absent_counts = [0] * 36
    for signature in absence_signatures:
        loops = signature & 0xFF
        edges = signature >> 8
        while loops:
            index = (loops & -loops).bit_length() - 1
            absent_counts[index] += 1
            loops &= loops - 1
        while edges:
            index = (edges & -edges).bit_length() - 1
            absent_counts[8 + index] += 1
            edges &= edges - 1
    selected = [
        index
        for index in range(8)
        if loop_mask >> index & 1
    ] + [
        8 + index
        for index in range(28)
        if edge_mask >> index & 1
    ]
    frequencies = [family_size - absent_counts[index] for index in selected]
    margin = 2 * max(frequencies, default=0) - family_size
    return family_size, frequencies, margin


def parse_coloured_graph(line: str) -> tuple[int, int]:
    fields = [int(value) for value in line.split()]
    if len(fields) < 10 or fields[0] != GENERATOR_COUNT:
        raise ValueError("expected an eight-vertex vcolg -T record")
    edge_count = fields[1]
    colours = fields[2:10]
    endpoints = fields[10:]
    if len(endpoints) != 2 * edge_count or any(colour not in (0, 1) for colour in colours):
        raise ValueError("malformed coloured-graph record")
    loop_mask = sum(1 << vertex for vertex, colour in enumerate(colours) if colour)
    edge_mask = 0
    for index in range(0, len(endpoints), 2):
        edge = tuple(sorted((endpoints[index], endpoints[index + 1])))
        edge_mask |= 1 << EDGE_INDEX[edge]
    if edge_mask.bit_count() != edge_count:
        raise ValueError("duplicate or malformed graph edge")
    return loop_mask, edge_mask


def scan(lines: object, minimum_columns: int, minimum_family: int, maximum_family: int) -> dict:
    digest = hashlib.blake2b(digest_size=32)
    graph_orbits = 0
    scanned_orbits = 0
    frontier_orbits = 0
    minimum_margin: int | None = None
    best: dict[str, object] | None = None
    counterexample: dict[str, object] | None = None
    for line_number, line in enumerate(lines, 1):
        if not line.strip():
            continue
        graph_orbits += 1
        loop_mask, edge_mask = parse_coloured_graph(line)
        column_count = loop_mask.bit_count() + edge_mask.bit_count()
        if column_count < minimum_columns:
            continue
        scanned_orbits += 1
        family_size, frequencies, margin = evaluate(loop_mask, edge_mask)
        digest.update(loop_mask.to_bytes(1, "little"))
        digest.update(edge_mask.to_bytes(4, "little"))
        digest.update(family_size.to_bytes(2, "little"))
        digest.update(margin.to_bytes(2, "little", signed=True))
        if minimum_family <= family_size <= maximum_family:
            frontier_orbits += 1
            witness = {
                "loop_mask": loop_mask,
                "edge_mask": edge_mask,
                "column_count": column_count,
                "family_size": family_size,
                "frequencies": frequencies,
                "margin": margin,
            }
            if minimum_margin is None or margin < minimum_margin:
                minimum_margin = margin
                best = witness
            if margin < 0 and counterexample is None:
                counterexample = witness
        if line_number % 250_000 == 0:
            print(
                f"processed={line_number} scanned={scanned_orbits} frontier={frontier_orbits}",
                file=sys.stderr,
            )
    return {
        "schema": "rad.boolean-lattice.generator-graph-frontier.v1",
        "generator_count": GENERATOR_COUNT,
        "minimum_column_count": minimum_columns,
        "maximum_column_count": 36,
        "maximum_column_weight": 2,
        "minimum_family_size": minimum_family,
        "maximum_family_size": maximum_family,
        "coloured_graph_orbits": graph_orbits,
        "scanned_orbits": scanned_orbits,
        "frontier_orbits": frontier_orbits,
        "minimum_margin": minimum_margin,
        "best": best,
        "counterexample": counterexample,
        "signature": digest.hexdigest(),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--minimum-columns", type=int, default=13)
    parser.add_argument("--minimum-family", type=int, default=64)
    parser.add_argument("--maximum-family", type=int, default=127)
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.input:
        with args.input.open(encoding="utf-8") as lines:
            result = scan(lines, args.minimum_columns, args.minimum_family, args.maximum_family)
    else:
        result = scan(sys.stdin, args.minimum_columns, args.minimum_family, args.maximum_family)
    encoded = json.dumps(result, indent=2, sort_keys=True)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)
    return 1 if result["counterexample"] is not None else 0


if __name__ == "__main__":
    raise SystemExit(main())
