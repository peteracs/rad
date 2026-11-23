#!/usr/bin/env python3
"""Summarize results.tsv and atlas_results.tsv as Markdown.

    python summarize.py            # print the summary
    python summarize.py --classes  # the summary and one line per class
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

    def column(row):
        if row["status"] == "TRIVIAL":
            return "TRIVIAL"
        if row["status"] in ("JOINED", "CORE"):
            return "with an AK(k)" if row["class"].startswith("AK") else "in another class"
        return "alone"

    columns = ["TRIVIAL", "with an AK(k)", "in another class", "alone"]
    by_n = collections.defaultdict(collections.Counter)
    sizes = collections.Counter()
    for row in open_rows.values():
        by_n[exponent(row["r1"])][column(row)] += 1
        if row["status"] in ("JOINED", "CORE"):
            sizes[row["class"]] += 1
    total = collections.Counter()
    for counts in by_n.values():
        total.update(counts)
    print(f"Of the {len(open_rows)} presentations the published greedy search left unsolved:\n")
    print("| n | " + " | ".join(columns) + " | total |")
    print("|---|" + "---|" * (len(columns) + 1))
    for n in sorted(by_n):
        print(f"| {n} | " + " | ".join(str(by_n[n][name]) for name in columns) + f" | {sum(by_n[n].values())} |")
    print("| all | " + " | ".join(str(total[name]) for name in columns) + f" | {len(open_rows)} |")

    ak = sorted(label for label in sizes if label.startswith("AK"))
    other = sorted((label for label in sizes if not label.startswith("AK")), key=lambda label: (-sizes[label], label))
    print()
    print(f"{len(ak)} classes hold an AK(k) ({', '.join(f'{label}: {sizes[label]}' for label in ak)}).")
    by_size = collections.Counter(sizes[label] for label in other)
    print(f"{len(other)} other classes hold {sum(sizes[label] for label in other)} presentations; "
          f"sizes: {', '.join(f'{size} (x{count})' for size, count in sorted(by_size.items(), reverse=True))}.")
    print(f"That is {len(ak) + len(other)} classes and {total['alone']} presentations that stand alone, "
          f"where the table started with {len(open_rows)}. This script only counts what the paths show; "
          f"`python novelty.py` also says which classes a published theorem settles.")
    if "--classes" in sys.argv:
        print()
        print("| class | presentations | starts at |")
        print("|---|---|---|")
        starts = {}
        for line in (HERE / "atlas_classes.tsv").read_text(encoding="utf-8").splitlines():
            if line.strip() and not line.startswith("#"):
                label, r1, r2 = line.split("\t")[:3]
                starts[label] = f"`<{r1}, {r2}>`"
        for label in ak + other:
            print(f"| {label} | {sizes[label]} | {starts.get(label, '')} |")


if __name__ == "__main__":
    main()
