from __future__ import annotations

import io
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from tooling import check_line_limits


class LineLimitGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        subprocess.run(["git", "init", "--quiet"], cwd=self.root, check=True)
        (self.root / "tooling").mkdir()
        self.exceptions = Path("tooling/line-limit-exceptions.tsv")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def track(self, path: str, text: str) -> None:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        subprocess.run(["git", "add", "--", path], cwd=self.root, check=True)

    def write_exceptions(self, text: str) -> None:
        self.track(self.exceptions.as_posix(), text)

    def test_rejects_unjustified_oversized_text(self) -> None:
        self.track("large.txt", "line\n" * 4)
        self.write_exceptions("")
        self.assertEqual(check_line_limits.audit(self.root, 3, self.exceptions), 1)

    def test_rejects_untracked_nonignored_oversized_text(self) -> None:
        target = self.root / "untracked.txt"
        target.write_text("line\n" * 4, encoding="utf-8")
        self.write_exceptions("")
        self.assertEqual(check_line_limits.audit(self.root, 3, self.exceptions), 1)

    def test_accepts_one_exact_justified_exception(self) -> None:
        self.track("large.txt", "line\n" * 4)
        self.write_exceptions(
            "large.txt\tCanonical generated fixture must remain a single ordered file.\n"
        )
        self.assertEqual(check_line_limits.audit(self.root, 3, self.exceptions), 0)

    def test_rejects_stale_exception_after_refactor(self) -> None:
        self.track("small.txt", "line\n" * 2)
        self.write_exceptions(
            "small.txt\tThis once-large fixture was split and no longer needs an exception.\n"
        )
        self.assertEqual(check_line_limits.audit(self.root, 3, self.exceptions), 1)

    def test_binary_files_are_not_misclassified_as_text(self) -> None:
        self.track("asset.bin", "prefix\0payload")
        self.write_exceptions("")
        self.assertEqual(check_line_limits.audit(self.root, 1, self.exceptions), 0)

    def test_every_run_reminds_callers_to_review_srp_before_splitting(self) -> None:
        self.track("small.txt", "line\n")
        self.write_exceptions("")
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(check_line_limits.audit(self.root, 3, self.exceptions), 0)
        reminder = output.getvalue()
        expected_lessons = (
            "SRP review trigger, not an automatic split",
            "Read the whole file",
            "composition root, siblings, and call sites",
            "one exact-path",
            "Exceptions are better than fake splits",
            "split mechanically first",
            "complete the semantic refactor",
            "multiple consumers",
            "Keep the result DRY",
            "Never create quota/remainder/misc/numbered fragments",
            'misleading "A_and_B" buckets',
            "minify, compress formatting",
            "A small file is not automatically wrong",
            "Review the parent for SRP",
            "Do not balance modules by line count",
            "999 lines as a design goal",
            "names and boundaries must describe",
            "the domain, not extraction order",
            "Re-audit every new small file and its parent",
            "do not leave a tiny tail",
            "Preserve unrelated work",
            "remove any stale exception",
        )
        for lesson in expected_lessons:
            self.assertIn(lesson, reminder)


if __name__ == "__main__":
    unittest.main()
