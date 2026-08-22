from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

from tooling import check_architecture


class ArchitectureGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        subprocess.run(["git", "init", "--quiet"], cwd=self.root, check=True)
        self.write("core/vm/Cargo.toml", "[package]\nname = \"core\"\n")
        self.write("docs/rfcs/0001.md", "# RFC\n")
        self.write("docs/src/rfcs/0001.md", "{{#include ../../rfcs/0001.md}}\n")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write(self, path: str, text: str) -> None:
        self.write_untracked(path, text)
        subprocess.run(["git", "add", "--", path], cwd=self.root, check=True)

    def write_untracked(self, path: str, text: str) -> None:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    def test_accepts_authority_oriented_layout(self) -> None:
        self.write("adapters/cli/src/main.rs", "fn main() {}\n")
        self.write("core/relation/src/lib.rs", "// authoritative relation frontend\n")
        self.write("projects/moba/kit/main.rad", "fn main() -> nil {}\n")
        self.assertEqual(check_architecture.audit(self.root), [])

    def test_rejects_non_authoritative_code_under_core(self) -> None:
        self.write("core/simcore/src/lib.rs", "// project code\n")
        errors = check_architecture.audit(self.root)
        self.assertTrue(any("core/ is reserved" in error for error in errors))

    def test_rejects_untracked_non_authoritative_code_under_core(self) -> None:
        self.write_untracked("core/simcore/src/lib.rs", "// project code\n")
        errors = check_architecture.audit(self.root)
        self.assertTrue(any("core/ is reserved" in error for error in errors))

    def test_rejects_mechanical_split_directory(self) -> None:
        self.write("core/vm/src/lexer_sections/tail.rs", "// split tail\n")
        errors = check_architecture.audit(self.root)
        self.assertTrue(any("file-splitting mechanism" in error for error in errors))

    def test_rejects_core_dependency_on_adapter(self) -> None:
        self.write(
            "core/vm/Cargo.toml",
            "[dependencies]\nrad-lsp = { path = \"../../adapters/lsp\" }\n",
        )
        errors = check_architecture.audit(self.root)
        self.assertTrue(any("core must not depend" in error for error in errors))

    def test_rejects_relation_core_dependency_on_project(self) -> None:
        self.write(
            "core/relation/Cargo.toml",
            "[dependencies]\napp = { path = \"../../projects/app\" }\n",
        )
        errors = check_architecture.audit(self.root)
        self.assertTrue(any("core must not depend" in error for error in errors))

    def test_rejects_independently_editable_rfc_copy(self) -> None:
        self.write("docs/src/rfcs/0001.md", "# copied RFC\n")
        errors = check_architecture.audit(self.root)
        self.assertTrue(any("must be exactly" in error for error in errors))

    def test_rejects_incomplete_folder_tree_when_present(self) -> None:
        self.write(
            "docs/src/project/folder_tree.md",
            "```text\n  - `core/` - Core.\n```\n",
        )
        errors = check_architecture.audit(self.root)
        self.assertTrue(any("missing repository directory" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
