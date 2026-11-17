#!/usr/bin/env python3
"""Run ac_explore.rad over table rows and verify every path it prints.

    python explore_sweep.py --rad <rad binary> [--rows pending|all|565,566]
                            [--strategy descent|level] [--worlds 400000]
                            [--max-total 40] [--jobs 1] [--wall 900]
                            [--out explore_results.tsv]

`pending` (the default) is every presentation with no verified result yet, as
`summarize.py --pending` lists them. The budget is a number of worlds, so a
run gives the same answer on any machine; `--wall` is only a guard against a
run that does not come back.

`descent` uses a Whitehead automorphism only when it shortens the pair, as
ac_levels.rad does. `level` also uses the ones that keep its length.

A SOLVED line counts only after verify.py accepts the path (separate process,
separate code). One line per presentation is appended to the output table:

    index  r1  r2  status  steps  expanded  seen  ms  worlds  max_total  strategy

status is VERIFIED, UNSOLVED, TIMEOUT, ERROR, or REJECTED (a path verify.py
refused). Verified paths are written to paths/ms_<index>.txt, and a verified
row is also appended to results.tsv in sweep.py's format, which is the table
summarize.py, novelty.py and export.py read.
"""
import argparse
import concurrent.futures
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent


def table():
    rows, greedy = {}, {}
    for line in (HERE / "data" / "miller_schupp.tsv").read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            index, r1, r2, solved = line.split("\t")
            rows[index] = (index, r1, r2)
            greedy[index] = solved
    return rows, greedy


def pending():
    listed = subprocess.run(
        [sys.executable, str(HERE / "summarize.py"), "--pending"],
        capture_output=True, text=True, check=True,
    )
    return [line.split("\t")[0] for line in listed.stdout.splitlines() if line.strip()]


def run_one(rad, row, worlds, max_total, wall, strategy):
    index, r1, r2 = row
    tail = (str(worlds), str(max_total), strategy)
    try:
        done = subprocess.run(
            [rad, str(HERE / "ac_explore.rad"), "--", "solve", r1, r2, str(worlds), str(max_total), strategy],
            capture_output=True, text=True, timeout=wall,
        )
    except subprocess.TimeoutExpired:
        return (index, r1, r2, "TIMEOUT", "", "", "", "") + tail
    lines = [line for line in done.stdout.splitlines() if line.strip()]
    head = next((line for line in lines if line.startswith(("SOLVED", "UNSOLVED"))), "")
    fields = dict(part.split("=", 1) for part in head.split() if "=" in part)
    counts = (fields.get("expanded", ""), fields.get("seen", ""), fields.get("ms", ""))
    if not head.startswith("SOLVED"):
        return (index, r1, r2, "UNSOLVED" if head else "ERROR", "") + counts + tail
    path = "\n".join(lines[lines.index(head):]) + "\n"
    check = subprocess.run(
        [sys.executable, str(HERE / "verify.py"), r1, r2], input=path, capture_output=True, text=True,
    )
    # `followed` is Rad's own replay of the path; verify.py is the outside one.
    accepted = check.returncode == 0 and fields.get("followed") == "true"
    if accepted:
        (HERE / "paths").mkdir(exist_ok=True)
        (HERE / "paths" / f"ms_{index}.txt").write_text(path, encoding="utf-8", newline="\n")
    return (index, r1, r2, "VERIFIED" if accepted else "REJECTED", fields.get("steps", "")) + counts + tail


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True)
    parser.add_argument("--rows", default="pending")
    parser.add_argument("--worlds", type=int, default=400000)
    parser.add_argument("--max-total", type=int, default=40)
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--wall", type=int, default=900)
    parser.add_argument("--strategy", choices=["descent", "level"], default="descent")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--out", default=str(HERE / "explore_results.tsv"))
    args = parser.parse_args()
    rad = str(pathlib.Path(args.rad).resolve())

    rows, greedy = table()
    if args.rows == "pending":
        wanted = pending()
    elif args.rows == "all":
        wanted = list(rows)
    else:
        wanted = args.rows.split(",")
    chosen = [rows[index] for index in wanted]
    if args.limit:
        chosen = chosen[: args.limit]

    counts = {}
    with open(args.out, "a", encoding="utf-8", newline="\n") as out:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            work = lambda row: run_one(rad, row, args.worlds, args.max_total, args.wall, args.strategy)
            for result in pool.map(work, chosen):
                out.write("\t".join(result) + "\n")
                out.flush()
                if result[3] == "VERIFIED":
                    # index r1 r2 greedy status steps expanded seen ms
                    line = result[:3] + (greedy[result[0]], "VERIFIED") + result[4:8]
                    with open(HERE / "results.tsv", "a", encoding="utf-8", newline="\n") as table_out:
                        table_out.write("\t".join(line) + "\n")
                counts[result[3]] = counts.get(result[3], 0) + 1
                if result[3] != "UNSOLVED":
                    print(f"{result[3]}  ms_{result[0]}  <{result[1]}, {result[2]}>  steps={result[4]} seen={result[6]} ms={result[7]}", flush=True)
    print(" ".join(f"{name}={count}" for name, count in sorted(counts.items())), f"of {len(chosen)}")


if __name__ == "__main__":
    main()
