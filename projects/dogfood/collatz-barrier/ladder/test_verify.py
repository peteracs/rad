"""The verifier must accept the recorded RAD run and reject tampered claims."""

import copy
from fractions import Fraction
from pathlib import Path
import unittest

from reference import BLOCK, S, SIGMA, backward_bounds, extremal_offsets, exact_f, forward_bounds
from verify import read_rows, split_rows, verify_ladder, verify_rows

RUN = Path(__file__).resolve().parent / "out" / "run-1.txt"


class ReferenceArithmetic(unittest.TestCase):
    def test_phase_values_are_exact_small_cases(self):
        lo, hi = forward_bounds(4)
        # v_0=1, v_1=2/3, v_2=8/9, v_3=16/27
        for k, value in enumerate((Fraction(1), Fraction(2, 3), Fraction(8, 9), Fraction(16, 27))):
            self.assertLessEqual(Fraction(lo[k], S), value)
            self.assertGreaterEqual(Fraction(hi[k], S), value)
            self.assertLessEqual(hi[k] - lo[k], 1)
        blo, bhi = backward_bounds(3)
        # v_-1 = 3/4, v_-2 = 9/16
        self.assertEqual((blo[1], bhi[1]), (3 * S // 4, 3 * S // 4))
        self.assertEqual((blo[2], bhi[2]), (9 * S // 16, 9 * S // 16))

    def test_known_sharp_intercept_is_recovered(self):
        offsets = extremal_offsets(3)
        self.assertEqual(offsets, [0, 1, 5, 23])
        self.assertEqual(exact_f(offsets, 3) / 3 - Fraction(3, 4), Fraction(11, 108))


@unittest.skipUnless(RUN.exists(), "run accept.py first to record a RAD run")
class RecordedRun(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = read_rows(RUN)
        cls.lanes, cls.assembled, cls.probes, cls.ladder, cls.complete = split_rows(cls.rows)

    def test_recorded_run_verifies(self):
        summary = verify_rows(self.rows)
        self.assertEqual(summary["vertices"][-1], 7489)
        self.assertEqual(set(summary["gaps"][2:]), {12, 41, 665})

    def test_missing_lane_is_rejected(self):
        with self.assertRaises(ValueError):
            verify_rows([r for r in self.rows if r is not self.lanes[3]])

    def check_ladder_rejects(self, mutate):
        ladder = copy.deepcopy(self.ladder)
        mutate(ladder)
        with self.assertRaises(ValueError):
            verify_ladder(ladder, self.assembled["excess"])

    def test_enclosure_that_misses_exact_value_is_rejected(self):
        def mutate(ladder):
            ladder["vertex_hi"][5] = ladder["vertex_lo"][5] - 1
        self.check_ladder_rejects(mutate)

    def test_extra_or_missing_vertex_is_rejected(self):
        def drop(ladder):
            del ladder["vertices"][9]
            ladder["gaps"] = [b - a for a, b in zip(ladder["vertices"], ladder["vertices"][1:])]
            del ladder["vertex_lo"][9], ladder["vertex_hi"][9]
        self.check_ladder_rejects(drop)

        def extend(ladder):
            ladder["vertices"].append(8154)
            ladder["gaps"].append(665)
        self.check_ladder_rejects(extend)

    def test_bootstrap_too_early_is_rejected(self):
        def mutate(ladder):
            ladder["tail_start"] = 9000
        self.check_ladder_rejects(mutate)

    def test_wrong_slope_is_rejected(self):
        def mutate(ladder):
            ladder["sigma_numerator"] += 1
        self.check_ladder_rejects(mutate)

    def test_constants(self):
        self.assertEqual(BLOCK, 15601)
        self.assertLess(SIGMA, Fraction(7214, 10000))


if __name__ == "__main__":
    unittest.main()
