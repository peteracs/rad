"""Run RAD, replay it across worker counts, and independently verify the math."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time

from verify import read_rows, verify_rows
from symbolic_verify import verify as verify_symbolic
from backward_oracle import verify_rows as verify_weighted
from capacity_oracle import verify_rows as verify_capacity

PROJECT = Path(__file__).resolve().parent
ROOT = PROJECT.parents[2]


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(command, output, workers):
    start = time.perf_counter_ns()
    result = subprocess.run([str(arg) for arg in command], cwd=ROOT,
                            env={**os.environ, "RAYON_NUM_THREADS": str(workers)},
                            text=True, encoding="utf-8", errors="replace",
                            capture_output=True, timeout=300)
    elapsed = time.perf_counter_ns() - start
    output.write_text(result.stdout, encoding="utf-8")
    output.with_suffix(".stderr.txt").write_text(result.stderr, encoding="utf-8")
    if result.returncode:
        raise RuntimeError(f"command failed: {command}\n{result.stderr}\n{result.stdout}")
    return result, {"command": [str(arg) for arg in command], "workers": workers,
                    "elapsed_ns": elapsed, "stdout_sha256": sha256(output)}


def main():
    parser = argparse.ArgumentParser()
    executable = "rad.exe" if os.name == "nt" else "rad"
    parser.add_argument("--rad", type=Path, default=ROOT / "target/release" / executable)
    parser.add_argument("--max-n", type=int, default=10)
    parser.add_argument("--weighted-max-n", type=int, default=30)
    parser.add_argument("--output", type=Path, default=PROJECT / "out")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    commands = []
    reference = None
    for workers in (1, 4):
        trace = args.output / f"search-{workers}.radr"
        stdout = args.output / f"search-{workers}.jsonl"
        print(f"RAD exact search: n=2..{args.max_n}, workers={workers}", flush=True)
        _, receipt = run([args.rad, PROJECT / "main.rad", "--strict-types", "--deny-warnings",
                          "--record", trace, "--", args.max_n], stdout, workers)
        commands.append(receipt)
        rows = verify_rows(read_rows(stdout), args.max_n)
        if reference is not None and rows != reference:
            raise RuntimeError("worker-count change altered the mathematical results")
        reference = rows
        result, receipt = run([args.rad, "replay", trace],
                              args.output / f"replay-{workers}.txt", 5 - workers)
        commands.append(receipt)
        if "Replay verified: world digest matches" not in result.stdout + result.stderr:
            raise RuntimeError("replay omitted final digest verification")
        print(f"independently verified {len(rows)} instances and replay", flush=True)
    trace = args.output / "proof-audit.radr"
    _, receipt = run([args.rad, PROJECT / "proof_audit.rad", "--experimental-laws",
                      "--strict-types", "--deny-warnings", "--record", trace],
                     args.output / "proof-audit.txt", 4)
    commands.append(receipt)
    result, receipt = run([args.rad, "replay", trace], args.output / "proof-replay.txt", 1)
    commands.append(receipt)
    if "Replay verified: world digest matches" not in result.stdout + result.stderr:
        raise RuntimeError("proof replay omitted final digest verification")
    print(f"RAD backward weighted search: n=3..{args.weighted_max_n}", flush=True)
    weighted_output = args.output / "optimality.jsonl"
    _, weighted_receipt = run([args.rad, PROJECT / "optimality.rad", "--strict-types", "--deny-warnings",
                      "--", args.weighted_max_n], weighted_output, 4)
    commands.append(weighted_receipt)
    weighted_rows = verify_weighted(read_rows(weighted_output), args.weighted_max_n)
    print("RAD general power-rank capacity: all maps on 2..4 states, budgets 0..3", flush=True)
    capacity_output = args.output / "rank-capacity.jsonl"
    _, capacity_receipt = run([args.rad, PROJECT / "rank_capacity.rad", "--strict-types", "--deny-warnings"],
                     capacity_output, 1)
    commands.append(capacity_receipt)
    capacity_rows = verify_capacity(read_rows(capacity_output))
    symbolic = verify_symbolic(args.output / "symbolic")
    common = {
        "completed_utc": datetime.now(timezone.utc).isoformat(),
        "source_sha256": {p.name: sha256(p) for p in sorted(PROJECT.iterdir())
                          if p.suffix in {".rad", ".py"}},
        "mathematical_text_sha256": {name: sha256(PROJECT / name)
                                     for name in ("RESULTS.md", "OPTIMALITY.md", "GENERAL_BOUND.md")},
        "binary_sha256": sha256(args.rad), "platform": platform.platform(),
        "python": platform.python_version(),
    }
    artifacts = {
        "optimality-evidence.json": {
            **common, "format": "rad-weighted-optimality-v1", "max_n": args.weighted_max_n,
            "command": weighted_receipt, "instances": weighted_rows,
            "scope": "Finite weighted searches; all-size proof is OPTIMALITY.md; priority unverified",
        },
        "capacity-evidence.json": {
            **common, "format": "rad-general-capacity-v1", "command": capacity_receipt,
            "instances": capacity_rows, "total_chain_cases": sum(row["checks"] for row in capacity_rows),
            "scope": "All maps n=2..4, k=0..3, all nested chains; general proof is GENERAL_BOUND.md",
        },
    }
    for name, artifact in artifacts.items():
        (args.output / name).write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    report = {
        **common, "format": "rad-deletion-sync-evidence-v2", "max_n": args.max_n,
        "commands": commands,
        "additional_evidence_sha256": {name: sha256(args.output / name) for name in artifacts},
        "trace_sha256": {name: sha256(args.output / name)
                         for name in ("search-1.radr", "search-4.radr", "proof-audit.radr")},
        "instances": reference,
        "symbolic_verification": symbolic,
        "scope": {"finite_optimality": f"2 <= n <= {args.max_n}",
                  "universal_one_deletion_upper_bound": "n^2 for n >= 3; see RESULTS.md",
                  "universal_two_deletion_impossibility": "n >= 3; see RESULTS.md",
                  "universal_n_squared_optimality": "proved for n >= 3; see OPTIMALITY.md",
                  "universal_weighted_optimality": "alpha*((n-1)^2+1)+beta*(2n-2); nonnegative costs",
                  "universal_local_capacity": "n-rank(f^(k+1)); upper bound and sharpness proved in GENERAL_BOUND.md",
                  "historical_novelty": "not established"},
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"PASS: {len(reference)} forward and {len(weighted_rows)} weighted exact cases, "
          f"{sum(row['checks'] for row in capacity_rows)} general capacity cases, three replays", flush=True)


if __name__ == "__main__":
    main()
