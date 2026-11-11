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
| `atlas.rad` | A saved world of classes (`atlas.radw`). `grow` continues a class across runs; `probe` searches from a presentation until it meets a class. |
| `words.rad` | Cyclic words in F(x, y): inverse, canonical rotation, product, Whitehead automorphisms. |
| `verify.py` | Checks a printed path. Shares no code with the searches. |
| `sweep.py`, `atlas_sweep.py` | Run a search over the table, 30 s wall limit per run, and verify every path. |
| `paths/` | The verified paths. `results.tsv`, `atlas_results.tsv`: one line per presentation tried. |

Every Rad script stops itself after 20 s.

## The two steps

A state is a pair of cyclic words (each relator up to rotation and
inversion).

- **product**: one relator `a` becomes the cyclic reduction of `u * v`, with
  `u` a rotation of `a` and `v` a rotation of the other relator or its
  inverse. This is a composition of AC moves.
- **aut**: both relators are rewritten by a Whitehead automorphism of the free
  group (`y -> yx`, `y -> yX`, `x -> xy`, `x -> xY`) when that makes the pair
  shorter.

Automorphisms are not AC moves, and the published search does not use them.
They are sound for this question. AC moves commute with automorphisms, so a
path with aut steps from `<r1, r2>` to `<x, y>` shows that `<r1, r2>` is
AC-equivalent to `<psi(x), psi(y)>` for an automorphism `psi`. That pair is a
basis of the free group, and Nielsen moves, which are AC moves, take every
basis to `<x, y>`.

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

## Running it

```sh
rad ac_levels.rad -- XyyyxYYYY XyxyyyXy | python verify.py XyyyxYYYY XyxyyyXy
python sweep.py --rad <rad> --only unsolved --states 100000 --jobs 8
python atlas_sweep.py --rad <rad> grow AK3 xxxYYYY xyxYXY --rounds 4
python atlas_sweep.py --rad <rad> probe
```

## Results

Of the 657 presentations the published greedy search left unsolved:

| n | TRIVIAL | JOINED AK3 | JOINED AK4 | JOINED AK5 | JOINED AK6 | JOINED AK7 | JOINED ms1037 | JOINED ms1040 | JOINED ms891 | UNSOLVED | total |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 32 | 32 |
| 3 | 2 | 34 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 36 | 72 |
| 4 | 24 | 0 | 22 | 0 | 0 | 0 | 0 | 0 | 0 | 72 | 118 |
| 5 | 31 | 0 | 0 | 16 | 0 | 0 | 0 | 0 | 0 | 88 | 135 |
| 6 | 14 | 0 | 0 | 0 | 14 | 0 | 0 | 0 | 1 | 117 | 146 |
| 7 | 8 | 0 | 0 | 0 | 0 | 14 | 14 | 5 | 0 | 113 | 154 |
| all | 79 | 34 | 22 | 16 | 14 | 14 | 14 | 5 | 1 | 458 | 657 |

TRIVIAL: a verified path to `<x, y>` that the published greedy search did not
find. Paths have 15 to 86 steps (median 28) and take a median of 160 ms to find.

JOINED *c*: a verified chain to class *c*'s starting presentation, so the two
are trivializable together or not at all. AK3 to AK7 are `AK(n)` with that
`n`; `ms<i>` classes start at row `i` of the table; that starting presentation
(ms891, ms1037, ms1040) is counted in its own column, so 117 presentations
are joined to a class and 3 are class cores. Every join so far is between presentations with the same first
relator. The 458 UNSOLVED rows are open here: the searches stopped at their
20 s limit without reaching a class.

The classification was stopped by hand after three classifier rounds; rerun
`python classify.py --rad <rad>` to continue it.

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

- "New" means: not in AC-Solver's list of presentations its greedy search
  solved. The same paper solves a few more by other means and proves some
  infinite families trivial; this project has not compared against those.
- A JOINED result says two presentations are trivializable together or not at
  all. It does not say which.
- AK(3), `<x, y | x^3 = y^4, xyx = yxy>`, is not trivialized here.
- The searches are not exhaustive below any length: a child can come out
  shorter than the level that generates it.
