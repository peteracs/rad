"""Adversarial tests for abundance versus generator-column multiplicity."""

from __future__ import annotations

import argparse
import itertools
import json
import random


def quotient_family(columns: tuple[int, ...], generator_count: int) -> tuple[int, ...]:
    outputs = set()
    for selected in range(1 << generator_count):
        output = 0
        for coordinate, column in enumerate(columns):
            if selected & column:
                output |= 1 << coordinate
        outputs.add(output)
    return tuple(sorted(outputs))


def first_violation(columns: tuple[int, ...], generator_count: int) -> dict | None:
    family = quotient_family(columns, generator_count)
    for coordinate, column in enumerate(columns):
        if column.bit_count() < 2:
            continue
        frequency = sum(member >> coordinate & 1 for member in family)
        if frequency * 2 < len(family):
            return {
                "columns": list(columns),
                "generator_count": generator_count,
                "family": list(family),
                "coordinate": coordinate,
                "column": column,
                "column_weight": column.bit_count(),
                "frequency": frequency,
                "family_size": len(family),
            }
    return None


def exhaustive(generator_count: int) -> tuple[int, dict | None]:
    patterns = tuple(range(1, 1 << generator_count))
    checked = 0
    for width in range(1, len(patterns) + 1):
        for columns in itertools.combinations(patterns, width):
            checked += 1
            violation = first_violation(columns, generator_count)
            if violation is not None:
                return checked, violation
    return checked, None


def randomized(generator_count: int, width: int, trials: int, seed: int) -> dict | None:
    rng = random.Random(seed)
    patterns = list(range(1, 1 << generator_count))
    for trial in range(trials):
        columns = tuple(sorted(rng.sample(patterns, width)))
        violation = first_violation(columns, generator_count)
        if violation is not None:
            violation["trial"] = trial
            return violation
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--exhaustive-generators", type=int, default=4)
    parser.add_argument("--random-generators", type=int, default=8)
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--trials", type=int, default=100_000)
    parser.add_argument("--seed", type=int, default=1979)
    args = parser.parse_args()
    checked, exact_violation = exhaustive(args.exhaustive_generators)
    print(json.dumps({"exhaustive_checked": checked, "violation": exact_violation}, sort_keys=True))
    if exact_violation is not None:
        return 1
    sampled_violation = randomized(
        args.random_generators,
        args.width,
        args.trials,
        args.seed,
    )
    print(json.dumps({"randomized_trials": args.trials, "violation": sampled_violation}, sort_keys=True))
    return 1 if sampled_violation is not None else 0


if __name__ == "__main__":
    raise SystemExit(main())
