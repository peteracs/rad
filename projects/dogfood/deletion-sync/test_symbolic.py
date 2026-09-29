"""The proof receipt must hash actual file bytes, and reject altered artifacts."""

from pathlib import Path
import tempfile
import unittest

from symbolic_verify import check_artifacts, verify


class SymbolicArtifactTests(unittest.TestCase):
    def test_saved_hashes_and_mutated_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            report = verify(output)
            self.assertEqual(sum(row["result"] == "unsat" for row in report["obligations"]), 21)
            self.assertEqual(sum(row["result"] == "sat" for row in report["obligations"]), 2)
            check_artifacts(report, output)
            for suffix in ("smt2", "proof"):
                artifact = output / (report["obligations"][0]["name"] + "." + suffix)
                original = artifact.read_bytes()
                artifact.write_bytes(original + b"\n; altered artifact\n")
                with self.assertRaises(ValueError):
                    check_artifacts(report, output)
                artifact.write_bytes(original)
            check_artifacts(report, output)


if __name__ == "__main__":
    unittest.main()
