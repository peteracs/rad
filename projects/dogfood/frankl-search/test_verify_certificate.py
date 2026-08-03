from __future__ import annotations

import copy
import unittest

from verify_certificate import VerificationError, verify


def valid_document() -> dict:
    # P({0, 1}) plus the full three-element set.
    return {
        "schema": "rad.frankl-search-certificate.v1",
        "universe_size": 3,
        "full_set_mask": 7,
        "generators": [7, 1, 2],
        "family": [0, 1, 2, 3, 7],
        "frequencies": [3, 3, 1],
        "member_sets": 5,
        "max_frequency": 3,
        "frankl_holds": True,
        "counterexample": False,
        "union_closed": True,
        "covers_universe": True,
        "basis_irredundant": True,
        "separating": True,
        "live_world_unchanged": True,
        "result_world_digest": "ab" * 32,
    }


class CertificateTests(unittest.TestCase):
    def test_accepts_independently_valid_family(self) -> None:
        report = verify(valid_document())
        self.assertTrue(report["union_closed"])
        self.assertFalse(report["counterexample"])

    def test_rejects_missing_union(self) -> None:
        document = valid_document()
        document["family"] = [0, 1, 2, 7]
        document["member_sets"] = 4
        with self.assertRaises(VerificationError):
            verify(document)

    def test_rejects_forged_frequency_claim(self) -> None:
        document = copy.deepcopy(valid_document())
        document["frequencies"][0] = 2
        with self.assertRaises(VerificationError):
            verify(document)

    def test_rejects_redundant_basis(self) -> None:
        document = valid_document()
        document["generators"].append(3)
        with self.assertRaisesRegex(VerificationError, "not irredundant"):
            verify(document)


if __name__ == "__main__":
    unittest.main()
