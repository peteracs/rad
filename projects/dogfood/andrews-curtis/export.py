#!/usr/bin/env python3
"""Turn the search paths into certificates of elementary AC moves.

    python export.py              # write certificates/*.json and certificates/index.tsv
    python export.py paths/ms_0.txt

A search path mixes two kinds of step: products (compositions of AC moves
on cyclic words) and Whitehead automorphisms, which are not AC moves. This
script replays a path on explicit reduced words and emits only the moves

    ["mul", i, j, e]   r_i <- r_i * r_j^e          (e = 1 or -1, i != j)
    ["inv", i]         r_i <- r_i^-1
    ["conj", i, g]     r_i <- g * r_i * g^-1       (g one of x, X, y, Y)

each followed by free reduction (relators are indexed 0 and 1).

Automorphisms are removed with the identity phi(Q) ->AC... : if the path
goes Q -> phi(Q) -> ..., every later move m is replaced by Psi(m), where Psi
is the product of the inverses of the automorphisms met so far; a move
conjugating by g becomes conjugation by the word Psi(g), spelled letter by
letter. The moves then end at Psi(F) instead of F:

- for a trivialization F = <x, y>, Psi(F) is a basis of the free group,
  and a Nielsen reduction (mul and inv moves) takes it to <x, y>;
- for a join to a class core K, the certificate ends at a pair whose
  relators are conjugates of Psi(K)'s relators or their inverses; it
  records Psi, and K is trivializable exactly when Psi(K) is.

check_certificate.py replays a certificate with nothing but free reduction.
"""
import json
import pathlib
import sys

from verify import cyc_reduce, cyclic_class, free_reduce, inv, rotations

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "certificates"

# Inverses of the four automorphisms of the searches, as images of x and y.
INVERSE = {
    "aut0": {"x": "x", "y": "yX"},
    "aut1": {"x": "x", "y": "yx"},
    "aut2": {"x": "xY", "y": "y"},
    "aut3": {"x": "xy", "y": "y"},
}
FORWARD = {
    "aut0": {"x": "x", "y": "yx"},
    "aut1": {"x": "x", "y": "yX"},
    "aut2": {"x": "xy", "y": "y"},
    "aut3": {"x": "xY", "y": "y"},
}


def apply(images, word):
    table = dict(images)
    table.update({g.upper(): inv(w) for g, w in images.items()})
    return free_reduce("".join(table[c] for c in word))


def compose(outer, inner):
    """Images of outer o inner."""
    return {g: apply(outer, inner[g]) for g in "xy"}


class Replay:
    """Keeps the local pair (the search's coordinates) and the global pair
    (the presentation the certificate is about), global = Psi(local)."""

    def __init__(self, pair):
        self.local = [free_reduce(pair[0]), free_reduce(pair[1])]
        self.glob = list(self.local)
        self.psi = {"x": "x", "y": "y"}
        self.moves = []
        for i in (0, 1):
            self.cyclically_reduce(i)

    def emit(self, move):
        self.moves.append(move)
        op, i = move[0], move[1]
        r = self.glob
        if op == "mul":
            r[i] = free_reduce(r[i] + (r[move[2]] if move[3] == 1 else inv(r[move[2]])))
        elif op == "inv":
            r[i] = inv(r[i])
        else:
            r[i] = free_reduce(move[2] + r[i] + inv(move[2]))

    def local_mul(self, i, j, e):
        self.local[i] = free_reduce(self.local[i] + (self.local[j] if e == 1 else inv(self.local[j])))
        self.emit(["mul", i, j, e])

    def local_inv(self, i):
        self.local[i] = inv(self.local[i])
        self.emit(["inv", i])

    def local_conj(self, i, g):
        self.local[i] = free_reduce(g + self.local[i] + inv(g))
        # Conjugating by the word w = c1...ck is conjugating by ck, then ..., then c1.
        for c in reversed(apply(self.psi, g)):
            self.emit(["conj", i, c])

    def cyclically_reduce(self, i):
        while len(self.local[i]) >= 2 and self.local[i][0] == self.local[i][-1].swapcase():
            self.local_conj(i, self.local[i][-1])

    def rotate_to(self, i, target):
        """Turn local[i] (cyclically reduced) into its rotation `target`."""
        while self.local[i] != target:
            self.local_conj(i, inv(self.local[i][0]))

    def automorphism(self, name):
        self.local = [apply(FORWARD[name], w) for w in self.local]
        self.psi = compose(self.psi, INVERSE[name])
        for i in (0, 1):
            self.cyclically_reduce(i)

    def product(self, new):
        """Make one relator the cyclic product the search took, the other kept."""
        for i, j in ((0, 1), (1, 0)):
            if cyclic_class(self.local[j]) not in (cyclic_class(new[0]), cyclic_class(new[1])):
                continue
            changed = new[0] if cyclic_class(new[1]) == cyclic_class(self.local[j]) else new[1]
            target = cyclic_class(changed)
            for flip_i in (False, True):
                a = inv(self.local[i]) if flip_i else self.local[i]
                for u in sorted(rotations(a)):
                    for e in (1, -1):
                        b = self.local[j] if e == 1 else inv(self.local[j])
                        for v in sorted(rotations(b)):
                            if cyc_reduce(u + v) in target:
                                if flip_i:
                                    self.local_inv(i)
                                self.rotate_to(i, u)
                                self.rotate_to(j, v if e == 1 else inv(v))
                                self.local_mul(i, j, e)
                                self.cyclically_reduce(i)
                                return
        raise ValueError(f"no product gives {new} from {self.local}")

    def check(self):
        assert self.glob == [apply(self.psi, w) for w in self.local], "global != Psi(local)"


def nielsen_to_xy(replay):
    """Nielsen-reduce a basis (the global pair) to x, y with mul and inv moves."""
    r = replay.glob

    def options():
        for i, j in ((0, 1), (1, 0)):
            for e in (1, -1):
                yield [["mul", i, j, e]]                                # r_i r_j^e
                yield [["inv", i], ["mul", i, j, -e], ["inv", i]]       # r_j^e r_i
    while len(r[0]) + len(r[1]) > 2:
        best = None
        for seq in options():
            trial = list(r)
            for m in seq:
                if m[0] == "mul":
                    trial[m[1]] = free_reduce(trial[m[1]] + (trial[m[2]] if m[3] == 1 else inv(trial[m[2]])))
                else:
                    trial[m[1]] = inv(trial[m[1]])
            size = len(trial[0]) + len(trial[1])
            if best is None or size < best[0]:
                best = (size, seq)
        if best[0] >= len(r[0]) + len(r[1]):
            raise ValueError(f"Nielsen reduction stalled at {r}")
        for m in best[1]:
            replay.emit(m)
    # Now each relator is a single letter; put x first and y second, positive.
    if r[0].lower() == "y":
        # (a, b) -> (ab, b) -> (ab, a^-1) -> (b^-1 a^-1, a^-1) -> (b^-1, a^-1) -> (b, a^-1)
        for m in (["mul", 0, 1, 1], ["mul", 1, 0, -1], ["inv", 0], ["mul", 0, 1, -1], ["inv", 0]):
            replay.emit(m)
    for i in (0, 1):
        if r[i].isupper():
            replay.emit(["inv", i])
    assert r == ["x", "y"], r


def read_path(path):
    lines = [line for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    head, states = lines[0], []
    for line in lines[1:]:
        parts = line.split()
        step, state = (parts[0], parts[1]) if len(parts) == 2 else ("product", parts[0])
        states.append((step, tuple(state.split(","))))
    return head, states


def certificate(path, start, classes):
    head, states = read_path(path)
    replay = Replay(start)
    if cyclic_class(start[0]) != cyclic_class(states[0][1][0]) and cyclic_class(start[0]) != cyclic_class(states[0][1][1]):
        raise ValueError("path does not start at the presentation")
    for step, state in states[1:]:
        if step == "product":
            replay.product(state)
        else:
            replay.automorphism(step)
    replay.check()
    label = head.split(" meets class ")[1].split()[0] if " meets class " in head else "T"
    cert = {"start": list(start), "moves": replay.moves}
    if label == "T":
        nielsen_to_xy(replay)
        cert["end"] = ["x", "y"]
        cert["claim"] = "AC-trivial"
    else:
        cert["end"] = list(replay.glob)
        cert["class"] = label
        cert["core"] = list(classes[label])
        cert["psi"] = replay.psi
        cert["claim"] = "AC-trivial if and only if the core is"
    return cert


def main():
    classes = {}
    for line in (HERE / "atlas_classes.tsv").read_text(encoding="utf-8").splitlines():
        label, r1, r2 = line.split("\t")[:3]
        classes[label] = (r1, r2)
    rows = {}
    for line in (HERE / "data" / "miller_schupp.tsv").read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            index, r1, r2, _ = line.split("\t")
            rows[index] = (r1, r2)
    paths = [pathlib.Path(p) for p in sys.argv[1:]] or sorted((HERE / "paths").glob("ms_*.txt"))
    OUT.mkdir(exist_ok=True)
    index_lines = []
    for path in paths:
        index = path.stem.split("_")[1]
        cert = certificate(path, rows[index], classes)
        cert["index"] = int(index)
        name = path.stem + ".json"
        (OUT / name).write_text(json.dumps(cert, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
        index_lines.append(f"{index}\t{cert.get('class', 'T')}\t{len(cert['moves'])}\t{name}")
        print(f"{name}: {len(cert['moves'])} moves", flush=True)
    if not sys.argv[1:]:
        (OUT / "index.tsv").write_text("# index\tclass\tmoves\tfile\n" + "\n".join(index_lines) + "\n",
                                       encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
