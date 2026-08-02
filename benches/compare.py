"""Rust VM profile comparison: debug vs release.

Runs selected benchmark .rad files through Rust VM debug/release binaries,
measures wall-clock time (with process-startup overhead subtracted), and prints
a markdown table with speedup ratios.

Usage:
    py benches/compare.py [--runs N]
"""

import argparse
import os
import statistics
import subprocess
import sys
import tempfile
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXAMPLES_DIR = os.path.join(REPO_ROOT, "examples")

RUST_CLI_NAME = "rad.exe" if os.name == "nt" else "rad"
RUST_CLI = os.path.join(REPO_ROOT, "target", "release", RUST_CLI_NAME)
RUST_CLI_DEBUG = os.path.join(REPO_ROOT, "target", "debug", RUST_CLI_NAME)

BENCHMARK_FILES = [
    "sorting.rad",
    "pipeline.rad",
    "ecs_benchmark.rad",
    "demo.rad",
    "calculator.rad",
]

BASELINE_PROGRAM = "let x = 0\n"


def find_cli(path):
    return path if os.path.isfile(path) else None


def time_run(cmd, cwd, runs):
    times = []
    for _ in range(runs):
        start = time.perf_counter()
        proc = subprocess.run(
            cmd,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=120,
        )
        elapsed = time.perf_counter() - start
        if proc.returncode != 0:
            return None, proc.stderr[:200]
        times.append(elapsed)
    return times, None


def measure_baseline(cmd_template, baseline_file, cwd, runs):
    """Measure process-startup overhead using a trivial program."""
    times, err = time_run(cmd_template + [baseline_file], cwd, runs)
    if err:
        return 0.0
    return statistics.median(times)


def main():
    parser = argparse.ArgumentParser(description="Rad Rust VM profile benchmark")
    parser.add_argument("--runs", type=int, default=5, help="Number of runs per benchmark")
    args = parser.parse_args()

    rust_release = find_cli(RUST_CLI)
    rust_debug = find_cli(RUST_CLI_DEBUG)
    if rust_release is None or rust_debug is None:
        print(
            "ERROR: Missing Rust VM binaries. Build both with:",
            file=sys.stderr,
        )
        print("  cargo build -p rad-vm", file=sys.stderr)
        print("  cargo build -p rad-vm --release", file=sys.stderr)
        sys.exit(1)

    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".rad", delete=False, dir=REPO_ROOT
    ) as f:
        f.write(BASELINE_PROGRAM)
        baseline_path = f.name

    try:
        print(f"Rust debug:   {rust_debug}")
        print(f"Rust release: {rust_release}")
        print(f"Runs per benchmark: {args.runs}")
        print()

        dbg_base = measure_baseline(
            [rust_debug], baseline_path, REPO_ROOT, args.runs
        )
        rel_base = measure_baseline(
            [rust_release], baseline_path, REPO_ROOT, args.runs
        )
        print(
            "  Baseline (startup overhead): "
            f"debug={dbg_base:.4f}s  release={rel_base:.4f}s"
        )
        print()

        rows = []
        for name in BENCHMARK_FILES:
            path = os.path.join(EXAMPLES_DIR, name)
            if not os.path.isfile(path):
                print(f"  SKIP {name} (not found)")
                continue

            dbg_times, dbg_err = time_run(
                [rust_debug, path], REPO_ROOT, args.runs
            )
            if dbg_err:
                print(f"  FAIL {name} (debug): {dbg_err}")
                continue

            rel_times, rel_err = time_run([rust_release, path], REPO_ROOT, args.runs)
            if rel_err:
                print(f"  FAIL {name} (release): {rel_err}")
                continue

            dbg_exec = max(statistics.median(dbg_times) - dbg_base, 0.0001)
            rel_exec = max(statistics.median(rel_times) - rel_base, 0.0001)
            speedup = dbg_exec / rel_exec if rel_exec > 0 else float("inf")
            rows.append((name, dbg_exec, rel_exec, speedup))
            print(
                f"  {name}: debug={dbg_exec:.4f}s  "
                f"release={rel_exec:.4f}s  speedup={speedup:.1f}x"
            )

        if not rows:
            print("\nNo benchmarks completed.")
            return

        print("\n## Rust VM Profile Performance Comparison\n")
        print("*Process-startup overhead subtracted from all timings.*\n")
        print("| Example | Debug (s) | Release (s) | Speedup |")
        print("|---|---:|---:|---:|")
        for name, dbg_t, rel_t, sp in rows:
            print(f"| `{name}` | {dbg_t:.4f} | {rel_t:.4f} | **{sp:.1f}x** |")
        print()
    finally:
        os.unlink(baseline_path)


if __name__ == "__main__":
    main()
