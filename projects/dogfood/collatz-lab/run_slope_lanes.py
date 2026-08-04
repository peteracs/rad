#!/usr/bin/env python3
"""Run exact RAD lane programs concurrently with durable atomic outputs."""

from __future__ import annotations

import argparse
import concurrent.futures
import subprocess
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", default="target/release/rad.exe")
    parser.add_argument("--support", type=int, default=11)
    parser.add_argument("--depth", type=int, default=945)
    parser.add_argument("--lanes", type=int, default=256)
    parser.add_argument("--workers", type=int, default=16)
    parser.add_argument(
        "--output", default="projects/dogfood/collatz-lab/out/slope11-lanes"
    )
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    rad = (repo / args.rad).resolve()
    program = Path(__file__).with_name("slope_lane.rad")
    output = (repo / args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)

    def run_lane(lane: int) -> int:
        target = output / f"lane-{lane:04d}.json"
        if target.exists():
            return lane
        process = subprocess.run(
            [
                str(rad),
                str(program),
                "--",
                str(args.support),
                str(args.depth),
                str(lane),
                str(args.lanes),
            ],
            cwd=repo,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        if process.returncode != 0:
            raise RuntimeError(f"exact slope lane {lane} failed:\n{process.stdout}")
        temporary = target.with_suffix(".json.partial")
        temporary.write_text(process.stdout, encoding="utf-8")
        temporary.replace(target)
        return lane

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = [executor.submit(run_lane, lane) for lane in range(args.lanes)]
        for completed, future in enumerate(concurrent.futures.as_completed(futures), 1):
            lane = future.result()
            print(f"completed lane {lane}/{args.lanes - 1} ({completed}/{args.lanes})")

    verifier = Path(__file__).with_name("verify_slope_lanes.py")
    return subprocess.call(
        [
            sys.executable,
            str(verifier),
            str(output),
            str(args.support),
            str(args.depth),
            str(args.lanes),
        ],
        cwd=repo,
    )


if __name__ == "__main__":
    raise SystemExit(main())
