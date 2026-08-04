from __future__ import annotations

import copy
import unittest

from analyze_deletion import analyze
from verify_deletion import verify


def full_square_certificate() -> dict:
    return {
        "width": 2,
        "deleted": [],
        "family_size": 4,
        "frequencies": [2, 2],
        "deletion_surpluses": [0, 0],
        "pair_biases": [0],
        "margin": 0,
        "frontier": [0, 1, 2],
        "separating": True,
    }


class DeletionCertificateTests(unittest.TestCase):
    def test_pair_bias_is_independently_recomputed(self) -> None:
        report = verify(full_square_certificate())
        self.assertEqual(report["maximum_pair_bias"], 0)
        self.assertEqual(report["pairs_failing_minority_necessary_condition"], 1)

    def test_tampered_pair_bias_is_rejected(self) -> None:
        document = copy.deepcopy(full_square_certificate())
        document["pair_biases"] = [-1]
        with self.assertRaisesRegex(ValueError, "pair-bias"):
            verify(document)

    def test_structure_report_audits_cross_union_image(self) -> None:
        report = analyze(full_square_certificate())
        pressure = report["strongest_pair_pressures"][0]
        self.assertEqual(pressure["cross_pairs"], 1)
        self.assertEqual(pressure["distinct_cross_unions"], 1)
        self.assertEqual(pressure["both_minus_neither"], 0)


if __name__ == "__main__":
    unittest.main()
