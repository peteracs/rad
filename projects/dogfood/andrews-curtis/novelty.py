#!/usr/bin/env python3
"""Compare the results with what is already published.

    python novelty.py            # summary
    python novelty.py --list     # one line per presentation with its verdict
    python novelty.py --table    # write certificates/results.tsv

Known before this project (Shehper et al., arXiv:2408.15332v2, 2025):

- greedy and BFS solved 533 of the 1190 presentations (the table's last
  column; all of them are excluded from the 657 here);
- Theorem B: MS(1, w) is AC-trivial for every w; MS(n, w*) with
  w* = y^-1 x y x^-1 is AC-trivial for every n (for n = 3..8 also shown by
  Lisitsa, Examples and Counterexamples 2024); MS(2, w_k) with
  w_k = y^-k x^-1 y x y is AC-trivial for every k;
- Theorem B: for each n, every MS(n, w_k) is AC-equivalent to AK(n);
- Theorem 10: two presentations trivialized by PPO.

A published statement about a presentation Q is also a statement about
phi(Q) for every automorphism phi of F(x, y): AC moves commute with phi, and
phi of a basis is a basis. Five automorphisms map the first Miller-Schupp
relator to a conjugate of itself or of its inverse, and so map MS(n, w) to
another MS(n, w'):

    y -> y^-1,   x -> x y,   x -> x y^-1,   x -> y x,   x -> y^-1 x.

For example MS(n, y x y^-1 x^-1) is the image of MS(n, w*) under y -> y^-1.

What a presentation is known to be:

    TRIVIAL   AC-trivial by a published theorem
    AK(n)     AC-trivial exactly when AK(n) is, by Theorem B

and how:

    KNOWN     it is one of the published presentations
    IMAGE     it is the image of one under a product of the five automorphisms
    NEW       neither: only the paths of this project say so

A class of this project (presentations joined by verified chains) inherits
what any one of its members is known to be. So a presentation in a class
with an image of MS(n, w*) is AC-trivial, and that is NEW unless it is itself
KNOWN or an IMAGE.

Not compared: the paper's further length-14 equivalences with AK(3), which
it does not list in full, and the unreviewed alejandrozarco/andrews-curtis-ms14
certificate (n = 2 only).
"""
import collections
import pathlib
import sys

from verify import cyclic_class, free_reduce, inv

HERE = pathlib.Path(__file__).resolve().parent

# Images of x and y. Each maps x^-1 y^n x y^-(n+1) to a conjugate of itself
# or of its inverse; images() asserts it.
SYMMETRIES = (
    {"x": "x", "y": "Y"},
    {"x": "xy", "y": "y"},
    {"x": "xY", "y": "y"},
    {"x": "yx", "y": "y"},
    {"x": "Yx", "y": "y"},
)


def relator1(n):
    return "X" + "y" * n + "x" + "Y" * (n + 1)


def ms(n, w):
    """MS(n, w) as a pair of relators: x^-1 y^n x y^-(n+1) and x^-1 w."""
    return (relator1(n), "X" + w)


def key(pair):
    return frozenset((cyclic_class(pair[0]), cyclic_class(pair[1])))


def w_k(k):
    return ("Y" * k if k >= 0 else "y" * -k) + "Xyxy"


def apply(images, word):
    table = dict(images)
    table.update({g.upper(): inv(w) for g, w in images.items()})
    return free_reduce("".join(table[c] for c in word))


def images(pair, rounds=10, longest=14):
    """Keys of the images of `pair` under products of at most `rounds` of the
    symmetries, keeping second relators of at most `longest` letters."""
    first = cyclic_class(pair[0])
    seen = {key(pair)}
    frontier = [pair]
    for _ in range(rounds):
        later = []
        for a, b in frontier:
            for symmetry in SYMMETRIES:
                image = (apply(symmetry, a), apply(symmetry, b))
                assert cyclic_class(image[0]) == first, "not a symmetry of the first relator"
                if len(image[1]) <= longest and key(image) not in seen:
                    seen.add(key(image))
                    later.append(image)
        frontier = later
    return seen


def known():
    """key -> (what, how) for the published presentations and their images."""
    listed = []   # (what, pair)
    for n in range(1, 8):
        listed.append(("TRIVIAL", ms(n, "YxyX")))
        for k in range(-3, 4):
            listed.append(("TRIVIAL" if n == 2 else f"AK{n}", ms(n, w_k(k))))
    # Theorem 10. The second presentation's last relator is printed two ways
    # in the paper (main text and appendix); both are listed.
    listed.append(("TRIVIAL", ("XyyyxYYYY", "XYxYYYXY")))
    listed.append(("TRIVIAL", ("XyyyyxYYYYY", "XyxYXYYY")))
    listed.append(("TRIVIAL", ("XyyyyxYYYYY", "XyxYXXX")))
    table = {}
    for what, pair in listed:
        table[key(pair)] = (what, "KNOWN")
    for what, pair in listed:
        for image in images(pair):
            table.setdefault(image, (what, "IMAGE"))
    return table


def results():
    """index -> row, for the 657 presentations the greedy search left."""
    table = {}
    for line in (HERE / "data" / "miller_schupp.tsv").read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            index, r1, r2, greedy = line.split("\t")
            if greedy == "0":
                table[index] = {"pair": (r1, r2), "status": "UNSOLVED", "class": ""}
    for line in (HERE / "results.tsv").read_text(encoding="utf-8").splitlines():
        cells = line.split("\t")
        if len(cells) >= 5 and cells[4] == "VERIFIED":
            table[cells[0]]["status"] = "TRIVIAL"
    for line in (HERE / "atlas_results.tsv").read_text(encoding="utf-8").splitlines():
        cells = line.split("\t")
        row = table.get(cells[0])
        if row is None or row["status"] == "TRIVIAL" or len(cells) < 5:
            continue
        if cells[3] in ("TRIVIAL", "JOINED", "CORE"):
            row["status"], row["class"] = cells[3], cells[4]
    return table


def judge():
    """Add to every row: `is` (what it is known or shown to be), `by` (how
    this project shows it), `verdict` (KNOWN, IMAGE, NEW or empty)."""
    published = known()
    rows = results()
    classes = {}
    for line in (HERE / "atlas_classes.tsv").read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            label, r1, r2 = line.split("\t")[:3]
            classes[label] = (r1, r2)
    # What each class inherits from its start and its members.
    inherits = {}
    for label, pair in classes.items():
        hit = published.get(key(pair))
        if label.startswith("AK"):
            inherits[label] = label
        if hit:
            inherits[label] = "TRIVIAL" if hit[0] == "TRIVIAL" else inherits.get(label, hit[0])
    for row in rows.values():
        hit = published.get(key(row["pair"]))
        if hit and row["status"] in ("JOINED", "CORE") and inherits.get(row["class"]) != "TRIVIAL":
            inherits[row["class"]] = hit[0]
    for row in rows.values():
        hit = published.get(key(row["pair"]))
        if row["status"] == "TRIVIAL":
            row["is"], row["by"] = "TRIVIAL", "certificate"
        elif row["class"] and row["class"] in inherits:
            row["is"], row["by"] = inherits[row["class"]], "chain"
        elif row["class"]:
            row["is"], row["by"] = "", "chain"
        else:
            row["is"], row["by"] = (hit[0] if hit else ""), ""
        if not row["is"]:
            row["verdict"] = ""
        elif hit and hit[0] == row["is"]:
            row["verdict"] = hit[1]
        else:
            row["verdict"] = "NEW"
    return rows, classes


def column(row):
    if row["is"] == "TRIVIAL":
        return "trivial, certificate here" if row["by"] == "certificate" else (
            "trivial, by Theorem B and a chain here" if row["by"] == "chain" else "trivial, published only")
    if row["is"].startswith("AK"):
        return "with an AK(k)"
    return "in an open class" if row["class"] else "alone"


def orbits(rows):
    """index -> the least index of its orbit: rows that are images of each
    other under the five automorphisms, which takes no search to see."""
    by_key = {key(row["pair"]): index for index, row in rows.items()}
    orbit = {}
    for index, row in rows.items():
        if index in orbit:
            continue
        members = {index} | {by_key[image] for image in images(row["pair"]) if image in by_key}
        merged = {orbit[member] for member in members if member in orbit}
        name = min(merged | {index}, key=int)
        for member, old in list(orbit.items()):
            if old in merged:
                orbit[member] = name
        for member in members:
            orbit[member] = name
    return orbit


COLUMNS = ["trivial, certificate here", "trivial, by Theorem B and a chain here", "trivial, published only",
           "with an AK(k)", "in an open class", "alone"]


def main():
    rows, classes = judge()
    lines = []
    for index, row in rows.items():
        lines.append("\t".join([index, row["status"], row["class"], row["is"], row["by"], row["verdict"], *row["pair"]]))
    if "--list" in sys.argv:
        print("\n".join(lines))
        return
    if "--table" in sys.argv:
        # The release table: one line per presentation with a certificate.
        moves = {}
        for line in (HERE / "certificates" / "index.tsv").read_text(encoding="utf-8").splitlines()[1:]:
            index, _, count, name = line.split("\t")
            moves[index] = (count, name)
        out = ["# index\tn\tr1\tr2\tresult\tclass\tac_moves\tcertificate\tpublished_before\tnote"]
        orbit = orbits(rows)
        for index, row in rows.items():
            if row["status"] not in ("TRIVIAL", "JOINED"):
                continue
            r1, r2 = row["pair"]
            notes = []
            if row["status"] == "JOINED" and row["is"] == "TRIVIAL":
                notes.append("its class contains a presentation that Theorem B trivializes, so it is AC-trivial")
            if row["verdict"] == "IMAGE":
                notes.append("an automorphic image of a published presentation")
            start = row["class"][2:] if row["class"].startswith("ms") else ""
            if row["status"] == "JOINED" and start in orbit and orbit[start] == orbit[index]:
                notes.append("an automorphic image of the presentation its class starts at")
            if row["status"] == "JOINED" and row["verdict"] == "NEW" and row["class"] == "AK3" and len(r1) + len(r2) == 14:
                notes.append("length 14: may be one of the four the paper relates to AK(3) without listing them")
            count, name = moves[index]
            published = {"KNOWN": "yes", "IMAGE": "image", "NEW": "no", "": "no"}[row["verdict"]]
            out.append("\t".join([index, str(r1.count("y")), r1, r2, row["status"], row["class"] or "T", count, name,
                                  published, "; ".join(notes)]))
        (HERE / "certificates" / "results.tsv").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
        print(f"{len(out) - 1} rows written to certificates/results.tsv")
        return

    by_n = collections.defaultdict(collections.Counter)
    new_by_n = collections.defaultdict(collections.Counter)
    verdicts = collections.defaultdict(collections.Counter)
    for row in rows.values():
        n = row["pair"][0].count("y")
        by_n[n][column(row)] += 1
        if row["verdict"]:
            verdicts[column(row)][row["verdict"]] += 1
        if row["verdict"] == "NEW":
            new_by_n[n][column(row)] += 1
    total = collections.Counter()
    for counts in by_n.values():
        total.update(counts)
    columns = [name for name in COLUMNS if total[name]]

    def cell(count, new, name):
        # The columns that state a fact also say how much of it is new.
        return f"{count} ({new} new)" if name in COLUMNS[:4] and count else str(count)

    print(f"Of the {len(rows)} presentations the published greedy search left unsolved:\n")
    print("| n | " + " | ".join(columns) + " | total |")
    print("|---|" + "---|" * (len(columns) + 1))
    for n in sorted(by_n):
        print(f"| {n} | " + " | ".join(cell(by_n[n][name], new_by_n[n][name], name) for name in columns)
              + f" | {sum(by_n[n].values())} |")
    print("| all | " + " | ".join(cell(total[name], verdicts[name]["NEW"], name) for name in columns)
          + f" | {len(rows)} |")
    print()
    for name in COLUMNS[:4]:
        counts = verdicts[name]
        print(f"{name}: {counts['NEW']} new, {counts['IMAGE']} images of a published presentation, "
              f"{counts['KNOWN']} published")
    sizes = collections.Counter(row["class"] for row in rows.values() if column(row) == "in an open class")
    by_size = collections.Counter(sizes.values())
    print(f"in an open class: {len(sizes)} classes; sizes: "
          f"{', '.join(f'{size} (x{count})' for size, count in sorted(by_size.items(), reverse=True))}")
    print(f"\nQuestions left open: {len(sizes)} classes and {total['alone']} presentations that stand alone, "
          f"besides AK(3) to AK(7).")

    orbit = orbits(rows)
    opened =[index for index, row in rows.items() if column(row) == "in an open class"]
    print(f"\nUnder the five automorphisms alone the {len(rows)} presentations are {len(set(orbit.values()))} orbits. "
          f"The {len(opened)} in open classes are {len({orbit[index] for index in opened})} orbits, "
          f"which the chains join into {len(sizes)} classes.")


if __name__ == "__main__":
    main()
