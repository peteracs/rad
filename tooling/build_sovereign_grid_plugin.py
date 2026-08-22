#!/usr/bin/env python3
"""Build Sovereign Grid's native oracle and publish one OS-neutral test path."""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "projects/dogfood/sovereign-grid/plugins/grid-oracle/Cargo.toml"
TARGET = ROOT / "target/sovereign-grid-plugin"
OUTPUT = TARGET / "grid-oracle.radext"


def main() -> int:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "--manifest-path",
            str(MANIFEST),
            "--target-dir",
            str(TARGET),
            "--release",
            "-j",
            "1",
        ],
        cwd=ROOT,
    )
    if completed.returncode:
        return completed.returncode
    if sys.platform == "win32":
        library = TARGET / "release/sovereign_grid_oracle.dll"
    elif sys.platform == "darwin":
        library = TARGET / "release/libsovereign_grid_oracle.dylib"
    else:
        library = TARGET / "release/libsovereign_grid_oracle.so"
    if not library.is_file():
        raise RuntimeError(f"plugin output is missing: {library}")
    shutil.copyfile(library, OUTPUT)
    print(OUTPUT.relative_to(ROOT))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
