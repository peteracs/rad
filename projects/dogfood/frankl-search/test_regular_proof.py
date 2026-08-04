from __future__ import annotations

import contextlib
import io
import json
import tempfile
import unittest
from itertools import product
from pathlib import Path

from pysat.solvers import Solver

from orbit_horn import all_join_horn_clauses, orbit_owner
from permutation_symmetry_solver import (
    canonical_permutation,
    solve as solve_permutation_symmetry,
    subset_orbits,
)
from regular_family_miner import is_union_closed, rotation_orbits
from regular_maxsat_solver import solve as solve_regular
from weighted_cnf import encode_weighted_at_most


class WeightedCnfTests(unittest.TestCase):
    def test_weighted_encoder_matches_every_assignment(self) -> None:
        terms = [(1, 3), (-2, 2), (3, 5), (-4, 1)]
        variable_count, clauses = encode_weighted_at_most(4, terms, 6)
        self.assertGreater(variable_count, 4)
        for assignment in product((False, True), repeat=4):
            units = [[index + 1 if value else -(index + 1)] for index, value in enumerate(assignment)]
            with Solver(name="cadical195", bootstrap_with=clauses + units) as solver:
                actual = solver.solve()
            expected = sum(
                weight
                for literal, weight in terms
                if assignment[abs(literal) - 1] == (literal > 0)
            ) <= 6
            self.assertEqual(actual, expected, assignment)

    def test_accumulator_does_not_wrap_at_comparison_bound(self) -> None:
        terms = [(1, 7), (2, 7), (3, 7)]
        _, clauses = encode_weighted_at_most(3, terms, 7)
        with Solver(
            name="cadical195",
            bootstrap_with=clauses + [[1], [2], [3]],
        ) as solver:
            self.assertFalse(solver.solve())


class OrbitHornTests(unittest.TestCase):
    def test_horn_theory_is_exact_for_small_cyclic_families(self) -> None:
        for width in range(2, 6):
            orbits = rotation_orbits(width)
            owner = orbit_owner(orbits, 1 << width)
            clauses = all_join_horn_clauses(orbits, owner)
            for selection in range(1 << len(orbits)):
                selected = {index for index in range(len(orbits)) if selection >> index & 1}
                horn_closed = all(
                    left not in selected or right not in selected or target in selected
                    for left, right, target in clauses
                )
                member_bits = 0
                for index in selected:
                    for member in orbits[index]:
                        member_bits |= 1 << member
                self.assertEqual(horn_closed, is_union_closed(member_bits, 1 << width))

    def test_exact_optimizer_finds_sharp_small_separating_margin(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "proof.json"
            with contextlib.redirect_stdout(io.StringIO()):
                result = solve_regular(6, True, True, True, output)
            self.assertEqual(result, 0)
            proof = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(proof["minimum_margin"], 1)
            self.assertEqual(proof["family_encoding"], "all_nonempty_subsets")
            self.assertFalse(proof["counterexample_exists"])

    def test_permutation_orbits_partition_and_small_symmetry_is_safe(self) -> None:
        permutation, _ = canonical_permutation((2, 2))
        orbits = subset_orbits(permutation)
        self.assertEqual(sorted(member for orbit in orbits for member in orbit), list(range(16)))
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "symmetry.json"
            with contextlib.redirect_stdout(io.StringIO()):
                result = solve_permutation_symmetry(4, (2, 2), True, 100, "lazy", 1_000, 100, output)
            self.assertEqual(result, 0)
            proof = json.loads(output.read_text(encoding="utf-8"))
            self.assertFalse(proof["counterexample_exists"])


if __name__ == "__main__":
    unittest.main()
