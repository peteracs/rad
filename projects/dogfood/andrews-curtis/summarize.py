#!/usr/bin/env python3
"""Summarize results.tsv and atlas_results.tsv as Markdown.

    python summarize.py            # print the summary
    python summarize.py --pending  # print the unresolved rows, in results.tsv form

A presentation counts as trivialized when a verified path to <x, y> exists
for it (sweep.py or an atlas probe that met class T), and as joined to a
class when a verified chain to that class's starting presentation exists.
The latest line per presentation wins in atlas_results.tsv.
"""
import collections
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def load():
    table = {}
    for line in (HERE / "data" / "miller_schupp.tsv").read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            index, r1, r2, greedy = line.split("\t")
            table[index] = {"r1": r1, "r2": r2, "greedy": greedy == "1", "status": "", "class": "", "steps": ""}
    results = HERE / "results.tsv"
    if results.exists():
        for line in results.read_text(encoding="utf-8").splitlines():
            cells = line.split("\t")
            if len(cells) >= 6:
                row = table[cells[0]]
                row["status"] = {"VERIFIED": "TRIVIAL"}.get(cells[4], cells[4])
                row["steps"] = cells[5]
    atlas = HERE / "atlas_results.tsv"
    if atlas.exists():
        for line in atlas.read_text(encoding="utf-8").splitlines():
            cells = line.split("\t")
            if len(cells) >= 6 and table[cells[0]]["status"] != "TRIVIAL":
                row = table[cells[0]]
                if cells[3] in ("TRIVIAL", "JOINED", "CORE"):
                    row["status"], row["class"], row["steps"] = cells[3], cells[4], cells[5]
                elif row["status"] not in ("JOINED", "CORE"):
                    row["status"] = cells[3]
    return table


def exponent(r1):
    """n of the Miller-Schupp relator x^-1 y^n x = y^(n+1)."""
    return r1.count("y")


def main():
    table = load()
    if "--pending" in sys.argv:
        for index, row in table.items():
            if not row["greedy"] and row["status"] not in ("TRIVIAL", "JOINED", "CORE"):
                print("\t".join([index, row["r1"], row["r2"], "0", "UNSOLVED"]))
        return
    open_rows = {index: row for index, row in table.items() if not row["greedy"]}
    by_status = collections.Counter()
    by_n = collections.defaultdict(collections.Counter)
    for row in open_rows.values():
        key = row["status"] if row["status"] in ("TRIVIAL", "UNSOLVED", "TIMEOUT", "") else f"{row['status']} {row['class']}"
        if row["status"] == "CORE":
            key = f"JOINED {row['class']}"
        if key in ("TIMEOUT", ""):
            key = "UNSOLVED"
        by_status[key] += 1
        by_n[exponent(row["r1"])][key] += 1
    keys = sorted(by_status, key=lambda key: (key != "TRIVIAL", key == "UNSOLVED", key))
    print(f"Of the {len(open_rows)} presentations the published greedy search left unsolved:\n")
    print("| n | " + " | ".join(keys) + " | total |")
    print("|---|" + "---|" * (len(keys) + 1))
    for n in sorted(by_n):
        print(f"| {n} | " + " | ".join(str(by_n[n][key]) for key in keys) + f" | {sum(by_n[n].values())} |")
    print("| all | " + " | ".join(str(by_status[key]) for key in keys) + f" | {len(open_rows)} |")


if __name__ == "__main__":
    main()
