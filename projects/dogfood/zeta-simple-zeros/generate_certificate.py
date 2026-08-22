"""Generate the Arb-backed input consumed by the RAD coverage verifier.

The source repository is pinned to commit 040c5e8.  Arb reconstructs the
transcendental interval tables and certifies convex-tangent leaves.  All
ordinary interval pruning, tree coverage, counters, and final arithmetic are
rechecked by verify.rad using scaled integers.
"""

from __future__ import annotations

import argparse
import itertools
import json
import math
import subprocess
import sys
from pathlib import Path
from typing import Iterable, Sequence


UPSTREAM_COMMIT = "040c5e899e658aed7b56a2a87f501798fe10761d"
KERNEL_SHA256 = "a9992300d2bf71665aa2b6bd2727e798624cd297103bb200c7f0ca2baea55a2c"
SECOND_SHA256 = "7913c5511a572c32dd573cd53123d8cf3ddf73d3ec63b1aa823faae2ae83570a"
SCALE = 100_000_000_000_000
H0_SCALE = 1_000_000_000
CHECKSUM_MODULUS = 2_147_483_629
CHECKSUM_BASE = 1_000_003


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("upstream", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--progress-every", type=int, default=100_000)
    parser.add_argument("--target-numerator", type=int, default=19)
    parser.add_argument("--target-denominator", type=int, default=5_000)
    return parser.parse_args()


def exact_scaled_floor(value: float, scale: int = SCALE) -> int:
    numerator, denominator = value.as_integer_ratio()
    return numerator * scale // denominator


def arb_scaled_floor(value: object, scale: int = SCALE) -> int:
    # Arb's lower endpoint is rigorous.  Widen its binary64 conversion once
    # more before taking an exact integer floor.
    candidate = math.nextafter(float(value.lower()), -math.inf)  # type: ignore[attr-defined]
    return exact_scaled_floor(candidate, scale)


def checksum(values: Iterable[int]) -> int:
    result = 0
    for value in values:
        result = (
            result * CHECKSUM_BASE + value % CHECKSUM_MODULUS
        ) % CHECKSUM_MODULUS
    return result


def components(indices: Iterable[int]) -> list[tuple[int, int]]:
    result: list[list[int]] = []
    for index in indices:
        if not result or index > result[-1][1] + 1:
            result.append([index, index])
        else:
            result[-1][1] = index
    return [(left, right) for left, right in result]


def main() -> int:
    args = parse_args()
    upstream = args.upstream.resolve()
    source = upstream / "src"
    if not source.is_dir():
        raise SystemExit(f"missing upstream source directory: {source}")

    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=upstream, text=True
    ).strip()
    if revision != UPSTREAM_COMMIT:
        raise SystemExit(
            f"upstream revision mismatch: expected {UPSTREAM_COMMIT}, got {revision}"
        )

    sys.path.insert(0, str(source))
    from flint import arb, fmpq
    from zeta_simple_zeros.kernel import (
        RangeMinimum,
        build_kernel_table,
        build_second_derivative_lower_table,
        kernel_constants,
        squared_kernel_derivatives,
        table_sha256,
    )
    from zeta_simple_zeros.verify_seven import (
        COEFFICIENT_RATIONALS,
        COEFFICIENTS,
        COEFFICIENTS_UP,
        GRID,
        PRECISION_BITS,
        PRESSURE_CUTOFF_CELLS,
        PRESSURE_DENOMINATOR,
    )

    if args.target_numerator <= 0 or args.target_denominator <= 0:
        raise SystemExit("target numerator and denominator must be positive")

    cell_count = PRESSURE_CUTOFF_CELLS + 8
    float_table = build_kernel_table(GRID, cell_count, PRECISION_BITS)
    second_table = build_second_derivative_lower_table(
        GRID, cell_count, start_index=3_800, precision=PRECISION_BITS
    )
    kernel_hash = table_sha256(float_table)
    second_hash = table_sha256(second_table)
    if kernel_hash != KERNEL_SHA256 or second_hash != SECOND_SHA256:
        raise SystemExit(
            "upstream interval-table digest mismatch; refusing to emit a certificate"
        )

    table = [exact_scaled_floor(value) for value in float_table]
    ranges = RangeMinimum(table)
    second_ranges = RangeMinimum(second_table)
    constants = kernel_constants()
    target_scaled = (
        args.target_numerator * SCALE + args.target_denominator - 1
    ) // args.target_denominator
    search_pressure_cutoff_cells = (
        target_scaled * GRID * PRESSURE_DENOMINATOR + SCALE - 1
    ) // SCALE
    coefficient_pairs = {
        1: (1, 3),
        2: (2, 5),
        3: (1, 2),
        4: (2, 3),
        5: (1, 1),
        6: (2, 1),
    }

    surviving: list[int] = []
    for index in range(search_pressure_cutoff_cells):
        pressure = index * SCALE // (GRID * PRESSURE_DENOMINATOR)
        numerator, denominator = coefficient_pairs[1]
        kernel_lower = table[index] if index < len(table) else 0
        one_body = pressure + numerator * kernel_lower // denominator
        if one_body < target_scaled:
            surviving.append(index)
    gap_components = components(surviving)

    def kernel_min(left: int, right: int) -> int:
        if right >= ranges.length:
            return 0
        return ranges.query(left, right)

    def box_lower(box: Sequence[tuple[int, int]]) -> int:
        low_prefix = [0]
        high_prefix = [0]
        for low, high in box:
            low_prefix.append(low_prefix[-1] + low)
            high_prefix.append(high_prefix[-1] + high)
        result = low_prefix[-1] * SCALE // (GRID * PRESSURE_DENOMINATOR)
        for span in range(1, 7):
            numerator, denominator = coefficient_pairs[span]
            for start in range(7 - span):
                left = low_prefix[start + span] - low_prefix[start]
                right = high_prefix[start + span] - high_prefix[start] + span - 1
                result += numerator * kernel_min(left, right) // denominator
        return result

    def coefficient_times_signed_lower(span: int, lower: float) -> float:
        if lower == -math.inf:
            return lower
        coefficient = COEFFICIENTS[span] if lower >= 0.0 else COEFFICIENTS_UP[span]
        return math.nextafter(coefficient * lower, -math.inf)

    def exact_float(value: float) -> object:
        numerator, denominator = value.as_integer_ratio()
        return arb(fmpq(numerator, denominator))

    def arb_ldl_is_positive(terms: Sequence[tuple[int, int, float]]) -> bool:
        matrix = [[arb(0) for _ in range(6)] for _ in range(6)]
        for start, span, coefficient in terms:
            exact = exact_float(coefficient)
            for row in range(start, start + span):
                for column in range(start, start + span):
                    matrix[row][column] += exact
        lower = [[arb(0) for _ in range(6)] for _ in range(6)]
        diagonal = [arb(0) for _ in range(6)]
        for column in range(6):
            lower[column][column] = arb(1)
            pivot = matrix[column][column]
            for previous in range(column):
                pivot -= lower[column][previous] ** 2 * diagonal[previous]
            if not (pivot > 0):
                return False
            diagonal[column] = pivot
            for row in range(column + 1, 6):
                value = matrix[row][column]
                for previous in range(column):
                    value -= (
                        lower[row][previous]
                        * lower[column][previous]
                        * diagonal[previous]
                    )
                lower[row][column] = value / pivot
        return True

    def tangent_scaled(box: Sequence[tuple[int, int]]) -> int | None:
        low_prefix = [0]
        high_prefix = [0]
        for low, high in box:
            low_prefix.append(low_prefix[-1] + low)
            high_prefix.append(high_prefix[-1] + high)
        terms: list[tuple[int, int, float]] = []
        for span in range(1, 7):
            for start in range(7 - span):
                left = low_prefix[start + span] - low_prefix[start]
                right = high_prefix[start + span] - high_prefix[start] + span - 1
                if right >= second_ranges.length:
                    return None
                scalar = coefficient_times_signed_lower(
                    span, second_ranges.query(left, right)
                )
                if scalar == -math.inf:
                    return None
                terms.append((start, span, scalar))
        if not arb_ldl_is_positive(terms):
            return None

        midpoints = [fmpq(low + high + 1, 2 * GRID) for low, high in box]
        radii = [fmpq(high - low + 1, 2 * GRID) for low, high in box]
        value = sum((arb(point) for point in midpoints), arb(0)) / PRESSURE_DENOMINATOR
        gradient = [arb(fmpq(1, PRESSURE_DENOMINATOR)) for _ in range(6)]
        for span in range(1, 7):
            coefficient = arb(COEFFICIENT_RATIONALS[span])
            for start in range(7 - span):
                point = sum(midpoints[start : start + span], fmpq(0))
                potential, derivative, _ = squared_kernel_derivatives(
                    arb(point), constants
                )
                value += coefficient * potential
                for coordinate in range(start, start + span):
                    gradient[coordinate] += coefficient * derivative
        lower = value
        for derivative, radius in zip(gradient, radii):
            lower -= derivative.abs_upper() * arb(radius)
        return arb_scaled_floor(lower)

    initial = [
        tuple(parts) for parts in itertools.product(gap_components, repeat=6)
    ]
    stack: list[tuple[tuple[tuple[int, int], ...], int]] = [
        (box, 0) for box in initial
    ]
    tangent_boxes: list[list[int]] = []
    nodes = pruned = splits = maximum_depth = 0
    pressure_pruned = interval_pruned = tangent_pruned = 0

    while stack:
        box, depth = stack.pop()
        nodes += 1
        maximum_depth = max(maximum_depth, depth)
        pressure_cells = sum(part[0] for part in box)
        if pressure_cells >= search_pressure_cutoff_cells:
            pressure_lower = pressure_cells * SCALE // (GRID * PRESSURE_DENOMINATOR)
            if pressure_lower >= target_scaled:
                pruned += 1
                pressure_pruned += 1
                continue
        if box_lower(box) >= target_scaled:
            pruned += 1
            interval_pruned += 1
            continue
        tangent = tangent_scaled(box)
        if tangent is not None and tangent >= target_scaled:
            tangent_boxes.append(
                [coordinate for part in box for coordinate in part] + [tangent]
            )
            pruned += 1
            tangent_pruned += 1
            continue
        widths = [right - left for left, right in box]
        if max(widths) == 0:
            raise SystemExit(f"unresolved terminal fixed-point box: {box}")
        splits += 1
        coordinate = max(range(6), key=widths.__getitem__)
        left, right = box[coordinate]
        midpoint = (left + right) // 2
        lower_half = list(box)
        upper_half = list(box)
        lower_half[coordinate] = (left, midpoint)
        upper_half[coordinate] = (midpoint + 1, right)
        stack.append((tuple(lower_half), depth + 1))
        stack.append((tuple(upper_half), depth + 1))
        if args.progress_every and nodes % args.progress_every == 0:
            print(
                f"nodes={nodes} pending={len(stack)} "
                f"tangent={tangent_pruned} depth={maximum_depth}"
            )

    inv_sqrt_two = 1 / arb(2).sqrt()
    h0 = arb(3) / 2 - inv_sqrt_two / inv_sqrt_two.tan()
    h0_lower_scaled = arb_scaled_floor(h0, H0_SCALE)
    flattened_tangents = (
        value for tangent in tangent_boxes for value in tangent
    )
    certificate = {
        "format": "rad-zeta-seven-v1",
        "upstream_commit": UPSTREAM_COMMIT,
        "kernel_sha256": kernel_hash,
        "second_derivative_sha256": second_hash,
        "grid": GRID,
        "precision_bits": PRECISION_BITS,
        "pressure_cutoff_cells": search_pressure_cutoff_cells,
        "scale": SCALE,
        "target_scaled": target_scaled,
        "h0_scale": H0_SCALE,
        "h0_lower_scaled": h0_lower_scaled,
        "kernel_scaled": table,
        "kernel_checksum": checksum(table),
        "gap_components": [list(part) for part in gap_components],
        "tangent_boxes": tangent_boxes,
        "tangent_checksum": checksum(flattened_tangents),
        "stats": {
            "initial_boxes": len(initial),
            "nodes": nodes,
            "pruned": pruned,
            "splits": splits,
            "maximum_depth": maximum_depth,
            "pressure_pruned": pressure_pruned,
            "interval_pruned": interval_pruned,
            "tangent_pruned": tangent_pruned,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(certificate, handle, separators=(",", ":"))
        handle.write("\n")
    print(json.dumps(certificate["stats"], sort_keys=True))
    print(f"certificate={args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
