#!/usr/bin/env python3
"""Writes one trapped_survival receipt per bit length.

A mark is a step count (`4000`) or a multiple of the bit length (`3L`). Receipts
already holding the requested marks are kept. After changing the kernel, pass
--force.
"""

import argparse
import json
import subprocess
from pathlib import Path

KERNELS = Path(__file__).resolve().parent


def resolve(mark, bits):
    return str(int(mark[:-1]) * bits) if mark.endswith("L") else mark


def is_current(receipt, marks):
    """A receipt is a pure function of its name and marks; reuse a matching one."""
    if not receipt.exists():
        return False
    found = [row["mark"] for row in json.loads(receipt.read_text())["marks"]]
    return found == [int(mark) for mark in marks.split(",")]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--d", default=1, type=int)
    parser.add_argument("--bits", required=True, nargs="+", type=int)
    parser.add_argument("--marks", required=True, nargs="+")
    parser.add_argument("--force", action="store_true", help="recompute current receipts")
    arguments = parser.parse_args()
    arguments.out.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cargo", "build", "--release", "--bin", "trapped_survival"],
        cwd=KERNELS,
        check=True,
    )
    tool = KERNELS / "target" / "release" / "trapped_survival"
    for bits in arguments.bits:
        marks = ",".join(resolve(mark, bits) for mark in arguments.marks)
        receipt = arguments.out / f"survival-b{bits}-d{arguments.d}.json"
        if not arguments.force and is_current(receipt, marks):
            print(f"{receipt}: current")
            continue
        subprocess.run([tool, str(bits), str(arguments.d), marks, receipt], check=True)
        print(receipt)


if __name__ == "__main__":
    main()
