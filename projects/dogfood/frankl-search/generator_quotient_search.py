"""Search separating generator-column quotients of a Boolean join cube."""

from __future__ import annotations

import argparse
import json
import math
import random
from pathlib import Path
from time import perf_counter


def generators_from_columns(columns: list[int], generator_count: int) -> list[int]:
    return [
        sum(1 << coordinate for coordinate, column in enumerate(columns) if column >> row & 1)
        for row in range(generator_count)
    ]


def closure(generators: list[int]) -> list[int]:
    family = {0}
    for generator in generators:
        family |= {member | generator for member in tuple(family)}
    return sorted(family)


def profile(columns: list[int], generator_count: int) -> tuple[tuple[int, int, int], dict]:
    generators = generators_from_columns(columns, generator_count)
    family = closure(generators)
    frequencies = [
        sum(member >> coordinate & 1 for member in family)
        for coordinate in range(len(columns))
    ]
    margin = 2 * max(frequencies) - len(family)
    distance = abs(len(family) - 51)
    score = (margin, distance, -len(family)) if len(family) >= 51 else (100 + distance, distance, -len(family))
    return score, {
        "columns": sorted(columns),
        "generators": generators,
        "family": family,
        "frequencies": frequencies,
        "size": len(family),
        "margin": margin,
    }


def search(
    width: int,
    generator_count: int,
    restarts: int,
    steps: int,
    seed: int,
) -> dict:
    rng = random.Random(seed)
    patterns = list(range(1, 1 << generator_count))
    global_score = (10**9, 10**9, 0)
    global_best: dict = {}
    started = perf_counter()
    for restart in range(restarts):
        columns = rng.sample(patterns, width)
        current_score, current = profile(columns, generator_count)
        temperature = 4.0
        for step in range(steps):
            coordinate = rng.randrange(width)
            occupied = set(columns)
            replacement = rng.choice(patterns)
            while replacement in occupied:
                replacement = rng.choice(patterns)
            candidate_columns = columns.copy()
            candidate_columns[coordinate] = replacement
            candidate_score, candidate = profile(candidate_columns, generator_count)
            scalar_current = current_score[0] * 100 + current_score[1]
            scalar_candidate = candidate_score[0] * 100 + candidate_score[1]
            accept = candidate_score < current_score or rng.random() < math.exp(
                min(0.0, (scalar_current - scalar_candidate) / max(temperature, 0.05))
            )
            if accept:
                columns = candidate_columns
                current_score, current = candidate_score, candidate
            temperature *= 0.9995
            if current_score < global_score:
                global_score = current_score
                global_best = current
                print(
                    f"restart={restart} step={step} generators={generator_count} "
                    f"size={current['size']} margin={current['margin']} "
                    f"elapsed_s={perf_counter() - started:.3f}",
                    flush=True,
                )
                if current["margin"] < 0 and current["size"] >= 51:
                    return global_best
    return global_best


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--width", type=int, default=13)
    parser.add_argument("--generators", type=int, default=6)
    parser.add_argument("--restarts", type=int, default=64)
    parser.add_argument("--steps", type=int, default=10_000)
    parser.add_argument("--seed", type=int, default=1979)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    best = search(args.width, args.generators, args.restarts, args.steps, args.seed)
    encoded = json.dumps(best, indent=2, sort_keys=True)
    print(encoded)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    return 1 if best.get("margin", 0) < 0 else 0


if __name__ == "__main__":
    raise SystemExit(main())
