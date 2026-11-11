#!/usr/bin/env python3
"""Check an Andrews-Curtis trivialization printed by ac.rad, independently.

Reads the output of `rad ac.rad -- r1 r2 ...` on stdin. Shares no code with
the search: words are free-reduced here from scratch, and every step is
checked by trying all conjugates.

After the SOLVED line every line is a state `a,b`, optionally prefixed by
`autK ` when the state was reached by a Whitehead automorphism.

  product step   <a, b> -> <a', b'>: one relator is unchanged as a cyclic word
                 up to inversion, the other is the cyclic reduction of
                 g a g^-1 * h b^e h^-1 for some words g, h and e = +1 or -1.
                 These are products of AC moves.
  aut step       both relators are the images of the previous ones under
                 automorphism K (0: y->yx, 1: y->yX, 2: x->xy, 3: x->xY).

Why aut steps prove AC-triviality. AC moves commute with automorphisms: if
<phi(r1), phi(r2)> goes to <u, v> by AC moves then <r1, r2> goes to
<phi^-1(u), phi^-1(v)> by AC moves. So a path with aut steps from <r1, r2> to
<x, y> shows that <r1, r2> is AC-equivalent to <psi(x), psi(y)> for an
automorphism psi. That pair is a basis of F(x, y), and Nielsen moves, which
are AC moves, take every basis to <x, y>.

    rad ac.rad -- <r1> <r2> | python verify.py <r1> <r2>

Exit 0 only if the path starts at the given presentation, every step is
valid, and it ends at <x, y | x, y>.

With `--to <s1> <s2>` the path must end at <s1, s2> instead (atlas.rad
prints such paths). Every step can be undone by a step of the same kind, so
the two presentations are then AC-equivalent up to an automorphism of the
free group, and one can be trivialized exactly when the other can.
"""
import sys

# Only the image of the generator is written down. The image of its inverse
# is computed, so a table entry cannot fail to be a homomorphism.
GENERATOR_IMAGES = {
    "aut0": ("y", "yx"),
    "aut1": ("y", "yX"),
    "aut2": ("x", "xy"),
    "aut3": ("x", "xY"),
}


def inv(w):
    return w[::-1].swapcase()


def free_reduce(w):
    out = []
    for c in w:
        if out and out[-1] == c.swapcase():
            out.pop()
        else:
            out.append(c)
    return "".join(out)


def cyc_reduce(w):
    w = free_reduce(w)
    while len(w) >= 2 and w[0] == w[-1].swapcase():
        w = w[1:-1]
    return w


def rotations(w):
    return {w[i:] + w[:i] for i in range(len(w))} if w else {""}


def cyclic_class(w):
    """All rotations of the cyclically reduced word and of its inverse."""
    w = cyc_reduce(w)
    return frozenset(rotations(w) | rotations(inv(w)))


def same_pair(p, q):
    return {cyclic_class(p[0]), cyclic_class(p[1])} == {cyclic_class(q[0]), cyclic_class(q[1])}


def product_step_ok(old, new):
    (a, b), (c, d) = old, new
    for keep, change, kept_new, changed_new in ((b, a, d, c), (b, a, c, d), (a, b, c, d), (a, b, d, c)):
        if cyclic_class(keep) != cyclic_class(kept_new):
            continue
        target = cyclic_class(changed_new)
        for u in rotations(cyc_reduce(change)):
            for other in (cyc_reduce(keep), inv(cyc_reduce(keep))):
                for v in rotations(other):
                    if cyc_reduce(u + v) in target:
                        return True
    return False


def abelian_determinant(pair):
    """Determinant of the exponent-sum matrix. It is +1 or -1 for every
    presentation of the trivial group, and every valid step preserves that."""
    (a, b) = pair
    row = lambda w: (w.count("x") - w.count("X"), w.count("y") - w.count("Y"))
    (p, q), (r, s) = row(a), row(b)
    return p * s - q * r


def aut_step_ok(old, new, name):
    letter, image = GENERATOR_IMAGES[name]
    table = {letter: image, letter.swapcase(): inv(image)}
    image = tuple("".join(table.get(c, c) for c in word) for word in old)
    return same_pair(image, new)


def main():
    target = ("x", "y")
    args = sys.argv[1:]
    if len(args) == 5 and args[2] == "--to":
        target = (args[3], args[4])
        args = args[:2]
    if len(args) != 2:
        sys.exit("usage: verify.py <r1> <r2> [--to <s1> <s2>]  (path on stdin)")
    lines = [line.strip() for line in sys.stdin if line.strip()]
    if not lines or not lines[0].startswith(("SOLVED", "JOINED")):
        sys.exit(f"no SOLVED or JOINED line: {lines[:1]}")
    path = []
    for line in lines[1:]:
        via, _, state = line.rpartition(" ")
        pair = tuple(state.split(","))
        if len(pair) != 2 or set("".join(pair)) - set("xXyY") or (via and via not in GENERATOR_IMAGES):
            sys.exit(f"malformed path line: {line!r}")
        path.append((via, pair))
    given = (args[0], args[1])
    if path[0][0] or not same_pair(given, path[0][1]):
        sys.exit(f"path starts at {path[0][1]}, not at the given presentation")
    for index, (_, state) in enumerate(path):
        if abs(abelian_determinant(state)) != 1:
            sys.exit(f"state {index} {state} does not abelianize to the trivial group")
    auts = 0
    for index in range(len(path) - 1):
        old, (via, new) = path[index][1], path[index + 1]
        ok = aut_step_ok(old, new, via) if via else product_step_ok(old, new)
        auts += bool(via)
        if not ok:
            sys.exit(f"invalid step {index + 1}: {old} -> {via} {new}")
    if not same_pair(path[-1][1], target):
        sys.exit(f"path ends at {path[-1][1]}, not at <{target[0]}, {target[1]}>")
    if target == ("x", "y"):
        print(f"VERIFIED {len(path) - 1} steps ({auts} automorphisms): <x,y | {given[0]}, {given[1]}> is AC-trivial")
    else:
        print(
            f"VERIFIED {len(path) - 1} steps ({auts} automorphisms): <x,y | {given[0]}, {given[1]}> "
            f"is AC-trivial if and only if <x,y | {target[0]}, {target[1]}> is"
        )


if __name__ == "__main__":
    main()
