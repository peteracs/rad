"""Exact ordered-set SMT encoding for a fixed-size Frankl counterexample.

This is independent of both RAD's constructive search and the lazy 2^n
membership encoding. A family of size m is represented by m strictly ordered
n-bit vectors; every pairwise OR must equal one of those vectors.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from time import perf_counter

from z3 import (
    And,
    BitVec,
    BitVecSort,
    BitVecVal,
    Extract,
    Function,
    Or,
    PbGe,
    PbLe,
    Solver,
    UGE,
    ULE,
    ULT,
    sat,
)


def solve(
    width: int,
    family_size: int,
    timeout_ms: int,
    empty: str,
    engine: str,
    closure: str,
    minimal: bool,
    size_split: int | None,
    co_singletons: tuple[int, ...],
    output: Path | None,
) -> int:
    if family_size < 2:
        raise ValueError("family size must be at least two")
    sets = [BitVec(f"set_{index}", width) for index in range(family_size)]
    # Cardinality constraints are integer/PB formulas, so the mixed problem
    # must not be sent through a QF_BV-only tactic.
    solver = Solver()
    solver.set(timeout=timeout_ms)
    for index in range(family_size - 1):
        solver.add(ULT(sets[index], sets[index + 1]))
    solver.add(sets[-1] == BitVecVal((1 << width) - 1, width))
    if empty == "included":
        solver.add(sets[0] == BitVecVal(0, width))
    elif empty == "excluded":
        solver.add(sets[0] != BitVecVal(0, width))

    frequency_bits = [
        [Extract(bit, bit, member) == 1 for member in sets]
        for bit in range(width)
    ]
    minority_bound = (family_size - 1) // 2
    for bit, bits in enumerate(frequency_bits):
        solver.add(PbGe([(value, 1) for value in bits], 10 if minimal else 1))
        solver.add(PbLe([(value, 1) for value in bits], minority_bound))
        if minimal and bit >= width - 3:
            solver.add(PbGe([(value, 1) for value in bits], minority_bound))
    for bit in range(width - 1):
        solver.add(
            PbLe(
                [(value, 1) for value in frequency_bits[bit]]
                + [(value, -1) for value in frequency_bits[bit + 1]],
                0,
            )
        )

    if minimal:
        if len(co_singletons) != 3 or len(set(co_singletons)) != 3:
            raise ValueError("minimal mode requires three distinct co-singleton coordinates")
        for bit in co_singletons:
            if bit not in range(width):
                raise ValueError("co-singleton coordinate is out of range")
            target = BitVecVal(((1 << width) - 1) ^ (1 << bit), width)
            solver.add(Or(*(member == target for member in sets)))
        for member in sets:
            rank_bits = [(Extract(bit, bit, member) == 1, 1) for bit in range(width)]
            solver.add(Or(member == 0, PbGe(rank_bits, 3)))
        # Minimal support forces deletion of every coordinate to identify two
        # members: for each x there are A and A+{x} in the family.
        for bit in range(width):
            singleton = BitVecVal(1 << bit, width)
            solver.add(
                Or(
                    *[
                        And(
                            Extract(bit, bit, sets[left]) == 0,
                            sets[right] == sets[left] | singleton,
                        )
                        for left in range(family_size)
                        for right in range(left + 1, family_size)
                    ]
                )
            )
        if size_split is not None:
            if size_split not in range(3, 10):
                raise ValueError("the width-13 size split must lie in 3..9")
            low_ten = [
                PbLe([(Extract(bit, bit, member) == 1, 1) for bit in range(width)], size_split)
                for member in sets
            ]
            low_twenty_seven = [
                PbLe(
                    [(Extract(bit, bit, member) == 1, 1) for bit in range(width)],
                    12 - size_split,
                )
                for member in sets
            ]
            solver.add(PbGe([(value, 1) for value in low_ten], 10))
            solver.add(PbGe([(value, 1) for value in low_twenty_seven], 27))

    for left in range(width):
        for right in range(left + 1, width):
            solver.add(
                Or(
                    *[
                        Extract(left, left, member) != Extract(right, right, member)
                        for member in sets
                    ]
                )
            )

    # Frequency ordering plus separation gives a staircase member for each
    # coordinate except the most frequent one.
    for bit in range(width - 1):
        solver.add(
            Or(
                *[
                    And(
                        Extract(bit, bit, member) == 0,
                        *[
                            Extract(later, later, member) == 1
                            for later in range(bit + 1, width)
                        ],
                    )
                    for member in sets
                ]
            )
        )

    # Strict ordering means a union of positions i,j can only occur at or
    # after max(i,j). The direct form uses a finite disjunction. The indexed
    # form gives each pair a compact witness position into an uninterpreted
    # finite lookup table constrained at every legal index.
    if closure == "disjunction":
        for left in range(family_size):
            for right in range(left, family_size):
                union = sets[left] | sets[right]
                solver.add(Or(*[union == sets[index] for index in range(right, family_size)]))
    else:
        index_width = max(1, (family_size - 1).bit_length())
        member_at = Function("member_at", BitVecSort(index_width), BitVecSort(width))
        for index, member in enumerate(sets):
            solver.add(member_at(BitVecVal(index, index_width)) == member)
        for left in range(family_size):
            for right in range(left, family_size):
                witness = BitVec(f"join_{left}_{right}", index_width)
                solver.add(
                    UGE(witness, BitVecVal(right, index_width)),
                    ULE(witness, BitVecVal(family_size - 1, index_width)),
                    member_at(witness) == sets[left] | sets[right],
                )

    started = perf_counter()
    result = solver.check()
    elapsed = perf_counter() - started
    print(
        f"result={result} width={width} family_size={family_size} "
        f"empty={empty} engine={engine} closure={closure} solve_s={elapsed:.3f}"
    )
    if result != sat:
        return 0 if str(result) == "unsat" else 2

    model = solver.model()
    family = [model.eval(member).as_long() for member in sets]
    exact_frequencies = [sum(mask >> bit & 1 for mask in family) for bit in range(width)]
    if not all(2 * frequency < family_size for frequency in exact_frequencies):
        raise RuntimeError("SMT model violates the strict-minority objective")
    document = {
        "schema": "rad.frankl.indexed-smt-witness.v1",
        "width": width,
        "family_size": family_size,
        "family": family,
        "frequencies": exact_frequencies,
        "counterexample": max(exact_frequencies) * 2 < family_size,
    }
    encoded = json.dumps(document, indent=2, sort_keys=True)
    print(encoded)
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(encoded + "\n", encoding="utf-8")
    return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--family-size", type=int, default=51)
    parser.add_argument("--timeout-ms", type=int, default=120_000)
    parser.add_argument(
        "--empty",
        choices=("free", "included", "excluded"),
        default="free",
    )
    parser.add_argument("--minimal", action="store_true")
    parser.add_argument("--size-split", type=int, choices=range(3, 10))
    parser.add_argument(
        "--co-singletons",
        default="10,11,12",
        help="three comma-separated coordinates for the Lo Faro co-singletons",
    )
    parser.add_argument("--output", type=Path)
    parser.add_argument("--engine", choices=("smt",), default="smt")
    parser.add_argument(
        "--closure",
        choices=("disjunction", "indexed"),
        default="disjunction",
    )
    args = parser.parse_args()
    return solve(
        args.width,
        args.family_size,
        args.timeout_ms,
        args.empty,
        args.engine,
        args.closure,
        args.minimal,
        args.size_split,
        tuple(int(value) for value in args.co_singletons.split(",")),
        args.output,
    )


if __name__ == "__main__":
    raise SystemExit(main())
