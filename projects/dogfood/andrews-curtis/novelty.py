#!/usr/bin/env python3
"""Compare the results with what is already published.

    python novelty.py            # summary
    python novelty.py --list     # one line per result with its verdict

Known before this project (Shehper et al., arXiv:2408.15332v2, 2025):

- greedy and BFS solved 533 of the 1190 presentations (the table's last
  column; all of them are excluded from the 657 here);
- Theorem B: MS(1, w) is AC-trivial for every w; MS(n, w*) with
  w* = y^-1 x y x^-1 is AC-trivial for every n (for n = 3..8 also shown by
  Lisitsa, Examples and Counterexamples 2024); MS(2, w_k) with
  w_k = y^-k x^-1 y x y is AC-trivial for every k;
- Theorem B: for each n, every MS(n, w_k) is AC-equivalent to AK(n);
- Theorem 10: two presentations trivialized by PPO.

A result is KNOWN when its presentation is one of these, and NEW otherwise.
A join between two presentations is KNOWN only when both are AC-equivalent
to the same AK(n) by Theorem B. Not compared: the paper's further
length-14 equivalences with AK(3), which it does not list in full, and the
unreviewed alejandrozarco/andrews-curtis-ms14 certificate (n = 2 only, where
this project has no result).
"""
import collections
import pathlib
import sys

from verify import cyclic_class

HERE = pathlib.Path(__file__).resolve().parent


def relator1(n):
    return "X" + "y" * n + "x" + "Y" * (n + 1)


def ms(n, w):
    """MS(n, w) as a pair of relators: x^-1 y^n x y^-(n+1) and x^-1 w."""
    return (relator1(n), "X" + w)


def key(pair):
    return frozenset((cyclic_class(pair[0]), cyclic_class(pair[1])))


def w_k(k):
    return ("Y" * k if k >= 0 else "y" * -k) + "Xyxy"


def known():
    trivial, ak = set(), {}
    for n in range(1, 8):
        trivial.add(key(ms(n, "YxyX")))
        for k in range(-3, 4):
            if n == 2:
                trivial.add(key(ms(n, w_k(k))))
            ak[key(ms(n, w_k(k)))] = n
    # Theorem 10. The second presentation's last relator is printed two ways
    # in the paper (main text and appendix); both are listed.
    trivial.add(key(("XyyyxYYYY", "XYxYYYXY")))
    trivial.add(key(("XyyyyxYYYYY", "XyxYXYYY")))
    trivial.add(key(("XyyyyxYYYYY", "XyxYXXX")))
    return trivial, ak


def results():
    """index -> (status, class) from both result tables, best status wins."""
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


def main():
    trivial, ak = known()
    classes = {}
    for line in (HERE / "atlas_classes.tsv").read_text(encoding="utf-8").splitlines():
        label, r1, r2 = line.split("\t")[:3]
        classes[label] = (r1, r2)
    counts = collections.Counter()
    lines = []
    for index, row in results().items():
        here = key(row["pair"])
        status, label = row["status"], row["class"]
        if status == "TRIVIAL":
            verdict = "KNOWN" if here in trivial else "NEW"
        elif status == "JOINED":
            # Equivalent to the class core; known when both sides are in the
            # same Theorem B family (AK(n) itself counts as its own family).
            core = key(classes[label])
            n_here = ak.get(here)
            n_core = ak.get(core, int(label[2:]) if label.startswith("AK") else None)
            verdict = "KNOWN" if n_here is not None and n_here == n_core else "NEW"
        else:
            verdict = ""
        counts[(status, verdict)] += 1
        lines.append(f"{index}\t{status}\t{label}\t{verdict}\t{row['pair'][0]}\t{row['pair'][1]}")
    if "--list" in sys.argv:
        print("\n".join(lines))
        return
    if "--table" in sys.argv:
        # The release table: one line per resolved presentation.
        moves = {}
        for line in (HERE / "certificates" / "index.tsv").read_text(encoding="utf-8").splitlines()[1:]:
            index, _, count, name = line.split("\t")
            moves[index] = (count, name)
        out = ["# index\tn\tr1\tr2\tresult\tclass\tac_moves\tcertificate\tpublished_before\tnote"]
        for line in lines:
            index, status, label, verdict, r1, r2 = line.split("\t")
            if status not in ("TRIVIAL", "JOINED"):
                continue
            n = r1.count("y")
            note = ""
            if status == "JOINED" and verdict == "NEW" and label == "AK3" and len(r1) + len(r2) == 14:
                note = "length 14: may be one of the four the paper relates to AK(3) without listing them"
            count, name = moves[index]
            out.append("\t".join([index, str(n), r1, r2, status, label or "T", count, name,
                                  "yes" if verdict == "KNOWN" else "no", note]))
        (HERE / "certificates" / "results.tsv").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
        print(f"{len(out) - 1} rows written to certificates/results.tsv")
        return
    for (status, verdict), count in sorted(counts.items()):
        print(f"{status:9} {verdict:6} {count}")


if __name__ == "__main__":
    main()
