#!/usr/bin/env python3
"""Check Andrews-Curtis certificates. Standalone: Python 3 standard library only.

    python check_certificate.py certificates/*.json

Words are strings over x, y, X, Y (X = x^-1, Y = y^-1). A certificate is

    {"index": i, "start": [r0, r1], "moves": [...], "end": [s0, s1], "claim": ...}

and, for a join, also "core": [k0, k1] and "psi": {"x": wx, "y": wy}.
Each move acts on the ordered pair of relators and is followed by free
reduction:

    ["mul", i, j, e]   r_i <- r_i r_j^e      (i != j, e = +1 or -1)
    ["inv", i]         r_i <- r_i^-1
    ["conj", i, g]     r_i <- g r_i g^-1     (g a single letter)

These are the elementary Andrews-Curtis moves. The checker confirms:

1. "start" is the Miller-Schupp presentation x^-1 y^n x y^-(n+1), x^-1 w
   of the stated table row (it is given explicitly; compare with the table);
2. replaying "moves" from "start" gives exactly "end";
3. for claim "AC-trivial": end == ["x", "y"];
4. for a join: psi is an automorphism of F(x, y) (its images Nielsen-reduce
   to a basis), and each relator of "end" is a conjugate of psi(k) or its
   inverse for the core relators k, one each. Then start is AC-equivalent
   to psi(core), which is AC-trivial exactly when the core is.
"""
import json
import sys


def inv(w):
    return w[::-1].swapcase()


def reduce(w):
    out = []
    for c in w:
        if out and out[-1] == c.swapcase():
            out.pop()
        else:
            out.append(c)
    return "".join(out)


def cyclic(w):
    w = reduce(w)
    while len(w) >= 2 and w[0] == w[-1].swapcase():
        w = w[1:-1]
    return w


def conjugacy_class(w):
    """Cyclic words of w and w^-1: equal sets exactly when one of w, w^-1 is
    conjugate to the other's representative."""
    out = set()
    for v in (cyclic(w), inv(cyclic(w))):
        out |= {v[k:] + v[:k] for k in range(len(v))} or {""}
    return frozenset(out)


def image(psi, w):
    table = {"x": psi["x"], "y": psi["y"], "X": inv(psi["x"]), "Y": inv(psi["y"])}
    return reduce("".join(table[c] for c in w))


def is_basis(a, b):
    """Nielsen reduction of a pair; a basis of F2 reduces to two letters."""
    pair = [reduce(a), reduce(b)]
    while True:
        size = len(pair[0]) + len(pair[1])
        if size == 2 and {pair[0].lower(), pair[1].lower()} == {"x", "y"}:
            return True
        best = None
        for i, j in ((0, 1), (1, 0)):
            for o in (pair[j], inv(pair[j])):
                for new in (reduce(pair[i] + o), reduce(o + pair[i])):
                    trial = list(pair)
                    trial[i] = new
                    if best is None or len(trial[0]) + len(trial[1]) < best[0]:
                        best = (len(trial[0]) + len(trial[1]), trial)
        if best[0] >= size:
            return False
        pair = best[1]


def replay(start, moves):
    r = [reduce(start[0]), reduce(start[1])]
    for move in moves:
        op, i = move[0], move[1]
        if i not in (0, 1):
            raise ValueError(f"bad relator index in {move}")
        if op == "mul":
            j, e = move[2], move[3]
            if j != 1 - i or e not in (1, -1):
                raise ValueError(f"bad move {move}")
            r[i] = reduce(r[i] + (r[j] if e == 1 else inv(r[j])))
        elif op == "inv":
            r[i] = inv(r[i])
        elif op == "conj":
            g = move[2]
            if g not in ("x", "y", "X", "Y"):
                raise ValueError(f"bad move {move}")
            r[i] = reduce(g + r[i] + inv(g))
        else:
            raise ValueError(f"unknown move {move}")
    return r


def check(cert):
    start = cert["start"]
    n = start[0].count("y")
    if cyclic(start[0]) != "X" + "y" * n + "x" + "Y" * (n + 1):
        return "start[0] is not x^-1 y^n x y^-(n+1)"
    end = replay(start, cert["moves"])
    if end != cert["end"]:
        return f"moves end at {end}, not {cert['end']}"
    if cert["claim"] == "AC-trivial":
        return None if end == ["x", "y"] else "end is not [x, y]"
    psi = cert["psi"]
    if not is_basis(psi["x"], psi["y"]):
        return "psi is not an automorphism"
    targets = [conjugacy_class(image(psi, k)) for k in cert["core"]]
    have = [conjugacy_class(w) for w in end]
    if sorted(map(sorted, have)) != sorted(map(sorted, targets)):
        return "end is not psi(core) up to conjugation and inversion of each relator"
    return None


def main():
    failures = 0
    for name in sys.argv[1:]:
        with open(name, encoding="utf-8") as f:
            cert = json.load(f)
        problem = check(cert)
        if problem:
            failures += 1
            print(f"FAIL {name}: {problem}")
        else:
            what = "AC-trivial" if cert["claim"] == "AC-trivial" else f"AC-trivial iff {cert['core']} is"
            print(f"ok   {name}: <{cert['start'][0]}, {cert['start'][1]}> {what} ({len(cert['moves'])} moves)")
    print(f"{len(sys.argv) - 1 - failures} certificates checked, {failures} failed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
