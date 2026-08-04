"""Deterministic feasible seeds for the 51-by-13 minimal-counterexample frontier."""

from __future__ import annotations

import random


def sampled_seed(
    width: int,
    family_size: int,
    overlap: int,
    size_split: int,
    attempts: int = 250_000,
) -> list[int] | None:
    cube_size = 1 << width
    full = cube_size - 1
    co_singletons = tuple(range(overlap)) + tuple(range(3, 6 - overlap))
    fixed = {full, *(full ^ (1 << bit) for bit in co_singletons)}
    allowed = [
        mask
        for mask in range(cube_size)
        if mask not in fixed and mask.bit_count() not in (1, 2)
    ]
    # Bias toward the low-rank side required by w_10 + w_27 < 13, and
    # toward roughly half incidence on the three tight coordinates.
    weights = []
    for mask in allowed:
        rank = mask.bit_count()
        tight_rank = (mask & 0b111).bit_count()
        low_bonus = 8 if rank <= 12 - size_split else 1
        tight_bonus = (1, 4, 4, 1)[tight_rank]
        weights.append(low_bonus * tight_bonus)

    rng = random.Random((width << 24) ^ (family_size << 12) ^ (overlap << 8) ^ size_split)
    needed = family_size - len(fixed)
    for _ in range(attempts):
        chosen = set(rng.choices(allowed, weights=weights, k=needed * 2))
        if len(chosen) < needed:
            continue
        family = sorted(fixed | set(rng.sample(sorted(chosen), needed)))
        frequencies = [sum(mask >> bit & 1 for mask in family) for bit in range(width)]
        if frequencies[:3] != [25, 25, 25]:
            continue
        if not all(10 <= frequency <= 25 for frequency in frequencies):
            continue
        if sum(mask.bit_count() <= size_split for mask in family) < 10:
            continue
        if sum(mask.bit_count() <= 12 - size_split for mask in family) < 27:
            continue
        return family
    return None
