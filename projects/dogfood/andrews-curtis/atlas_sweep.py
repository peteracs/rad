#!/usr/bin/env python3
"""Grow classes in the atlas and probe presentations against it.

    python atlas_sweep.py --rad <rad> grow <label> <r1> <r2> [--rounds 3]
    python atlas_sweep.py --rad <rad> probe [--from-results results.tsv] [--jobs 8]

`grow` runs `rad atlas.rad -- grow` a number of times; each run is under 30 s
and continues where the last one stopped, because the search state is the
saved world (atlas.radw). The label and its presentation go into
atlas_classes.tsv.

`probe` takes every presentation that results.tsv lists as UNSOLVED and
searches from it until it reaches a state of the atlas. A JOINED line counts
only after verify.py accepts the printed chain between the presentation and
the start of the class it met. Results go to atlas_results.tsv:

    index  r1  r2  status  class  steps  visits  ms

status is TRIVIAL (met class T), JOINED (met another class: trivializable
exactly when that class is), UNSOLVED, TIMEOUT or REJECTED.
"""
import argparse
import concurrent.futures
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ATLAS = HERE / "atlas.radw"
CLASSES = HERE / "atlas_classes.tsv"
WALL_LIMIT = 30


def classes():
    table = {"T": ("x", "y")}
    if CLASSES.exists():
        for line in CLASSES.read_text(encoding="utf-8").splitlines():
            if line.strip() and not line.startswith("#"):
                label, r1, r2 = line.split("\t")
                table[label] = (r1, r2)
    return table


def grow(rad, label, r1, r2, rounds, wall_ms):
    table = classes()
    if label not in table:
        with open(CLASSES, "a", encoding="utf-8", newline="\n") as out:
            out.write(f"{label}\t{r1}\t{r2}\n")
    for _ in range(rounds):
        done = subprocess.run(
            [rad, str(HERE / "atlas.rad"), "--", "grow", str(ATLAS), label, r1, r2, str(wall_ms)],
            capture_output=True, text=True, timeout=WALL_LIMIT,
        )
        lines = [line for line in done.stdout.splitlines() if line.strip()]
        head = next((line for line in lines if line.startswith(("GROWN", "JOINED", "KNOWN"))), "no result")
        print(head, flush=True)
        if not head.startswith("GROWN"):
            if head.startswith("JOINED"):
                (HERE / "paths").mkdir(exist_ok=True)
                (HERE / "paths" / f"class_{label}.txt").write_text(
                    "\n".join(lines[lines.index(head):]) + "\n", encoding="utf-8", newline="\n")
            break


def probe_one(rad, row, wall_ms, table):
    index, r1, r2 = row
    try:
        done = subprocess.run(
            [rad, str(HERE / "atlas.rad"), "--", "probe", str(ATLAS), r1, r2, str(wall_ms)],
            capture_output=True, text=True, timeout=WALL_LIMIT,
        )
    except subprocess.TimeoutExpired:
        return (index, r1, r2, "TIMEOUT", "", "", "", "")
    lines = [line for line in done.stdout.splitlines() if line.strip()]
    head = next((line for line in lines if line.startswith(("JOINED", "UNSOLVED"))), "")
    fields = dict(part.split("=", 1) for part in head.split() if "=" in part)
    if not head.startswith("JOINED"):
        return (index, r1, r2, "UNSOLVED" if head else "ERROR", "", "", fields.get("visits", ""), fields.get("ms", ""))
    label = head.split(" meets class ")[1].split()[0]
    chain = "\n".join(lines[lines.index(head):]) + "\n"
    target = table.get(label)
    if target is None:
        return (index, r1, r2, "REJECTED", label, "", fields.get("visits", ""), fields.get("ms", ""))
    command = [sys.executable, str(HERE / "verify.py"), r1, r2]
    if label != "T":
        command += ["--to", target[0], target[1]]
    check = subprocess.run(command, input=chain, capture_output=True, text=True)
    if check.returncode != 0:
        return (index, r1, r2, "REJECTED", label, "", fields.get("visits", ""), fields.get("ms", ""))
    (HERE / "paths").mkdir(exist_ok=True)
    name = f"ms_{index}.txt" if label == "T" else f"ms_{index}_to_{label}.txt"
    (HERE / "paths" / name).write_text(chain, encoding="utf-8", newline="\n")
    status = "TRIVIAL" if label == "T" else "JOINED"
    return (index, r1, r2, status, label, str(len(lines) - lines.index(head) - 2), fields.get("visits", ""), fields.get("ms", ""))


def probe(rad, results, jobs, wall_ms, out_path):
    table = classes()
    rows = []
    for line in pathlib.Path(results).read_text(encoding="utf-8").splitlines():
        cells = line.split("\t")
        if len(cells) >= 5 and cells[4] in ("UNSOLVED", "TIMEOUT"):
            rows.append((cells[0], cells[1], cells[2]))
    counts = {}
    with open(out_path, "a", encoding="utf-8", newline="\n") as out:
        with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
            for result in pool.map(lambda row: probe_one(rad, row, wall_ms, table), rows):
                out.write("\t".join(result) + "\n")
                out.flush()
                key = result[3] + (f":{result[4]}" if result[4] else "")
                counts[key] = counts.get(key, 0) + 1
                if result[3] in ("TRIVIAL", "JOINED", "REJECTED", "ERROR"):
                    print(f"{result[3]}  ms_{result[0]}  <{result[1]}, {result[2]}>  class={result[4]} steps={result[5]}", flush=True)
    print(" ".join(f"{name}={count}" for name, count in sorted(counts.items())), f"of {len(rows)}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True)
    sub = parser.add_subparsers(dest="mode", required=True)
    g = sub.add_parser("grow")
    g.add_argument("label")
    g.add_argument("r1")
    g.add_argument("r2")
    g.add_argument("--rounds", type=int, default=3)
    g.add_argument("--wall-ms", type=int, default=20000)
    p = sub.add_parser("probe")
    p.add_argument("--from-results", default=str(HERE / "results.tsv"))
    p.add_argument("--jobs", type=int, default=8)
    p.add_argument("--wall-ms", type=int, default=18000)
    p.add_argument("--out", default=str(HERE / "atlas_results.tsv"))
    args = parser.parse_args()
    rad = str(pathlib.Path(args.rad).resolve())
    if args.mode == "grow":
        grow(rad, args.label, args.r1, args.r2, args.rounds, args.wall_ms)
    else:
        probe(rad, args.from_results, args.jobs, args.wall_ms, args.out)


if __name__ == "__main__":
    main()
