"""Checks aimed at hypotheses that would otherwise yield false global proofs."""

from fractions import Fraction
import unittest

from verify import correction_data, iterate, profile, verify_bound, verify_merge


class CorrectionChecks(unittest.TestCase):
    def test_translation_requires_odd_count(self):
        self.assertEqual(iterate(1, 3), (2, 2))
        self.assertEqual(iterate(4, 3), (1, 2))
        self.assertNotEqual(iterate(9, 3)[1], iterate(12, 3)[1])

    def test_distinct_prefix_does_not_allow_unlimited_lookahead(self):
        states = [3, 5, 8, 4, 2, 1]
        self.assertEqual(len(states), len(set(states)))
        self.assertEqual(iterate(states[3], 1)[1], iterate(states[5], 1)[1])

    def test_repeat_hypothesis_is_essential(self):
        self.assertEqual(iterate(1, 54), (27, 1))
        self.assertGreater(Fraction(2 ** 54, 3 ** 27), 2048)

    def test_small_collision_profile(self):
        row = profile(3)
        self.assertEqual((row["classes"], row["odd_classes"]), (7, 4))

    def test_merge_before_any_coefficient_contraction(self):
        row = profile(6)
        self.assertEqual(row["coefficient_survivors"], 8)
        witness = dict(start=15, smaller=14, odds=4, endpoint=20)
        self.assertEqual(row["merged_survivors"], [witness])
        verify_merge(6, witness)
        self.assertGreater(3 ** 4, 2 ** 6)

    def test_forged_merge_odd_count_rejected(self):
        with self.assertRaises(ValueError):
            verify_merge(6, dict(start=15, smaller=14, odds=3, endpoint=20))

    def test_merging_into_larger_input_is_not_induction(self):
        with self.assertRaises(ValueError):
            verify_merge(6, dict(start=14, smaller=15, odds=4, endpoint=20))

    def test_global_certificate(self):
        row = correction_data(256, 0)
        self.assertEqual(row["correction_bits"], 11)
        verify_bound(row)

    def test_smaller_unsupported_constant_rejected(self):
        row = correction_data(256, 0)
        row["correction_bits"] = 10
        with self.assertRaises(ValueError):
            verify_bound(row)

    def test_omitted_terminal_boundary_rejected(self):
        row = correction_data(256, 0)
        row["sum_numerator"] = str(int(row["sum_numerator"]) - 2 * int(row["sum_denominator"]))
        with self.assertRaises(ValueError):
            verify_bound(row)

    def test_unproved_geometric_ratio_rejected(self):
        self.assertLess(2 * 41 ** 29 - 42 ** 29, 0)
        self.assertGreater(2 * 42 ** 29 - 43 ** 29, 0)


if __name__ == "__main__":
    unittest.main()
