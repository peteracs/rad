"""Rank residual permutation-symmetry cases by exact subset-orbit count."""

from __future__ import annotations

import json
from pathlib import Path

from layering_lemma import verify
from permutation_symmetry_solver import canonical_permutation, subset_orbits


certificate = verify(json.loads(
    Path(__file__).with_name("certificates").joinpath("cyclic-layering-n13.json").read_text(
        encoding="utf-8"
    )
))
rows = []
for encoded in certificate["uncovered"]:
    cycles = tuple(encoded)
    permutation, _ = canonical_permutation(cycles)
    rows.append((len(subset_orbits(permutation)), cycles))
for orbit_count, cycles in sorted(rows):
    print(f"{orbit_count:4} {','.join(str(length) for length in cycles)}")
