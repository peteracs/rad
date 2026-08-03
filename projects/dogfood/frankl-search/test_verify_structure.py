from __future__ import annotations

import unittest

from verify_structure import (
    complement,
    deletion_surpluses,
    high_rank_family,
    or_violation_count,
    representatives,
)


class StructuralVerifierTests(unittest.TestCase):
    def test_or_convolution_counts_ordered_missing_unions(self) -> None:
        self.assertEqual(or_violation_count([0, 1, 2], 2), 2)
        self.assertEqual(or_violation_count([0, 1, 2, 3], 2), 0)

    def test_prime_width_has_expected_nontrivial_rotation_orbits(self) -> None:
        self.assertEqual(len(representatives(13)), 630)

    def test_high_rank_deletion_has_majority_but_breaks_closure(self) -> None:
        deleted = [member for member in high_rank_family(13, 7) if member != (1 << 13) - 1]
        remaining = complement(deleted, 13)
        self.assertGreater(min(deletion_surpluses(deleted, 13)), 0)
        self.assertGreater(or_violation_count(remaining, 13), 0)


if __name__ == "__main__":
    unittest.main()
