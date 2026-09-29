"""Recompute RAD's exponent portfolio with independent rational formulas."""
from fractions import Fraction
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def main() -> None:
    root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", type=Path, default=root / "target/debug/rad.exe")
    args = parser.parse_args()
    command = [str(args.rad.resolve()),
               "projects/dogfood/navier-stokes/core_scale_screen.rad",
               "--experimental-laws", "--strict-types", "--deny-warnings"]
    process = subprocess.run(command, cwd=root, check=True, capture_output=True,
                             text=True, encoding="utf-8", env={**os.environ, "RAYON_NUM_THREADS": "1"})
    with tempfile.TemporaryDirectory(prefix="rad-fluid-replay-") as directory:
        trace = Path(directory) / "core-scale.radr"
        parallel = subprocess.run(command + ["--record", str(trace)], cwd=root,
                                  check=True, capture_output=True, text=True, encoding="utf-8",
                                  env={**os.environ, "RAYON_NUM_THREADS": "4"})
        require(parallel.stdout == process.stdout, "worker count or recording changed output")
        replay = subprocess.run([str(args.rad.resolve()), "replay", str(trace)], cwd=root,
                                check=True, capture_output=True, text=True, encoding="utf-8")
        require("Replay verified: world digest matches the recorded run" in replay.stderr,
                "recorded world did not verify on replay")
    rows = [json.loads(line) for line in process.stdout.splitlines() if line.startswith("{")]
    expected = [(r,s) for r in [16,32,48,64] for s in range(1,177)]
    require([(row["radius"],row["strain"]) for row in rows] == expected, "portfolio coverage mismatch")
    count = 0
    for row in rows:
        r, s = Fraction(row["radius"],16), Fraction(row["strain"],16)
        for name, value in [("exposure",2*r-s),("energy",2*s-5*r),("loss",s-3*r)]:
            require(Fraction(row[name],16) == value, f"wrong {name} exponent")
        feasible = 2*r < s < Fraction(5,2)*r
        require(row["feasible_scaling"] == feasible, "wrong strict-window decision")
        beta = r/s
        excluded = Fraction(2,5) < beta < Fraction(1,2)
        require(row["fixed_profile_excluded"] == excluded, "wrong fixed-profile classification")
        count += feasible
    require(2*Fraction(47,16)-Fraction(1,16) == Fraction(93,16), "published schedule arithmetic")
    summaries = [json.loads(line[len("portfolio "):]) for line in process.stdout.splitlines()
                 if line.startswith("portfolio ")]
    require(summaries == [{"cases": 704, "scale_feasible": count, "fixed_profile_excluded": count}],
            "causal aggregate differs from independent classification")
    require("AssembleCores" in process.stdout and "SubmitCore" in process.stdout,
            "expected causal provenance missing")
    for probe, diagnostic in [("duplicate", "duplicate candidate evidence"),
                              ("forged", "forged candidate evidence"),
                              ("missing", "incomplete proposal portfolio")]:
        rejected = subprocess.run(command + ["--", probe], cwd=root,
                                  capture_output=True, text=True, encoding="utf-8")
        require(rejected.returncode != 0, f"{probe} corruption was accepted")
        require(diagnostic in rejected.stdout + rejected.stderr,
                f"{probe} failed for an unrelated reason")
    print(f"Verified {len(rows)} RAD outputs; {count} satisfy the necessary strict scale window.")
    print("Rejected duplicate, forged and incomplete causal evidence.")
    print("One/four-worker output matched; recorded world digest verified on replay.")
    print("No PDE existence, localization or force cancellation certified.")


if __name__ == "__main__":
    main()
