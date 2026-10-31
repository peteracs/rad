#!/usr/bin/env python3
"""Runs the shadow-barrier experiments and writes out/report.json.

The relaxation runs past RAD's native-call deadline, so it is a batch tool.
RAD computes and audits the barriers; `shadow_walk` measures and certifies.
"""

import argparse
import json
import math
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
OUT = HERE / "out"
KERNELS = HERE.parent / "native-math-kernels"
ONE = 1 << 30
LADDER_LIMIT = 1 / (2 * math.log(2))
CERTIFY_MARGIN = 1 << 18
WITNESS_MARGIN = 1 << 18
CYCLE_REQUIREMENT = 0.5418
LAMBDA_MAX = 1.3316


def barrier_file(k):
    return OUT / f"barriers-k{k}.json"


def shift_of(lam):
    """Overstates log2(lambda), which only weakens the barriers."""
    return math.ceil(math.log2(lam) * ONE) + 1


class ShadowWalk:
    def __init__(self, binary):
        self.binary = binary

    def call(self, command, k, per_bit, top_bits, shift, *rest):
        arguments = [command, k, per_bit, top_bits, shift, barrier_file(k), *rest]
        done = subprocess.run(
            [self.binary, *map(str, arguments)],
            check=True,
            capture_output=True,
            text=True,
        )
        return json.loads(done.stdout)["receipt"]

    def ceiling(self, k, per_bit, top_bits=6, shift=0):
        return self.call("ceiling", k, per_bit, top_bits, shift)

    def certify(self, k, per_bit, top_bits, shift, requirement):
        return self.call("certify", k, per_bit, top_bits, shift, requirement)

    def checked_ceiling(self, k, per_bit, top_bits=6, shift=0):
        """The measured ceiling, bracketed by exact certificates either side."""
        found = self.ceiling(k, per_bit, top_bits, shift)
        value = found["ceiling"]
        above = self.certify(k, per_bit, top_bits, shift, value + CERTIFY_MARGIN)
        below = self.certify(k, per_bit, top_bits, shift, value - WITNESS_MARGIN)
        assert above["status"] == "certified", (k, per_bit, above["status"])
        assert below["status"] == "witness", (k, per_bit, below["status"])
        return found


def depth(tool):
    rows = []
    for k in [0, 3, 5, 7, 8, 9]:
        found = tool.checked_ceiling(k, 64)
        rows.append({"k": k, "ceiling": found["ceiling"] / ONE})
        print(f"depth k={k} H=64: {rows[-1]['ceiling']:.5f}, certified both sides")
    return rows


def lambdas(tool):
    rows = []
    for per_bit in [256, 1024]:
        for lam in [1.0, 1.2, LAMBDA_MAX]:
            found = tool.ceiling(5, per_bit, shift=shift_of(lam))
            row = {
                "per_bit": per_bit,
                "lambda": lam,
                "ceiling": found["ceiling"] / ONE,
                "requirement": CYCLE_REQUIREMENT * lam,
            }
            rows.append(row)
            print(f"lambda H={per_bit} {lam}: {row['ceiling']:.5f}")
    return rows


def matched(tool):
    rows = []
    for k, per_bit in [(5, 1024), (7, 512), (8, 256), (9, 128)]:
        barred = tool.ceiling(k, per_bit)
        free = tool.ceiling(0, per_bit)
        row = {
            "k": k,
            "per_bit": per_bit,
            "with_barriers": barred["ceiling"] / ONE,
            "without": free["ceiling"] / ONE,
            "cycle": [len(barred["cycle"]), barred["cycle_odd"]],
        }
        rows.append(row)
        drop = row["without"] - row["with_barriers"]
        print(f"matched k={k} H={per_bit}: drop {drop:.5f}")
    return rows


def refinement(tool):
    rows = []
    for per_bit in [256, 1024, 4096, 8192]:
        barred = tool.ceiling(5, per_bit, 3, shift_of(LAMBDA_MAX))
        free = tool.ceiling(0, per_bit, 3)
        row = {
            "per_bit": per_bit,
            "with_barriers": barred["ceiling"] / ONE,
            "without": free["ceiling"] / ONE,
            "cycle": [len(barred["cycle"]), barred["cycle_odd"]],
        }
        rows.append(row)
        print(f"refine H={per_bit}: {row['with_barriers']:.5f} / {row['without']:.5f}")
    return {"ladder_limit": LADDER_LIMIT, "rows": rows}


def escape_walk(tool):
    k, per_bit, requirement = 9, 64, 0.74
    found = tool.certify(k, per_bit, 6, 0, math.floor(requirement * ONE) - 1)
    assert found["status"] == "witness", found["status"]
    barriers = json.loads(barrier_file(k).read_text())
    bins = found["bins"]
    walk = found["witness"]
    pressed = 0
    for node in walk:
        bar = barriers[node // bins] / ONE
        if bar > 0 and bar > (node % bins) / per_bit - 0.02:
            pressed += 1
    odd = sum(1 for here, there in zip(walk, walk[1:] + walk[:1]) if there % bins > here % bins)
    print(f"escape walk: {len(walk)} steps, {odd} odd, {pressed} pressed against a barrier")
    return {"length": len(walk), "odd": odd, "pressed": pressed}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rad", default=REPO / "target" / "debug" / "rad", type=Path)
    arguments = parser.parse_args()
    OUT.mkdir(exist_ok=True)
    subprocess.run(
        ["cargo", "build", "--release", "--bin", "shadow_walk"],
        cwd=KERNELS,
        check=True,
    )
    barrier_program = HERE.relative_to(REPO) / "barrier.rad"
    subprocess.run(
        [arguments.rad, barrier_program, "--strict-types", "--deny-warnings"],
        cwd=REPO,
        check=True,
    )
    barrier_file(0).write_text("[0]")
    tool = ShadowWalk(KERNELS / "target" / "release" / "shadow_walk")
    report = {
        "depth": depth(tool),
        "lambda": lambdas(tool),
        "matched": matched(tool),
        "refinement": refinement(tool),
        "escape_walk": escape_walk(tool),
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
