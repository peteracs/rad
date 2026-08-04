from __future__ import annotations

import json
import unittest
from pathlib import Path

from verify_generator8_frontier import verify as verify_generator8
from verify_generator8_cnf_exclusions import verify as verify_generator8_cnf
from verify_generator_frontier import verify as verify_generator7
from verify_projected_partition_frontier import verify as verify_projected


ROOT = Path(__file__).parent


class GeneratorFrontierTests(unittest.TestCase):
    def test_complete_seven_generator_certificate(self) -> None:
        document = json.loads(
            (ROOT / "certificates/generator-frontier-n13.json").read_text(encoding="utf-8")
        )
        report = verify_generator7(document)
        self.assertEqual(report["covered_labelled_configurations"], 191_718_188)
        self.assertEqual(report["minimum_margin"], 18)

    def test_eight_generator_graph_certificate(self) -> None:
        document = json.loads(
            (ROOT / "certificates/generator8-graph-frontier.json").read_text(encoding="utf-8")
        )
        report = verify_generator8(document)
        self.assertEqual(report["coloured_graph_orbits"], 2_038_236)
        self.assertEqual(report["smallest_family_size"], 80)
        self.assertEqual(report["minimum_margin"], 20)

    def test_projected_partition_frontiers(self) -> None:
        expected_orbits = [39, 23, 11, 6, 3, 2, 1]
        for threshold, expected in zip(range(26, 33), expected_orbits, strict=True):
            report = verify_projected(
                ROOT
                / "certificates"
                / f"projected-partition-v5-w3-q{threshold}.json"
            )
            self.assertEqual(report["threshold"], threshold)
            self.assertEqual(report["orbits"], expected)

    def test_eight_generator_q13_exclusion_manifest(self) -> None:
        document = verify_generator8_cnf(
            ROOT / "certificates" / "generator8-q13-exclusions.json"
        )
        self.assertEqual(document["solver_runs"], 82)
        self.assertEqual(document["excluded_triple_counts"], list(range(1, 14)))
        self.assertTrue(document["complete_triple_count_coverage"])
        self.assertFalse(document["counterexample_exists"])


if __name__ == "__main__":
    unittest.main()
