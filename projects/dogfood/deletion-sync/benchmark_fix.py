"""Compare identical search inputs and outputs across two explicitly named builds."""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--samples", type=int, default=3)
    args = parser.parse_args()
    project = Path(__file__).resolve().parent
    receipts = {"before": [], "after": []}
    expected = None
    for _ in range(args.samples):
        for label, binary in (("before", args.before), ("after", args.after)):
            start = time.perf_counter_ns()
            completed = subprocess.run([str(binary), str(project / "single.rad"),
                                        "--strict-types", "--deny-warnings", "--", "8", "2"],
                                       text=True, capture_output=True, timeout=60)
            elapsed = time.perf_counter_ns() - start
            if completed.returncode:
                raise RuntimeError(completed.stdout + completed.stderr)
            row = json.loads(completed.stdout)
            if expected is not None and row != expected:
                raise RuntimeError("optimization changed the exact mathematical output")
            expected = row
            receipts[label].append(elapsed)
    report = {"samples_ns": receipts, "result": expected,
              "median_speedup": statistics.median(receipts["before"]) / statistics.median(receipts["after"]),
              "binary_sha256": {label: hashlib.sha256(path.read_bytes()).hexdigest()
                                for label, path in (("before", args.before), ("after", args.after))},
              "source_sha256": {name: hashlib.sha256((project / name).read_bytes()).hexdigest()
                                for name in ("search.rad", "single.rad")}}
    (project / "out/compiler-fix-benchmark.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
