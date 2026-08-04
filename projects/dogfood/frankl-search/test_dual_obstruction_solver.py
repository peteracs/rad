from __future__ import annotations

import unittest

from dual_obstruction_solver import violated_subcube_density


class SubcubeDensityCutTests(unittest.TestCase):
    def test_every_small_union_closed_complement_satisfies_the_cut(self) -> None:
        width = 3
        cube_size = 1 << width
        for encoded in range(1 << cube_size):
            family = {member for member in range(cube_size) if encoded & (1 << member)}
            if any(left | right not in family for left in family for right in family):
                continue
            deleted = set(range(cube_size)) - family
            self.assertEqual(violated_subcube_density(deleted, width, cube_size), [])

    def test_a_high_rank_only_deletion_exposes_aggregate_violations(self) -> None:
        width = 5
        deleted = {
            member
            for member in range(1 << width)
            if member.bit_count() >= 3 and member != (1 << width) - 1
        }
        violations = violated_subcube_density(deleted, width, 1 << width)
        self.assertTrue(violations)
        for union in violations:
            deleted_subsets = sum(
                subset in deleted
                for subset in range(1 << width)
                if subset & ~union == 0
            )
            self.assertLess(2 * deleted_subsets, 1 << union.bit_count())


if __name__ == "__main__":
    unittest.main()
