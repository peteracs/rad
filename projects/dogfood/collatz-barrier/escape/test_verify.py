"""Adversarial checks of exact arithmetic margins and required hypotheses."""

from fractions import Fraction
import unittest

from verify import exponent_data, log_bounds, strip_counts, strip_data, verify_strip


class EscapeChecks(unittest.TestCase):
    def test_weight_on_each_side_of_claimed_threshold(self):
        self.assertFalse(exponent_data(150, 100)["passes_one_over_28"])
        self.assertTrue(exponent_data(160, 100)["passes_one_over_28"])

    def test_certified_winner(self):
        self.assertEqual(exponent_data(156, 100)["exponent_floor"], 3585657)

    def test_atanh_domain_guard(self):
        with self.assertRaises(ValueError):
            log_bounds(1, 2)
        with self.assertRaises(ValueError):
            log_bounds(0, 0)

    def test_exact_deadline(self):
        row = strip_data(8)
        self.assertEqual((row["block_length"], row["linear_coefficient"], row["constant"]), (13, "92", "93226"))
        verify_strip(row)

    def test_rounding_down_is_rejected(self):
        row = strip_data(8)
        row["linear_coefficient"] = "91"
        with self.assertRaises(ValueError):
            verify_strip(row)

    def test_incorrect_word_count_is_rejected(self):
        row = strip_data(4)
        row["words"] = "9"
        with self.assertRaises(ValueError):
            verify_strip(row)

    def test_spread_is_not_just_maximum(self):
        # Trivial cycle: maximum coefficient stays 3/2 forever, while its
        # minimum tends to zero. Replacing spread by maximum is false.
        coefficients = [Fraction(1)]
        for step in range(100):
            coefficients.append(coefficients[-1] * (Fraction(3, 2) if step % 2 == 0 else Fraction(1, 2)))
        self.assertEqual(max(coefficients), Fraction(3, 2))
        self.assertGreater(max(coefficients) / min(coefficients), 1000000)

    def test_terminal_parity_count_is_not_prefix_count(self):
        # The five-step width-four strip has twenty words; checking only its
        # endpoint admits twenty-five. This mutation weakens the search.
        endpoint = sum(1 for word in range(32) if 3 ** word.bit_count() * 4 >= 32 and 3 ** word.bit_count() <= 128)
        self.assertEqual(endpoint, 25)
        self.assertEqual(strip_counts(4, 5)[-1], 20)


if __name__ == "__main__":
    unittest.main()
