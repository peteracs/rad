"""Mutation controls for mathematically material certificate boundaries."""

import copy
import unittest

from verify import (SCALE, expected_envelopes, literal_range, logarithm,
                    parity_dp, verify_barrier, verify_rotation)


def barrier():
    a, b, c, d = 103768467013, 65470613321, 10439860591, 6586818670
    l2, _ = logarithm(3)
    _, u3 = logarithm(2)
    gap = c * l2 - d * u3
    return dict(label="test", lower_numerator=a, lower_denominator=b,
                upper_numerator=c, upper_denominator=d, floor_value=str(1 << 71),
                gap_lower=str(gap), strict_margin=str(3 * (1 << 71) * gap - d * SCALE),
                last_depth=a + c - 1)


def rotations():
    maxima = [46438023168, 81266540544, 118674948096, 153503465472,
              182885548032, 220036414464, 253096296192, 292273339392,
              325333221120, 356483939328, 389543821056, 416200322061]
    phases = [0, 1, 0, 1, 0, 5, 6, 5, 6, 5, 6, 11]
    return [dict(block_length=m, phases=m, maximizing_phase=phases[m - 1],
                 maximum_scaled=maxima[m - 1], denominator=46438023168)
            for m in range(1, 13)]


class CertificateTests(unittest.TestCase):
    def test_real_barrier(self):
        self.assertEqual(verify_barrier(barrier())["last_depth"], 114208327603)

    def test_horizon_cannot_include_next_mediant(self):
        row = barrier()
        row["last_depth"] += 1
        with self.assertRaisesRegex(ValueError, "off by one"):
            verify_barrier(row)

    def test_bad_farey_data_rejected(self):
        row = barrier()
        row["lower_numerator"] += 1
        with self.assertRaisesRegex(ValueError, "determinant"):
            verify_barrier(row)

    def test_insufficient_floor_rejected(self):
        row = barrier()
        row["floor_value"] = "1"
        with self.assertRaisesRegex(ValueError, "does not imply descent"):
            verify_barrier(row)

    def test_forged_interval_rejected(self):
        row = barrier()
        row["gap_lower"] = str(int(row["gap_lower"]) + 1)
        with self.assertRaisesRegex(ValueError, "gap certificate"):
            verify_barrier(row)

    def test_rotation_certificate_and_mutation(self):
        rows = rotations()
        self.assertEqual(verify_rotation(rows)[2]["maximum"], "23/9")
        mutation = copy.deepcopy(rows)
        mutation[2]["maximum_scaled"] -= 1
        with self.assertRaises(ValueError):
            verify_rotation(mutation)

    def test_missing_rotation_phase_length(self):
        with self.assertRaisesRegex(ValueError, "missing rotation"):
            verify_rotation(rotations()[:-1])

    def test_integral_threshold_is_strict(self):
        self.assertEqual(expected_envelopes(2)[0]["cutoff"], 2)

    def test_unknown_tail_is_reported(self):
        self.assertEqual(literal_range(27, 28, 32)["censored"], 1)
        self.assertEqual(literal_range(27, 28, 64)["max_tau"], 59)

    def test_independent_parity_dynamic_program(self):
        self.assertEqual(parity_dp(20)["odd_counts"], 12)


if __name__ == "__main__":
    unittest.main()
