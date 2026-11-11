#!/usr/bin/env python3
"""Run ac.rad over the Miller-Schupp table and verify every path it prints.

    python sweep.py --rad <rad binary> [--only unsolved|solved|all] [--limit N]
                    [--cap 24] [--states 20000] [--jobs 8] [--from I] [--out results.tsv]

Each presentation gets one `rad ac.rad` process with a hard 30 s wall limit.
A SOLVED line counts only after verify.py accepts the path (separate process,
separate code). Results are appended to the output table, one line each:

    index  r1  r2  greedy_solved  status  steps  expanded  seen  ms

status is VERIFIED, UNSOLVED, TIMEOUT, or REJECTED (a path verify.py refused).
"""
import argparse
import concurrent.futures
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
WALL_LIMIT = 30


def run_one(rad, row, cap, states):
    index, r1, r2, greedy = row
    try:
        done = subprocess.run(
            [rad, str(HERE / "ac.rad"), "--", r1, r2, str(cap), str(states)],
            capture_output=True, text=True, timeout=WALL_LIMIT,
        )
    except subprocess.TimeoutExpired:
        return (index, r1, r2, greedy, "TIMEOUT", "", "", "", "")
    lines = [line for line in done.stdout.splitlines() if line.strip()]
    head = next((line for line in lines if line.startswith(("SOLVED", "UNSOLVED"))), "")
    fields = dict(part.split("=", 1) for part in head.split() if "=" in part)
    if not head.startswith("SOLVED"):
        status = "UNSOLVED" if head else "ERROR"
        return (index, r1, r2, greedy, status, "", fields.get("expanded", ""), fields.get("seen", ""), fields.get("ms", ""))
    path = "\n".join(lines[lines.index(head):]) + "\n"
    check = subprocess.run(
        [sys.executable, str(HERE / "verify.py"), r1, r2], input=path, capture_output=True, text=True,
    )
    status = "VERIFIED" if check.returncode == 0 else "REJECTED"
    if status == "VERIFIED":
        (HERE / "paths").mkdir(exist_ok=True)
        (HERE / "paths" / f"ms_{index}.txt").write_text(path, encoding="utf-8", newline="\n")
    return (index, r1, r2, greedy, status, fields.get("steps", ""), fields.get("expanded", ""), fields.get("seen", ""), fields.get("ms", ""))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True)
    parser.add_argument("--only", choices=["unsolved", "solved", "all"], default="all")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--from", dest="start", type=int, default=0)
    parser.add_argument("--cap", type=int, default=24)
    parser.add_argument("--states", type=int, default=20000)
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--out", default=str(HERE / "results.tsv"))
    args = parser.parse_args()
    rad = str(pathlib.Path(args.rad).resolve())

    rows = []
    for line in (HERE / "data" / "miller_schupp.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        index, r1, r2, greedy = line.split("\t")
        if int(index) < args.start:
            continue
        if args.only == "unsolved" and greedy == "1":
            continue
        if args.only == "solved" and greedy == "0":
            continue
        rows.append((index, r1, r2, greedy))
    if args.limit:
        rows = rows[: args.limit]

    counts = {}
    with open(args.out, "a", encoding="utf-8", newline="\n") as out:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            for result in pool.map(lambda row: run_one(rad, row, args.cap, args.states), rows):
                out.write("\t".join(result) + "\n")
                out.flush()
                counts[result[4]] = counts.get(result[4], 0) + 1
                if result[4] == "VERIFIED" and result[3] == "0":
                    print(f"NEW  ms_{result[0]}  <{result[1]}, {result[2]}>  steps={result[5]}", flush=True)
                if result[4] in ("REJECTED", "ERROR"):
                    print(f"{result[4]}  ms_{result[0]}  <{result[1]}, {result[2]}>", flush=True)
    print(" ".join(f"{name}={count}" for name, count in sorted(counts.items())), f"of {len(rows)}")


if __name__ == "__main__":
    main()
