#!/usr/bin/env python3
"""Sort the presentations no search trivialized into classes.

    python classify.py --rad <rad> [--max-cores 20] [--jobs 8] [--probe-ms 6000]

Reads atlas_results.tsv (latest line per presentation wins) and repeats:

1. among the presentations still UNSOLVED, take the first relator (the n)
   with the most left and its shortest presentation, and make that a class
   of the atlas (`atlas.rad grow`, one 20 s run); if a probe from it meets an
   existing class first, it is resolved instead;
2. probe every other UNSOLVED presentation with that first relator.

A presentation that meets a class is trivializable exactly when the class's
starting presentation is. Every chain is checked by verify.py before it
counts. New lines are appended to atlas_results.tsv; the classes and their
presentations are in atlas_classes.tsv.
"""
import argparse
import concurrent.futures
import pathlib
import subprocess
import sys

import atlas_sweep

HERE = pathlib.Path(__file__).resolve().parent
RESULTS = HERE / "atlas_results.tsv"


def latest():
    rows = {}
    for line in RESULTS.read_text(encoding="utf-8").splitlines():
        cells = line.split("\t")
        if len(cells) >= 4:
            rows[cells[0]] = cells
    return rows


def record(out, result):
    out.write("\t".join(result) + "\n")
    out.flush()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True)
    parser.add_argument("--max-cores", type=int, default=20)
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--probe-ms", type=int, default=6000)
    args = parser.parse_args()
    rad = str(pathlib.Path(args.rad).resolve())

    for round_number in range(args.max_cores):
        rows = latest()
        unsolved = [cells for cells in rows.values() if cells[3] in ("UNSOLVED", "TIMEOUT")]
        if not unsolved:
            print("nothing left unsolved")
            return
        # Joins have only ever happened between presentations with the same
        # first relator (the same n), so a round works on one n: the one with
        # the most presentations left, shortest presentation first.
        sizes = {}
        for cells in unsolved:
            sizes[cells[1]] = sizes.get(cells[1], 0) + 1
        busiest = max(sizes, key=lambda relator: (sizes[relator], -len(relator)))
        unsolved = [cells for cells in unsolved if cells[1] == busiest]
        unsolved.sort(key=lambda cells: (len(cells[2]), int(cells[0])))
        index, r1, r2 = unsolved[0][:3]
        label = f"ms{index}"
        print(f"round {round_number + 1}: {len(unsolved)} unsolved with first relator {r1}; core {label} <{r1}, {r2}>", flush=True)

        with open(RESULTS, "a", encoding="utf-8", newline="\n") as out:
            # A core that meets an existing class is resolved by a probe.
            first = atlas_sweep.probe_one(rad, (index, r1, r2), 8000, atlas_sweep.classes())
            if first[3] in ("TRIVIAL", "JOINED"):
                record(out, first)
                print(f"  core resolved: {first[3]} {first[4]}", flush=True)
                continue
            head, lines = atlas_sweep.grow(rad, label, r1, r2, 1, 20000)
            if head.startswith("JOINED"):
                # Growing the core reached another class: the core is resolved.
                core = atlas_sweep.check_chain((index, r1, r2), lines, head, atlas_sweep.classes())
                record(out, core)
                print(f"  core resolved while growing: {core[3]} {core[4]}", flush=True)
            else:
                record(out, (index, r1, r2, "CORE", label, "0", "", ""))
            table = atlas_sweep.classes()
            rest = [tuple(cells[:3]) for cells in unsolved[1:]]
            joined = 0
            with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
                for result in pool.map(lambda row: atlas_sweep.probe_one(rad, row, args.probe_ms, table), rest):
                    if result[3] in ("TRIVIAL", "JOINED", "REJECTED"):
                        record(out, result)
                        joined += result[3] != "REJECTED"
                        if result[3] == "REJECTED":
                            print(f"  REJECTED ms_{result[0]}", flush=True)
            print(f"  {joined} joined a class this round", flush=True)

    rows = latest()
    counts = {}
    for cells in rows.values():
        key = cells[3] + (f":{cells[4]}" if cells[4] else "")
        counts[key] = counts.get(key, 0) + 1
    print(" ".join(f"{name}={count}" for name, count in sorted(counts.items())))


if __name__ == "__main__":
    main()
