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

### Fixed-cardinality closure exchange

Greedy deletion can make an early structural choice that no later deletion
can repair. The generic lattice kernel therefore also exposes an exchange
move:

```text
adjoin one missing member
    -> add every required OR with the current family
    -> legally delete the same number of members
    -> retain the original cardinality and exact closure
```

RAD owns the exchange seeds and explores the resulting worlds through
`simulate_many()`. The kernel returns exact profiles and never contains a
Frankl verdict. An exhaustive first-neighbor pass over all 8,141 missing
members takes about 6.3 seconds at the 51-set boundary. It reduced the number
of maximum-frequency coordinates from five to three in one basin, and from
three to two in the balanced basin, while preserving the `+13` global margin.

The balanced 51-set certificate has frequencies

```text
[30, 32, 32, 29, 28, 29, 26, 31, 31, 28, 27, 29, 25].
```

This suggests a necessary pairwise diagnostic. For coordinates `i,j`, split
the family into the four incidence cells. Then

```text
both(i,j) - neither(i,j)
    = frequency(i) + frequency(j) - family_size.
```

If both coordinates are strict minorities in an odd-sized family, the right
side is negative, so every coordinate pair in a counterexample must have
`neither > both`. In the balanced certificate all 78 pairs fail this necessary
condition. Its worst pair has 21 `both` sets and 8 `neither` sets; 121
one-sided cross-pairs have 14 distinct unions, all forced into the `both`
cell by closure.

`pair_pressure.rad` emits one typed proposal per coordinate pair. Its resolver
combines all 78 simultaneous causes, a Candidate Constraint checks the exact
audit cardinality and closure of every cross-union, and `why()` renders the
fan-in. A separate pair-bias-guided universe reduces the failing pairs from
78 to 14, but only by creating rare coordinates with frequencies
`1,2,3,4,...`. Thus the two search basins expose the obstruction in both
directions:

```text
uniform coordinate density
    -> widespread both-cell pressure

low pair pressure
    -> rare-coordinate chains and severe frequency imbalance
```

The same dogfood found and fixed a general compiler defect: pure/read-only
helpers were compiled before their causal callers and could hide an in-place
heap opcode behind an otherwise pure call. Such helpers now receive the same
functional lowering as settlements, laws, resolvers, and constraints. The
fix is effect-based and contains no conjecture-specific branch.

The exchange kernel now supports exploratory walks and a bounded repair beam.
Exploratory worlds may cross a strict local regression while every
intermediate remains exactly union-closed and fixed-cardinality. The repair
beam considers alternative legal deletion sequences after closure restoration.
Ranking exact moves before cloning dense witness state reduced the identical
width-8 repair campaign from 37.9 seconds to 14.4 seconds (2.63x) with the same
independently verified family hash. A valley-crossing trajectory tightened the
51-set frequency range from `25..32` to `28..32`; the maximum frequency remains
32, so this is a stronger near-witness rather than a counterexample.

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

## Multi-orbit regular universes

`orbit_search.rad` continues beyond the exact one/two-orbit class with forked
three-, four-, and six-orbit worlds. Rotation acts transitively on the 13
coordinates, so every generated family has one common element frequency. The
search therefore optimizes one exact density instead of hiding a dominant
coordinate inside a scalar score.

After excluding the already-classified singleton orbit/full-cube equality
attractor, 47,361 candidates per lane produced no negative margin. The best
six-orbit basis is

```text
[3, 5, 9, 17, 33, 65],
```

the six cyclic distance classes of all two-element subsets. Its closure is

```text
{empty} union {A : |A| >= 2},
```

with 8,179 members, uniform frequency 4,095, and margin `+11`.

This yields an exact structural coordinate for every transitive family. If
its complement deletes `d` sets with total rank `R`, uniformity gives deleted
incidence `R/13` per coordinate, hence

```text
Frankl margin = d - 2R/13.
```

A regular counterexample must therefore delete sets of average rank strictly
greater than 6.5. The candidate above deletes only the 13 singleton sets, so
`d=13`, `R=13`, and the margin is `13 - 2 = 11`. One Causal-Laws proposal per
missing rotation orbit feeds a resolver; Candidate Constraints verify the
partition and dual identity, and `why()` renders the fan-in. The independent
`verify_orbit_search.py` checker regenerates the closure and reports SHA-256
`8eb0a6ebd79fc70c837223ba2a4f23aa4a74533790a0d49981e97b6a9ddea1ff`.

## Exact cyclic-invariant class at width 13

The orbit heuristic suggested a stronger exact coordinate system. A family
invariant under cyclic rotation is a union of binary-necklace orbits, so the
8,192 membership decisions collapse to 632 orbit decisions. For orbit
variables `x_i`, union closure is a Horn theory:

```text
orbit i selected
+ orbit j selected
    => every orbit containing A union B is selected.
```

`regular_maxsat_solver.py` generates all 1,895,650 non-tautological Horn
clauses. Since rotation is transitive, each selected orbit contributes the
same incidence to every coordinate; the Frankl margin is therefore one exact
weighted objective. RC2/CaDiCaL finds these sharp optima:

```text
width   necklace orbits   Horn clauses   proper separating minimum
  11          188           121,317                    +1
  12          352           481,907                    +1
  13          632         1,895,650                    +1
```

At width 13 the optimization took 154.75 seconds. The extremal is

```text
P([13]) - {empty},
```

with 8,191 sets and common frequency 4,096. Thus **no proper separating
cyclic-invariant width-13 family is a counterexample**, and the `+1` bound is
sharp. Without separation, duplicated-coordinate quotients can attain margin
zero, which is why the minimal-counterexample reduction is explicit rather
than silently assumed.

`regular_proof.rad` reads the compact certificate, independently recomputes
the extremal frequencies, closure, separation, and necklace count, then emits
three simultaneous evidence causes:

```text
weighted optimum
+ complete Horn theory and digest
+ independently checked extremal witness
        -> RegularClassProof
        -> Candidate Constraint
        -> atomic commit
```

`why()` preserves that fan-in through the external event instance. Generic
native `bitmask_rotation_orbit` and `bitmask_rotation_representatives` kernels
reduced this audit from roughly 7.0 seconds to 0.68 seconds and also accelerate
the multi-orbit search. `verify_regular_proof.py` independently regenerates
the orbit partition and Horn digest.

Because 13 is prime, this finite theorem covers more than families presented
with a chosen cyclic action. If a permutation group acts transitively on 13
coordinates, orbit--stabilizer makes its order divisible by 13; Cauchy's
theorem gives an element of order 13, and that element is a 13-cycle. Hence
the exact certificate proves Frankl for **every transitive union-closed family
on 13 coordinates**. `regular_proof.rad` includes this group-theoretic
reduction as a fourth causal input.

This is not a proof of Frankl's conjecture. The transitive-family case across
all degrees remains open. Aaronson, Ellis, and Leader proved the narrower
family generated by the cyclic translates of one fixed set in
[A note on transitive union-closed families](https://www.combinatorics.org/ojs/index.php/eljc/article/view/v28i2p3).

## Forbidden nontransitive automorphisms

`permutation_symmetry_solver.py` removes the transitivity requirement. A
coordinate permutation is supplied by its cycle type; the cyclic group it
generates partitions both coordinates and the Boolean cube. The exact theory
then has:

```text
one family variable per subset orbit
one Horn implication per orbit-level union requirement
one strict-minority circuit per coordinate orbit
```

The first exact width-13 sweep excludes these automorphism types:

```text
cycle type   subset orbits   Horn clauses   result
(10,3)             432          650,081     UNSAT
(9,4)              360          433,824     UNSAT
(8,5)              288          263,448     UNSAT
(7,6)              280          253,931     UNSAT
(7,5,1)            320          266,105     UNSAT
```

Thus a separating counterexample cannot be invariant under any listed
permutation. This begins a concrete asymmetry theorem rather than another
random search. `symmetry_proof.rad` treats the five solver universes as
simultaneous causes, Candidate Constraints reject a malformed partition or a
positive witness claim, and `why()` retains all five proof branches. The
harder `(12,1)` and `(11,2)` cases exceeded the bounded ten-minute run and are
explicitly not included in the certificate.

The timeout led to a proof-level reduction. Suppose one cycle has length `L`
coprime to the least common multiple of all other cycle lengths. Freeze the
membership pattern on the outside coordinates. Every resulting layer is
union-closed. Raising the automorphism to the outside period fixes that
pattern and, by coprimality, still acts as a full `L`-cycle on the chosen
coordinates. The unrestricted cyclic theorem at width `L` gives average layer
rank at least `L/2`. Summing over layers makes every coordinate on that cycle
abundant on average, contradicting a strict counterexample.

This coprime-cycle layering lemma closes `(12,1)` and `(11,2)` without solving
their large CNFs, and also explains all five completed solver exclusions.
`regular_maxsat_solver.py` has exact unrestricted minimum `0` certificates for
every cyclic width 2 through 12. Applying the lemma across the 101 integer
partitions of 13 excludes 63 of the 100 nonidentity permutation cycle types;
only 37 remain possible for an automorphism of a separating counterexample.

`layering_proof.rad` combines eleven cyclic-width certificates and the
target-width reduction as twelve simultaneous causes. Its resolver records a
complete width bitmask, digest coverage, 644,794 Horn clauses, and the exact
`63/37` partition; Candidate Constraints enforce those identities and
`why()` preserves the proof fan-in.

## Layered exact obstruction solver

`dual_obstruction_solver.py` now learns two levels of exact closure clauses.
For every deleted set `U`, the surviving subsets of `U` cannot contain two
sets with union `U`. Complementing inside `U` makes them pairwise intersecting,
so at most half survive. Equivalently:

```text
U deleted => at least half of P(U) is deleted.
```

The solver lazily adds this aggregate subcube-density cut when a speculative
model violates it, and adds individual union-decomposition clauses only after
all aggregate cuts pass. A finite-domain SAT run proves the width-6 instance
UNSAT in 53.1 seconds using 44 aggregate cuts and 497 residual clauses. At
width 13 the base pseudo-Boolean model still times out under the bounded run;
no proof or counterexample is claimed there.

## Join-generator quotient frontier

The most productive coordinate change has been to stop searching directly in
the `2^8192` family space.  If a union-closed family is generated under union
by `g` join-generators, every ground coordinate `x` has an incidence column

```text
C_x = { i : generator i contains x }.
```

A subset of the generators produces one family member; two generator subsets
produce the same member exactly when they hit every selected incidence column
in the same way.  The family is therefore a quotient of the `g`-cube.  A
coordinate whose column has weight `k` is absent from at most `2^(g-k)`
distinct quotient members.  This elementary bound, product factorization of a
disconnected incidence hypergraph, and the published 50-set/12-coordinate
frontiers reduce the first unresolved generator cases to small finite
quotients.

`generator_frontier.rad` audits a complete exact scan for at most seven
join-generators.  The scan covers every possible number of distinct columns,
not only ground width 13:

```text
labelled column configurations   191,718,188
symmetry orbits                       55,530
Pareto frontier orbits                   945
minimum Frankl margin                    +18
```

Consequently every union-closed family with at most seven join-generators
satisfies Frankl's conjecture.  The native implementation evaluates canonical
column quotients; RAD supplies the immutable proof inputs, typed evidence,
one owning resolver, a Candidate Constraint, record/replay, and the `why()`
fan-in.  `verify_generator_frontier.py` independently checks the compact
coverage certificate.

For eight generators, disconnected incidence support factors into smaller
generator components and is already covered.  The weight-one/weight-two
stratum is a vertex-coloured graph.  A nauty enumeration followed by the
generic quotient kernel gives:

```text
connected coloured graph orbits       2,038,236
orbits with at least 13 columns        1,992,040
smallest quotient family                       80
minimum observed Frankl margin                +20
counterexamples                                  0
```

Thus an eight-generator survivor with 51--63 members must have connected
support, maximum incidence-column weight three, and at least one triple
column.  `generator8_frontier.rad` combines the published boundary, product
factorization, multiplicity bound, and graph scan as simultaneous causal
evidence rather than hiding the reduction in a procedural schedule.

### Projected triple quotients

For a selected triple column `T`, strict minority has a particularly small
local meaning.  Delete the three incident generators and examine the quotient
on the five outside generators.  If the whole family has `m` members, that
outside quotient must have at least

```text
floor(m / 2) + 1
```

states.  The generic `projected_partition_frontier` kernel exhaustively
enumerates all inclusion-minimal systems of at most 12 tests of weight at most
three on this five-cube.  Canonicalization by all 120 permutations leaves:

```text
required states          26   27   28   29   30   31   32
labelled minimal cores 2356 1067  366  131   31    6    1
symmetry orbits           39   23   11    6    3    2    1
```

There is also a short analytic shadow of the enumeration.  If `r` singleton
tests occur, the full outside set and every full-minus-one set corresponding
to a missing singleton collide.  Hence the quotient has at most `27+r`
states.  Required state counts 28 through 32 therefore force 1 through 5
singleton traces respectively.  `projected_partition_proof.rad` settles the
seven exhaustive frontiers plus this collision lemma, Candidate Constraints
check all exact totals, and `why()` preserves the eight-way explanation.

The projected identity produced a much smaller exact SAT encoding.  At one
fixed family size, strict minority can be expressed directly through the
outside quotient instead of almost one million guarded global cardinality
variables.  A validation slice fell from about 987,000 variables to 185,000
and solved in 7.9 seconds.  Minimal projected cores are also fed back as
redundant, theorem-preserving SAT cuts.

The complete exact thirteen-column manifest contains 82 independently hashed
CaDiCaL runs and 240,687,998 clauses across the runs.  It excludes every triple
count 1 through 13 and every legal singleton/edge remainder.  The hardest
five-triple/no-singleton stratum is split over every exact family size 51
through 63.  The final eleven-triple/no-singleton case was also reproduced as
six disjoint fixed-triple trace-count universes; parallelizing that invariant
split reduced its wall-clock reproduction from roughly 2,231 seconds to a
maximum slice time of 284 seconds.  Combining this sweep with the graph
frontier proves that **no eight-join-generator counterexample exists on exactly
13 distinct ground coordinates**.

`generator8_q13_exclusions.rad` turns all 82 solver universes plus the manifest
into simultaneous typed proposals.  Its resolver reconstructs every
triple/singleton/family-size/trace-partition coverage mask, the Candidate
Constraint rejects any missing slice, `why()` explains the 83-way fan-in, and
record/replay verifies the committed proof digest.

These are genuine finite theorem-class exclusions, but the CaDiCaL UNSAT
records currently carry deterministic CNF hashes rather than independently
checkable DRAT/LRAT proof logs.  The next certificate-hardening step is proof
logging.  The theorem also does not cover eight-generator families with more
than 13 ground coordinates.  No full proof or counterexample is claimed.

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
