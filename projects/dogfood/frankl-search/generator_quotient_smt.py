"""Exact SMT theorem-class solver for bounded join-generator families."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from time import perf_counter

import z3


def solve(
    width: int,
    generator_count: int,
    minimum_family_size: int,
    maximum_family_size: int | None,
    minimum_nonempty_rank: int,
    maximum_column_weight: int | None,
    fix_triple: bool,
    require_connected: bool,
    timeout_ms: int,
    output: Path | None,
) -> int:
    if width >= 1 << generator_count:
        raise ValueError("separation requires fewer coordinates than nonzero columns")
    columns = [z3.BitVec(f"column_{coordinate}", generator_count) for coordinate in range(width)]
    solver = z3.Solver()
    solver.set(timeout=timeout_ms)
    for coordinate, column in enumerate(columns):
        solver.add(column != 0)
        if maximum_column_weight is not None:
            solver.add(
                z3.PbLe(
                    [
                        (z3.Extract(bit, bit, column) == 1, 1)
                        for bit in range(generator_count)
                    ],
                    maximum_column_weight,
                )
            )
        if coordinate:
            solver.add(z3.ULT(columns[coordinate - 1], column))
    if fix_triple:
        if generator_count < 3 or maximum_column_weight is not None and maximum_column_weight < 3:
            raise ValueError("fix_triple requires at least three generators and weight-three columns")
        solver.add(z3.Or(*(column == 0b111 for column in columns)))
    if require_connected:
        full_generators = (1 << generator_count) - 1
        for left_side in range(1, full_generators):
            if left_side & 1 == 0:
                continue
            right_side = full_generators ^ left_side
            solver.add(
                z3.Or(
                    *(
                        z3.And(column & left_side != 0, column & right_side != 0)
                        for column in columns
                    )
                )
            )

    outputs = [z3.BitVec(f"output_{selected}", width) for selected in range(1 << generator_count)]
    for selected, output_value in enumerate(outputs):
        for coordinate, column in enumerate(columns):
            solver.add(
                z3.Extract(coordinate, coordinate, output_value)
                == z3.If(column & selected != 0, z3.BitVecVal(1, 1), z3.BitVecVal(0, 1))
            )

    # Generator-row permutations leave the quotient unchanged.  Sort their
    # 13-bit incidence rows to remove that factorial symmetry.
    rows = [z3.BitVec(f"row_{row}", width) for row in range(generator_count)]
    for row, row_value in enumerate(rows):
        for coordinate, column in enumerate(columns):
            solver.add(
                z3.Extract(coordinate, coordinate, row_value)
                == z3.Extract(row, row, column)
            )
    if fix_triple:
        for row in range(1, 3):
            solver.add(z3.ULE(rows[row - 1], rows[row]))
        for row in range(4, generator_count):
            solver.add(z3.ULE(rows[row - 1], rows[row]))
    else:
        for row in range(1, generator_count):
            solver.add(z3.ULE(rows[row - 1], rows[row]))

    first = [z3.Bool(f"first_{selected}") for selected in range(1 << generator_count)]
    for selected, first_here in enumerate(first):
        if selected == 0:
            solver.add(first_here)
        else:
            solver.add(
                first_here
                == z3.And(*(outputs[selected] != outputs[prior] for prior in range(selected)))
            )
    family_size = z3.Sum(*(z3.If(first_here, 1, 0) for first_here in first))
    solver.add(family_size >= minimum_family_size)
    if maximum_family_size is not None:
        solver.add(family_size <= maximum_family_size)
    if minimum_nonempty_rank > 0:
        for output_value in outputs:
            rank = z3.PbGe(
                [
                    (z3.Extract(coordinate, coordinate, output_value) == 1, 1)
                    for coordinate in range(width)
                ],
                minimum_nonempty_rank,
            )
            solver.add(z3.Or(output_value == 0, rank))
    frequencies = []
    for coordinate in range(width):
        frequency = z3.Sum(
            *(
                z3.If(
                    z3.And(first[selected], z3.Extract(coordinate, coordinate, outputs[selected]) == 1),
                    1,
                    0,
                )
                for selected in range(1 << generator_count)
            )
        )
        frequencies.append(frequency)
        solver.add(2 * frequency < family_size)

    started = perf_counter()
    result = solver.check()
    elapsed = perf_counter() - started
    document: dict[str, object] = {
        "schema": "rad.boolean-lattice.generator-quotient-proof.v1",
        "width": width,
        "generator_count": generator_count,
        "minimum_family_size": minimum_family_size,
        "maximum_family_size": maximum_family_size,
        "minimum_nonempty_rank": minimum_nonempty_rank,
        "maximum_column_weight": maximum_column_weight,
        "fixed_triple": fix_triple,
        "connected": require_connected,
        "result": str(result),
        "counterexample_exists": result == z3.sat,
        "elapsed_seconds": elapsed,
        "solver": "z3-smt",
    }
    if result == z3.sat:
        model = solver.model()
        concrete_columns = [model.eval(column).as_long() for column in columns]
        concrete_outputs = sorted(
            {
                sum(
                    1 << coordinate
                    for coordinate, column in enumerate(concrete_columns)
                    if selected & column
                )
                for selected in range(1 << generator_count)
            }
        )
        concrete_frequencies = [
            sum(member >> coordinate & 1 for member in concrete_outputs)
            for coordinate in range(width)
        ]
        if not all(2 * frequency < len(concrete_outputs) for frequency in concrete_frequencies):
            raise RuntimeError("SMT model violates the strict-minority objective")
        document.update(
            {
                "columns": concrete_columns,
                "family": concrete_outputs,
                "family_size": len(concrete_outputs),
                "frequencies": concrete_frequencies,
                "model_family_size": model.eval(family_size).as_long(),
                "model_frequencies": [model.eval(frequency).as_long() for frequency in frequencies],
            }
        )
    encoded = json.dumps(document, indent=2, sort_keys=True)
    print(encoded)
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(encoded + "\n", encoding="utf-8")
    return 1 if result == z3.sat else 0 if result == z3.unsat else 2


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--generators", type=int, default=6)
    parser.add_argument("--min-family-size", type=int, default=2)
    parser.add_argument("--max-family-size", type=int)
    parser.add_argument("--min-nonempty-rank", type=int, default=0)
    parser.add_argument("--max-column-weight", type=int)
    parser.add_argument("--fix-triple", action="store_true")
    parser.add_argument("--connected", action="store_true")
    parser.add_argument("--timeout-ms", type=int, default=300_000)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    return solve(
        args.width,
        args.generators,
        args.min_family_size,
        args.max_family_size,
        args.min_nonempty_rank,
        args.max_column_weight,
        args.fix_triple,
        args.connected,
        args.timeout_ms,
        args.output,
    )


if __name__ == "__main__":
    raise SystemExit(main())
