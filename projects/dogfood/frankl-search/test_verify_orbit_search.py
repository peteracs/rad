from __future__ import annotations

import copy
import unittest

from verify_orbit_search import orbit, verify


class OrbitCertificateTests(unittest.TestCase):
    def certificate(self) -> dict[str, object]:
        width = 4
        basis = [3, 5]
        generators = sorted({member for value in basis for member in orbit(value, width)})
        return {
            "width": width,
            "basis": basis,
            "generators": generators,
            "family_size": 12,
            "frequencies": [7, 7, 7, 7],
            "uniform": True,
            "margin": 2,
            "deleted_sets": 4,
            "deleted_rank_sum": 4,
            "dual_counterexample_pressure": -8,
            "missing_orbits": 1,
        }

    def test_regular_non_singleton_family_is_verified(self) -> None:
        result = verify(self.certificate())
        self.assertEqual(result["family_size"], 12)
        self.assertEqual(result["margin"], 2)
        self.assertFalse(result["counterexample"])

    def test_tampered_dual_rank_is_rejected(self) -> None:
        document = copy.deepcopy(self.certificate())
        document["deleted_rank_sum"] = 5
        with self.assertRaisesRegex(ValueError, "deleted rank sum mismatch"):
            verify(document)


if __name__ == "__main__":
    unittest.main()
