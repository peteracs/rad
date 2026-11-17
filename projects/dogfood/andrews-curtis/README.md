# Andrews-Curtis: trivializing Miller-Schupp presentations

The Andrews-Curtis conjecture (1965) says that every balanced presentation of
the trivial group can be turned into `<x, y | x, y>` by AC moves: multiply one
relator by another, invert a relator, conjugate a relator. It is open. The
candidates for a counterexample are concrete presentations nobody has managed
to trivialize, and a trivialization is a short list of moves anyone can check.

This project searches for such lists. The test bed is the 1,190 Miller-Schupp
presentations `<x, y | x^-1 y^n x = y^(n+1), x = w>` with `n <= 7` and
`|w| <= 7` from Shehper et al., *What makes math problems hard for
reinforcement learning: a case study* (arXiv:2408.15332). Their greedy search
(one million nodes per presentation) solved 533 and left 657.
`data/miller_schupp.tsv` is their table, converted from the MIT-licensed
[AC-Solver](https://github.com/shehper/AC-Solver) repository.

## What is here

| File | What |
|---|---|
| `ac.rad` | Best-first search on total length. Every state generates all its children at once. |
| `ac_levels.rad` | The same steps, generated lazily by the total of the child. Far fewer states per solution. |
| `ac_explore.rad` | The same search as one call to `explore`: `solve` a presentation, list every pair a search `reach`es, or print the `path` to one of them. |
| `atlas.rad` | A saved world of classes (`atlas.radw`). `grow` continues a class across runs; `probe` searches from a presentation until it meets a class. |
| `words.rad` | Cyclic words in F(x, y): inverse, canonical rotation, product, Whitehead automorphisms. |
| `verify.py` | Checks a printed path. Shares no code with the searches. |
| `sweep.py`, `atlas_sweep.py` | Run a search over the table, 30 s wall limit per run, and verify every path. |
| `explore_sweep.py` | Run `ac_explore.rad solve` over the table with a budget of worlds, and verify every path. |
| `explore_meet.py` | Compare what the searches of different presentations reach, and turn every shared pair into a verified chain. |
| `novelty.py`, `export.py`, `check_certificate.py` | Compare with the literature; turn paths into pure AC-move certificates; check them. |
| `summarize.py` | Count what the paths show, without the comparison; `--classes` lists the classes. |
| `paths/` | The verified paths. `results.tsv`, `atlas_results.tsv`: one line per presentation tried. |

Every Rad script stops itself after 20 s, except `ac_explore.rad`, whose
budget is a number of worlds: the same arguments give the same result on any
machine.

## The two steps

A state is a pair of cyclic words (each relator up to rotation and
inversion).

- **product**: one relator `a` becomes the cyclic reduction of `u * v`, with
  `u` a rotation of `a` and `v` a rotation of the other relator or its
  inverse. This is a composition of AC moves.
- **aut**: both relators are rewritten by a Whitehead automorphism of the free
  group (`y -> yx`, `y -> yX`, `x -> xy`, `x -> xY`) when that makes the pair
  shorter. `explore_meet.py` also writes steps that only rename generators
  (`flipx`: `x -> X`, `flipy`: `y -> Y`, `swap`: `x <-> y`) where two searches
  reach the same pair written in different letters.

Automorphisms are not AC moves, and the published search does not use them.
They are sound for this question. If `phi(Q)` is carried to `F` by AC
moves, then `Q` is carried to `phi^-1(F)` by the same moves pulled back
through `phi^-1`. So a path with aut steps from `<r1, r2>` to `<x, y>` shows
that `<r1, r2>` is AC-equivalent to `<psi(x), psi(y)>` for an automorphism
`psi`; that pair is a basis, and Nielsen moves take every basis to `<x, y>`.
A path to a class core `K` shows AC-equivalence to `psi(K)`, which is
AC-trivial exactly when `K` is. `export.py` carries out this construction.

## What Rad is used for

- **The world is the search.** Each presentation reached is an entity named
  by its state. An indexed field is the priority queue: `lookup` on it returns
  the next state to visit.
- **The ledger audits the path.** Each visit is an event. A path is read from
  `from` fields the program wrote, and each hop is then checked against
  `why()`: the ledger must say the state was spawned while handling the visit
  of the state it claims to come from. The header reports `ledger_hops`, which
  must equal `steps`.
- **A saved world is a search that can be resumed.** `atlas.rad grow` loads
  the world, continues one class from its queue, and saves. A class explored
  for a minute is three 20-second runs. `probe` loads the same world and stops
  when it reaches any state in it.

- **`explore` is the search.** In `ac_explore.rad` the world is one
  presentation and a system offers the presentations one step away; the
  frontier, the visited set, the path and the parallelism belong to `explore`.
  Its `"collect"` option returns every pair a search reaches, so two searches
  can be compared: a pair that both reach joins the two presentations, and
  because the search is deterministic, asking each for the path to that pair
  finds it again.

## Running it

```sh
rad ac_levels.rad -- XyyyxYYYY XyxyyyXy | python verify.py XyyyxYYYY XyxyyyXy
python sweep.py --rad <rad> --only unsolved --states 100000 --jobs 8
python atlas_sweep.py --rad <rad> grow AK3 xxxYYYY xyxYXY --rounds 4
python atlas_sweep.py --rad <rad> probe
rad ac_explore.rad -- solve xxYYY xyxYXY | python verify.py xxYYY xyxYXY
python explore_meet.py --rad <rad> --sources all --worlds 20000 --jobs 8
```

## Results

Of the 657 presentations the published greedy search left unsolved:

| n | trivial, certificate here | trivial, by Theorem B and a chain here | with an AK(k) | in an open class | total |
|---|---|---|---|---|---|
| 2 | 0 | 0 | 0 | 32 | 32 |
| 3 | 2 (0 new) | 0 | 34 (20 new) | 36 | 72 |
| 4 | 24 (18 new) | 0 | 22 (8 new) | 72 | 118 |
| 5 | 31 (19 new) | 0 | 16 (2 new) | 88 | 135 |
| 6 | 28 (14 new) | 0 | 14 (0 new) | 104 | 146 |
| 7 | 8 (8 new) | 14 (0 new) | 14 (0 new) | 118 | 154 |
| all | 93 (59 new) | 14 (0 new) | 100 (30 new) | 450 | 657 |

This is the output of `python novelty.py`.

- **trivial, certificate here**: a verified path to `<x, y>`.
- **trivial, by Theorem B and a chain here**: a verified chain to
  `MS(7, y x y^-1 x^-1)` (class `ms1037`), which is the image of Theorem B's
  `MS(7, y^-1 x y x^-1)` under `y -> y^-1`.
- **with an AK(k)**: a verified chain to `AK(n)`, so the two are AC-trivial
  together or not at all.
- **in an open class**: a verified chain to the first table row of its class.
  None of the results compared here settles any of the 120 classes. Each has
  at least two presentations.

**What "new" means.** A published statement about a presentation is also a
statement about its image under any automorphism of the free group. Five
automorphisms map the first Miller-Schupp relator to a conjugate of itself or
of its inverse (`y -> y^-1`, `x -> x y^(+-1)`, `x -> y^(+-1) x`), so they map
table rows to table rows. `novelty.py` counts a result as new only if the
presentation is neither published nor such an image. Of the 93
trivializations, 3 are published and 31 are images; of the 100 presentations
with an AK(k), 35 are published and 35 are images. An earlier version of this
README counted the images as new (78 trivializations, 82 joins) and called
`ms1037` open.

**The classes.** Under the five automorphisms alone the 657 presentations
fall into 182 orbits, without any search. The 450 presentations in open
classes are 147 orbits, and the chains join them into 120 classes: 252 chains
consist of automorphism steps only and repeat the orbit, 78 cross to another
orbit and use 3 to 16 products each. Nothing shows that two of the 120
classes are different, so 120 is an upper bound on the number of separate
questions.

**What the meeting search added.** `explore_meet.py` at 20,000 worlds per
search put every one of the 458 presentations that were open before it into a
class: 13 are trivial (they met table row 891, which was already trivialized;
all 13 are `MS(6, y^-1 x y x^-1)` or its images, so the fact is Theorem B's
and only the certificates, of 10,832 to 29,042 AC moves, are new), 5 joined
`ms1040`, and 440 formed 119 new classes. `explore_sweep.py --strategy level`
with the same budget trivialized none of the 458, and no open presentation
met an AK(k) class or a row the greedy search had solved.

The earlier classification (`classify.py`, 20 s per search) was stopped by
hand after three rounds.

## Certificates

`python export.py` turns every path into a sequence of elementary AC moves
(multiply, invert, conjugate by a letter) with the automorphisms removed;
`python check_certificate.py certificates` replays them using only free
reduction, and shares no code with the searches or the exporter.
`novelty.py --table` writes `certificates/results.tsv`, and `make_release.py
<dir>` assembles the standalone release in `release/` (README, licenses,
citation, a short note in `release/paper/`), which needs no Rad.

## What the checker guards against

The first version of the automorphisms was wrong: the inverse letter was
mapped to `Xy` where `(yx)^-1 = XY` was needed, in the search and in the
checker alike. It "trivialized" AK(3) in 13 steps. The checker now

- computes the image of an inverse letter from the image of the generator, so
  the table cannot fail to be a homomorphism;
- requires every state on a path to abelianize to the trivial group
  (exponent-sum determinant +1 or -1), which that bug violated at step 4.

A path is still only as good as the two step rules above. They are stated in
`verify.py` so they can be checked by reading forty lines.

## Limits

- "New" means: not covered by the greedy and BFS lists, Theorem B or
  Theorem 10 of arXiv:2408.15332v2, or by an image of those under the five
  automorphisms above. That paper also relates four length-14 presentations
  to AK(3) without listing them; one join here has length 14. Results
  published elsewhere are not compared.
- A JOINED result says two presentations are trivializable together or not at
  all. It does not say which.
- AK(3), `<x, y | x^3 = y^4, xyx = yxy>`, is not trivialized here, and no
  presentation with n = 2 is.
- The classes depend on the budget: a larger one can only merge classes.
  Without the renaming steps, 276 of the 458 presentations met another one at
  2,000 worlds and 290 at 20,000; with them all 458 do at 20,000.
- The searches are not exhaustive below any length: a child can come out
  shorter than the level that generates it.
