# Frankl union-closed-family search

`projects/dogfood/frankl-search/` uses RAD as a deterministic computational
mathematics workbench for Frankl's union-closed sets conjecture.

The conjecture states that every finite nontrivial union-closed family
\(\mathcal F\) has an element contained in at least half of its member sets.
It remains open, but a counterexample on 10, 11, or 12 ground elements cannot
exist. Vučković and Živković gave a computer-assisted proof for ground sets of
size at most 12 in
[The 12-Element Case of Frankl's Conjecture](https://ipsitransactions.org/journals/papers/tir/2017jan/p9.pdf).
Together with Lo Faro's lower bound for a minimal counterexample, this also
rules out counterexamples having at most 50 member sets; see Theorem 17 and
Corollary 19 in the
[Bruhn--Schaudt survey](https://www.uni-ulm.de/fileadmin/website_uni_ulm/mawi.inst.081/Henning/UCSurvey.pdf).

The first open ground-set size is therefore 13.

## Exact small case

`exhaustive_n4.rad` enumerates all
\(2^{2^4}=65{,}536\) labeled families over four elements. It checks the union
of every pair in each family and evaluates the Frankl frequency predicate:

```text
families_checked=65536
union_closed=4960
nontrivial_union_closed=4958
counterexamples=0
```

This result is exhaustive for four elements.

## Optimized N=13 search

`search.rad` explores exact union closures of bounded generator bases. Its hot
loops live in the project-owned `native-math-kernels` extension and cross
RAD's generic `load_extension()` boundary through four typed adapters:

```rad
lattice_closure(generators)
lattice_profile(generators, width)
lattice_frequencies(family, width)
lattice_is_closed(family)
```

For a partial union-closed family \(F\), adjoining generator \(G\) produces

```text
F' = F union {A union G : A in F}.
```

Starting at `{empty}`, this constructs the exact least union-closed family
containing the generators. The native kernel orders sparse generators first,
skips generators already implied by the partial closure, uses dense membership
for small Boolean cubes, and returns compact frequency/separation/signature
statistics without boxing every family member into speculative VM state.

The algorithms and their limits are not VM builtins. The search still uses
RAD's runtime rather than a sidecar search engine:

- `simulate_many()` explores a diverse beam of copy-on-write candidate worlds;
- every fork runs a deterministic seeded local mutation batch;
- family signatures prevent distinct bases for the same closure from filling
  the beam;
- `fork_seed()` preserves the winning rollout seed;
- `world_digest()` proves that speculation did not mutate the live world;
- a `law` proposes finalist evidence, its owning `resolver` stages it, and
  validation-only `constraint`s accept the complete candidate atomically;
- `why()` renders the accepted causal path;
- record/replay re-executes the feature-gated program from an authenticated
  trace without refiring certificate I/O.

The default release run evaluates 258,561 exact candidates, retains a complete
landscape summary, materializes the closest strict near-miss, and performs the
native pairwise audit in about 5.84 seconds on the development workstation.
The original interpreted workload took about 43.3 seconds for one rollout plus
the same 4,097-member pair audit. That is not an apples-to-apples candidate
count: it is evidence that the hot loops and speculative state shape were
actually changed, not that an audit was merely disabled.

That deterministic default landscape contains:

```text
negative margin                       0
equality                          6,896
strictly positive               251,665
nonseparating                    25,931
smallest/largest family         107 / 8,192
closest positive margin                   1
```

The closest strict family has 4,097 members and maximum frequency 2,049. In
the deletion dual its minimum surplus is `-1`: it is exactly one incidence
short of the strict deleted-majority condition. The certificate includes this
complete near-miss, its generator basis, frequencies, rank histogram, and dual
surplus vector; the independent verifier regenerates and checks it.

## Legal-deletion search and causal explanation

`deletion_search.rad` searches a complementary construction space in which
**every speculative world is union-closed**. Starting from the full Boolean
cube, it removes a member only when no pair of remaining proper parents has
that member as its union. The native extension maintains the generic state

```text
surviving members
subset counts
exact-OR witness counts
coordinate frequencies
coordinate-separation witnesses
```

incrementally. Removing one member updates the exact-union counts by scanning
only pairs containing that member, rather than rebuilding the OR zeta/Mobius
transform. The kernel contains no Frankl-specific names or verdicts: RAD owns
the fork seeds, objective selection, beam/Pareto archive, COW worlds, causal
settlement, validation constraints, and `why()` explanation.

The full 6,144-deletion trajectory to a 2,048-set family fell from 13.67
seconds with per-step transform rebuilding to 1.04 seconds with the
incremental kernel. A 3,017-world fork campaign reached the same boundary in
5.14 seconds; the earlier deep broad run took roughly 188 seconds. Native
prefix tests compare every incremental profile with the independent full
transform.

The search repeatedly finds this staircase:

```text
family size    best margin    rare coordinates
4096                 +2              1
2048                 +4              2
1024                 +6              3
 512                 +8              4
```

At size 2,048, one independently verified representative is isomorphic to

```text
(P(S) - {{a}, {a,b}})
    union {S union {u}, S union {u,v}},    |S| = 11.
```

Its frequencies are eleven values between 1,024 and 1,026 plus the rare
frequencies 2 and 1; the exact Frankl margin is `+4`. The family is
union-closed and separating. `boundary_analysis.rad` recognizes the more
general power-set-plus-chain form and settles one coordinate-balance proposal
per base coordinate, so `why()` renders the complete fan-in.

The resulting obstruction applies to the whole template, not just this
representative. If `t` core sets are removed and replaced by `t` rare-tagged
sets, closure forces every added set to contain the full base union. Every
base coordinate therefore gains `t` incidences. Making all base coordinates
strict minorities would require the removed sets to contain each coordinate
more than `t` times, requiring more than `|S|*t` incidences. But `t` subsets
of `S` contain at most `|S|*t` incidences. Thus this attractive near-boundary
shape can never cross Frankl's half threshold.

`verify_deletion.py` independently reconstructs the full family from the raw
deletion list, recomputes exact OR-convolution counts, frequencies, separation
witnesses, the algebraic deletion frontier, and a SHA-256 family digest. The
saved 2,048-set boundary is union-closed and separating, has exact margin
`+4`, and has 20 effective legal continuations.

The Pareto dogfood also keeps fixed density/objective regimes in separate
universes. At size 2,048 it exposes the tradeoff rather than hiding it behind
one score: the near-Frankl world has margin `+4` but a coordinate of frequency
1; a 40%-density world has margin about `+250`; uniformly balanced worlds are
farther away still. Current-score dominance is deliberately not used across
regimes because different legal frontiers have different futures.

## Multi-million campaign

The wider campaign used 24-, 32-, and 48-generator lanes. Each lane evaluated
1,034,241 exact closures with a family-distinct beam:

```text
generator slots   candidates   release wall time   best margin
24                1,034,241    13.74 s             0
32                1,034,241    17.09 s             0
48                1,034,241    21.11 s             0
total             3,102,723    51.94 s             0
```

No strict counterexample was found. The best family in all three lanes was
the full powerset \(\mathcal P([13])\), with 8,192 member sets and every
element occurring 4,096 times. Its exact Frankl margin is

```text
2 * 4096 - 8192 = 0.
```

This is the correct equality baseline for a counterexample search: a witness
must have negative margin. The campaign is not exhaustive over all
\(2^{8192}\) labeled families, so a negative search result is not a proof for
\(N=13\) or in general.

## Why a counterexample is hard to form

`cyclic_universes.rad` does not perform another blind random sweep. It changes
coordinates to expose the obstruction.

Let \(F\subseteq 2^{[13]}\), and let \(D=2^{[13]}\setminus F\) be its deleted
sets. Since every element occurs in exactly half of the full Boolean cube,

```text
F is a strict Frankl counterexample
    iff every element occurs in strictly more than |D|/2 deleted sets.
```

But `F` is union-closed exactly when every deleted union is blocked:

```text
for U in D and A union B = U:
    A is in D or B is in D.
```

Those requirements pull in opposite directions. Deleting all sets of rank at
least 7, while retaining the full union, gives every element a positive
deletion-majority surplus of 923, so it
has exactly the frequency shape a counterexample wants. The remainder,
however, has 15,147,132 ordered pairs whose union is missing. Closing those
unions regenerates all 8,192 sets and returns the Frankl margin to zero.

The missing-union count is computed by the general
`lattice_violation_count(family, width)` extension kernel. It uses exact OR zeta/Mobius
convolution in \(O(n2^n)\), turning union closure from a boolean verdict into a
distance signal useful to search and diagnostics.

## Exact cyclic-universe experiment

The structural dogfood also exhausts a nontrivial construction class at
`N=13`: every union-closed family generated by one or two complete cyclic
rotation orbits. There are 630 nontrivial binary-necklace representatives, so
the exact class contains

```text
630 * 631 / 2 = 198,765 families.
```

Eight `fork_with()` universes enumerate disjoint lanes through
`simulate_many()`. Each lane proposes typed evidence into one settlement; the
owning resolver combines all eight simultaneous causes, constraints check the
exact class cardinality and arithmetic, and `why()` renders the fan-in. An
ordinary event then records the completed study, so `why_resource()` reaches
the event instance as well.

The exact result is:

```text
negative margin:                  0
equality:                       630
strictly positive:         198,135
```

All 630 equality cases are the same full powerset: the cyclic orbit of a
singleton already generates every subset, and pairing it with any of the 630
orbits changes nothing. This accounts for one diagonal and 629 off-diagonal
generator pairs. Every class member that does not contain the singleton orbit
lies strictly above one half. Thus the only equality basin in this exact class
is the Boolean cube itself; every genuinely different closure moves in the
opposite direction from a counterexample.

`verify_structure.py` independently checks the certificate, dual identities,
OR-convolution count, and finalist. Its default full mode independently
re-enumerates all 198,765 families; `--quick` skips only that expensive
reproduction and is used for normal CI after the RAD exact enumeration.

## Independent certificate

The RAD program writes `out/latest.json` containing the irredundant generator
basis, complete sorted family, all element frequencies, exact verdict, search
parameters, COW result digest, winning seed, and causal settlement explanation.

`verify_certificate.py` has no dependency on the RAD VM. It:

1. regenerates the family from its basis;
2. checks basis irredundancy;
3. verifies union closure with exact OR zeta/Möbius convolution in
   \(O(n2^n)\), instead of a slow \(O(|F|^2)\) Python loop;
4. recomputes the effective ground set and all frequencies;
5. rejects every inconsistent claim through explicit exceptions that remain
   active under `python -O`.

A certificate with `counterexample: true` that passes this checker would be an
independently verifiable finite witness. The generator-basis irredundancy check
is not a “minimal antichain certificate”; minimality is unnecessary for
checking a counterexample.

Run exhaustive verification, the default search, record/replay, and the
independent checker from the repository root:

```powershell
projects/dogfood/frankl-search/run.ps1
```

For context, the best current general theorem guarantees an element in at
least \((3-\sqrt5)/2\approx0.382\) of the member sets, still below Frankl's
conjectured one half; see Alweiss, Huang, and Sellke,
[Improved Lower Bound for Frankl's Union-Closed Sets Conjecture](https://www.combinatorics.org/ojs/index.php/eljc/article/view/v31i3p35).
