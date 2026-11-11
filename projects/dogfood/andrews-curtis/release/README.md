# Andrews–Curtis certificates for Miller–Schupp presentations

Machine-checkable Andrews–Curtis (AC) certificates for the Miller–Schupp
presentations

    MS(n, w) = < x, y | x^-1 y^n x = y^(n+1), x = w >,   1 <= n <= 7, |w| <= 7,

the 1,190-presentation benchmark of Shehper et al., *What makes math problems
hard for reinforcement learning: a case study* (arXiv:2408.15332). Their greedy
search solved 533 and left 657. For those 657:

| n | newly AC-trivial | newly tied to AK(n) | newly tied to another core | already known | still open |
|---|---|---|---|---|---|
| 2 | 0 | 0 | 0 | 0 | 32 |
| 3 | 1 | 27 | 0 | 8 | 36 |
| 4 | 23 | 15 | 0 | 8 | 72 |
| 5 | 31 | 9 | 0 | 7 | 88 |
| 6 | 15 | 7 | 0 | 7 | 117 |
| 7 | 8 | 7 | 17 | 7 | 115 |
| all | **78** | **65** | **17** | 37 | 460 |

- **AC-trivial**: an explicit sequence of elementary AC moves to `<x, y | x, y>`.
- **Tied to AK(n)**: an explicit sequence of AC moves to `psi(AK(n))` for a
  stated automorphism `psi` of F(x, y), where `AK(n) = <x, y | x^n = y^(n+1),
  xyx = yxy>`. Such a presentation is AC-trivial if and only if AK(n) is. For
  n = 3 this adds 27 presentations to those known to stand or fall with
  AK(3), the best-known potential counterexample.
- **Tied to another core**: the same, to `MS(7, y x y^-1 x^-1)` (13
  presentations, table row 1037) or `MS(7, y x y^2 x^-1)` (4, row 1040). Both
  cores are open; they are counted under "still open".
- **Already known**: in the published theorems (see *Novelty* below).

The certificates also cover 48 presentations the greedy search had solved.

## Files

| File | What |
|---|---|
| `results.tsv` | One row per resolved presentation: table index, n, relators, result, class, number of moves, certificate file, whether published before. |
| `certificates/*.json` | One certificate per presentation. |
| `check_certificate.py` | Checks certificates. Python 3 standard library only, about 150 lines. |
| `data/miller_schupp.tsv` | The benchmark table (from AC-Solver, MIT license; see `NOTICE`). |
| `paper/note.tex` | A short note describing the method and results. |

## Checking

```sh
python check_certificate.py certificates/*.json
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
`<x, y>`. Ties come from a saved search graph per class: a presentation is tied
when its search reaches a state already reached from the class core.

The search was written in [Rad](https://github.com/peteracs/rad) as a language
test (`projects/dogfood/andrews-curtis`); nothing here needs Rad.

## Novelty

"Already known" means one of: the greedy and BFS solutions; Theorem B of
arXiv:2408.15332v2 (MS(1, w) for all w; MS(n, w*) with w* = y^-1 x y x^-1 for
all n, also Lisitsa 2024 for n = 3..8; MS(2, w_k) with w_k = y^-k x^-1 y x y;
MS(n, w_k) AC-equivalent to AK(n)); its Theorem 10. Not compared exactly: that
paper's four length-14 presentations AC-equivalent to AK(3), which it does not
list in machine-readable form (one tied presentation here has length 14 and is
flagged in `results.tsv`); and the unreviewed
[andrews-curtis-ms14](https://github.com/alejandrozarco/andrews-curtis-ms14)
results, which concern n = 2 only. Corrections are welcome.

## License

Code: MIT (`LICENSE`). Certificates and tables: CC BY 4.0 (`LICENSE-DATA`).
The benchmark table is derived from AC-Solver (MIT, `NOTICE`).
