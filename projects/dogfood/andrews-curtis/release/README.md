# Andrews–Curtis certificates for Miller–Schupp presentations

Machine-checkable Andrews–Curtis (AC) certificates for the Miller–Schupp
presentations

    MS(n, w) = < x, y | x^-1 y^n x = y^(n+1), x = w >,   1 <= n <= 7, |w| <= 7,

the 1,190-presentation benchmark of Shehper et al., *What makes math problems
hard for reinforcement learning: a case study* (arXiv:2408.15332). Their greedy
search solved 533 and left 657. For those 657:

| n | AC-trivial, certificate here | AC-trivial by Theorem B, tied here | tied to AK(n) | in an open class | total |
|---|---|---|---|---|---|
| 2 | 0 | 0 | 0 | 32 | 32 |
| 3 | 2 (0 new) | 0 | 34 (20 new) | 36 | 72 |
| 4 | 24 (18 new) | 0 | 22 (8 new) | 72 | 118 |
| 5 | 31 (19 new) | 0 | 16 (2 new) | 88 | 135 |
| 6 | 28 (14 new) | 0 | 14 (0 new) | 104 | 146 |
| 7 | 8 (8 new) | 14 (0 new) | 14 (0 new) | 118 | 154 |
| all | 93 (**59** new) | 14 (0 new) | 100 (**30** new) | 450 | 657 |

- **AC-trivial, certificate here**: an explicit sequence of elementary AC
  moves to `<x, y | x, y>`.
- **AC-trivial by Theorem B, tied here**: an explicit sequence of AC moves to
  an automorphic image of `MS(7, y x y^-1 x^-1)` (table row 1037), which is
  itself the image of Theorem B's `MS(7, y^-1 x y x^-1)` under `y -> y^-1`.
  There are 13 such certificates; the fourteenth presentation is row 1037.
- **Tied to AK(n)**: an explicit sequence of AC moves to `psi(AK(n))` for a
  stated automorphism `psi` of F(x, y), where `AK(n) = <x, y | x^n = y^(n+1),
  xyx = yxy>`. Such a presentation is AC-trivial if and only if AK(n) is. For
  n = 3 this adds 20 presentations to those known to stand or fall with
  AK(3), the best-known potential counterexample.
- **In an open class**: tied in the same way to the first table row of its
  class. The 450 presentations form 120 classes of 2 to 30; none of the
  published results compared here settles any of them, and nothing shows
  that two classes are different.
- **new**: not published, and not the image of a published presentation under
  an automorphism (see *Novelty* below).

The certificates also cover 48 presentations the greedy search had solved.

## The 120 open classes

Five automorphisms of F(x, y) map the first Miller–Schupp relator to a
conjugate of itself or of its inverse: `y -> y^-1`, `x -> x y`, `x -> x y^-1`,
`x -> y x`, `x -> y^-1 x`. They map `MS(n, w)` to `MS(n, w')`, and an image of
a presentation is AC-trivial exactly when the presentation is. Under them the
657 presentations fall into 182 orbits; this takes no search. The 450 in open
classes are 147 orbits. The certificates join those into 120 classes: 252
certificates only repeat the orbit, and 78 tie a presentation to one in a
different orbit (the searches used 3 to 16 products of relators for each).
`results.tsv` marks the first kind in its `note` column.

## Files

| File | What |
|---|---|
| `results.tsv` | One row per presentation with a certificate: table index, n, relators, result, class, number of moves, certificate file, whether published before (`yes`, `image` or `no`), note. |
| `certificates/*.json` | One certificate per presentation. |
| `check_certificate.py` | Checks certificates. Python 3 standard library only, about 160 lines. |
| `data/miller_schupp.tsv` | The benchmark table (from AC-Solver, MIT license; see `NOTICE`). |
| `paper/note.tex` | A short note describing the method and results. |

A class named `ms<i>` starts at table row `i`; `T` is the trivial
presentation.

## Checking

```sh
python check_certificate.py certificates
```

Words are strings over `x, y, X, Y` (capitals are inverses). A certificate
lists moves on the ordered pair of relators, each followed by free reduction:

| Move | Effect |
|---|---|
| `["mul", i, j, e]` | `r_i <- r_i r_j^e`, `e = ±1` |
| `["inv", i]` | `r_i <- r_i^-1` |
| `["conj", i, g]` | `r_i <- g r_i g^-1`, `g` a letter |

The checker replays the moves from `start` and requires the result to equal
`end`; for a trivialization `end` must be `["x", "y"]`. For a tie it checks
that `psi` is an automorphism (its images Nielsen-reduce to a basis) and that
each relator of `end` is conjugate to `psi(k)` or its inverse for the core
relators `k`. It shares no code with the search.

## How they were found

A best-first search over pairs of cyclic words, using products of relators
(compositions of AC moves) and Whitehead automorphisms of F(x, y) that shorten
the pair. Automorphisms are not AC moves; the exporter removes them. If a path
reaches `phi(Q)` from `Q`, every later move is conjugated back through
`phi^-1`, so the certificate ends at an automorphic image of the target; for a
trivialization that image is a basis, which a Nielsen reduction takes to
`<x, y>`.

Ties come from two searches that reach the same pair of cyclic words, up to
renaming the generators (`x -> x^-1`, `y -> y^-1`, `x <-> y`): the first kind
from a saved search graph per class, the open classes from comparing the
pairs that a search of 20,000 states from each presentation reaches.

The search was written in [Rad](https://github.com/peteracs/rad) as a language
test (`projects/dogfood/andrews-curtis`); nothing here needs Rad.

## Novelty

"Published" means one of: the greedy and BFS solutions; Theorem B of
arXiv:2408.15332v2 (MS(1, w) for all w; MS(n, w*) with w* = y^-1 x y x^-1 for
all n, also Lisitsa 2024 for n = 3..8; MS(2, w_k) with w_k = y^-k x^-1 y x y;
MS(n, w_k) AC-equivalent to AK(n)); its Theorem 10. A result here is counted
as new only if the presentation is neither published nor the image of a
published one under a product of the five automorphisms above. Of the 93
trivializations 3 are published and 31 are images; of the 100 presentations
tied to an AK(n), 35 are published and 35 are images.

An earlier version of this release counted the images as new (78
trivializations, 65 ties to AK(n)) and called `MS(7, y x y^-1 x^-1)` open.
Both were wrong.

Not compared exactly: that paper's four length-14 presentations AC-equivalent
to AK(3), which it does not list in machine-readable form (one tied
presentation here has length 14 and is flagged in `results.tsv`); the
unreviewed
[andrews-curtis-ms14](https://github.com/alejandrozarco/andrews-curtis-ms14)
results, which concern n = 2 only; and anything published elsewhere about
individual presentations. Corrections are welcome.

## License

Code: MIT (`LICENSE`). Certificates and tables: CC BY 4.0 (`LICENSE-DATA`).
The benchmark table is derived from AC-Solver (MIT, `NOTICE`).
