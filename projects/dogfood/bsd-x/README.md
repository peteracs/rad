# Audit of the designated BSD X-lemmas

**Arithmetic deformation source:** [First-order reduction](SELMER_JET_SOURCE.md)
proves that at corank at least m, higher lifts cannot repair the degree-m
determinant coefficient. The leading term is controlled by induced maps from
kernel to cokernel. Arithmetic admissibility and analytic nonvanishing remain
unproved. Run `python projects/dogfood/bsd-x/verify_selmer_jet.py`.

**Adaptive layer decision:** [Optimal stopping bound](ADAPTIVE_LAYER_DECISION.md)
proves that exact length and sequential layer observations decide the fixed-module
rank target by `max(0, delta-m-1)` queries, using supplied parity. The cutoff is
sharp for abstract modules. This does not prove that every curve admits a prime
with a positive outcome. Run `python projects/dogfood/bsd-x/verify_adaptive_layers.py`.

**Mixed-coefficient experiment:** [Polarized comparison target](POLARIZED_MECHANISM.md)
extracts complementary minors from independent determinant perturbations.
An arithmetic nonvanishing identity connecting these coefficients to the
complex leading term is proposed but unproved. Run
`python projects/dogfood/bsd-x/verify_polarized.py`.

**Joint source/correction audit:** [Budget elimination](BUDGET_ELIMINATION.md)
proves that existence of a valid transfer budget is equivalent to s_p<=m.
Symbolic elimination exposes the surviving arithmetic premise; it does not
prove it. Run `python projects/dogfood/bsd-x/verify_budget_elimination.py`.

**Automatic quotient transfer:** [Exact correction rules](QUOTIENT_TRANSFER.md)
clarify that upper bounds transfer immediately, while identities require
subtracting length(TX). Finite higher-layer information can certify enough
correction for a weaker source bound. No complex comparison is manufactured.
Run `python projects/dogfood/bsd-x/verify_quotient_transfer.py`.

**After the supplied Iwasawa construction:** [Exact rank target](EXACT_RANK_TARGET.md)
replaces the excessive determinant-length bound by a unit-minor criterion.
The quotient X/TX also yields an arithmetic presentation with order exactly
s_p, but changes the original module. The complex analytic comparison remains
unproved. Run `python projects/dogfood/bsd-x/verify_fitting_rank.py`.

**Derived pairing experiment:** [Deformation and vanishing](DEFORMATION_PAIRING.md)
constructs an induced kernel-to-cokernel map from a formal matrix, computes
corrected higher terms and proves the general determinant/corank inequality.
The Selmer realization and complex analytic comparison remain unproved.
Run `python projects/dogfood/bsd-x/verify_deformation.py`.

**Construction recipes:** [Logarithmic and pairing hypotheses](MAP_CONSTRUCTION_HYPOTHESES.md)
give explicit candidate formulas, exact additivity defects, and an orthogonal
kernel decomposition. The arithmetic pairing and analytic vanishing theorem
remain unproved. Run `python projects/dogfood/bsd-x/verify_map_construction.py`.

**Arithmetic-map diagnostic:** [Required comparison theorem](ARITHMETIC_MAP_REQUIREMENTS.md)
and `selmer_observability.rad` expose exact kernels, dimension-budget failures,
and the distinction between rational and modular rank. No actual Selmer map
is constructed. Run `python projects/dogfood/bsd-x/verify_observability.py`.

**Independent hypothesis iteration (no web):** [Termination synthesis](INDEPENDENT_TERMINATION.md)
tests a global ranking-function hypothesis and an explicit block-decrease
repair. The first fails; the second gives a conditional bound but has an
existence premise equivalent to the missing X3 assertion. No X is filled.
Run `python projects/dogfood/bsd-x/verify_termination.py`.

**Universal inference audit:** [Global X3 bridge audit](GLOBAL_BRIDGE_AUDIT.md)
checks symbolic families for all nonnegative integer parameters and identifies
the complex-rank comparison absent from the listed structural premises. This
does not assert realizable curve counterexamples or fill an X. Run
`python projects/dogfood/bsd-x/verify_global_bridge.py`.

**Revised X3:** [One-level certificate](SINGLE_LEVEL_X3.md) proves that
`C(N) < p^[N(m+1)]` controls all subsequent growth using injectivity of
successive torsion quotients. With m independent points it also proves primary
tail saturation. Its universal existence premise remains unproved. Run
`python projects/dogfood/bsd-x/verify_single_level.py` for the RAD execution audit.

**New:** [Finite leading-term certificate](FINITE_X6_CERTIFICATE.md) proves a
conditional reduction of X6 to a quantitative uniform bound and finite local
certificates, with a RAD application and independent verification. It does not
fill a universal X. The audit below concerns the original five-placeholder
draft; in the revised four-placeholder draft X1 asks directly for rational
points and X4 is absent. Keep these two versions distinct.

No complete unconditional replacement for X1, X3, X4, X5, or X6 was found.
There is no X2 placeholder in the supplied text. This application checks
implications of the stated Selmer-module formulas and explains the missing
arithmetic information. It neither constructs elliptic curves nor verifies
theorems about L-functions. The abstract witnesses below are not counterexamples
to BSD and are not asserted to be realizable by elliptic curves.

Run `python projects/dogfood/bsd-x/verify.py` from the repository root.
The runner checks 1,408 isolated RAD forks, requested-input agreement,
parent-world preservation, deterministic execution with one and four workers,
record/replay, independent integer calculations, and rejection of a forged X1
conclusion. `selmer_model.why.txt` records the actual WHY and field-provenance
outputs. Those explain checked computations; they are not proofs of the X's.

## Exact reductions, with proofs

Take the Kummer formula, cofinitely generated primary decomposition, alternating
finite quotient, and parity in the supplied argument as premises. Write

    C_p(k)=p^[k s_p+2 sum_j min(k,a_j)],  s_p=r+t_p.

### X1 is a corank lower bound

For a finite-dimensional vector space V over a field, its m-th exterior power
is nonzero if and only if dim(V)>=m. A basis proves both implications. Thus X1
is exactly existence of a good prime p>=5 with s_p>=m, for each curve of
analytic rank m>=4. Under X4's finiteness conclusion it is exactly r>=m.
Writing down an unspecified nonzero exterior class has not supplied the lower
bound: its existence is the lower bound.

### X3 is a corank upper bound

At a fixed eligible prime p, put b_k=#{j:a_j>k}. The exact growth exponent is
s_p+2b_k. If this is <=m+1, parity gives s_p+2b_k<=m, hence s_p<=m.
Conversely, if s_p<=m, choose k>=max_j a_j, taking k=0 for an empty quotient.
Then b_k=0 and the X3 inequality holds. Therefore

    X3 for E <=> some good p>=5 has s_p<=m.

This equivalence does not assume global finiteness of Sha. Under X4 it reduces
to r<=m, since s_p=r for every p and there are good primes >=5.
Searching more and more levels k cannot overcome a corank that exceeds m.

### X4 is exactly global finiteness of Sha

The Kummer identity makes X4 precisely a uniform bound on #Sha[n]. The torsion
argument in the supplied text proves that this implies finiteness. Conversely,
if Sha is finite, B_E=#Sha works for every n. Thus the asserted bound is
equivalent to finiteness, with no improvement in logical strength.

For any fixed primary decomposition, increasing k detects

    log_p #Sha[p^k] = k t_p+2 sum_j min(k,a_j).

But arbitrarily long finite prefixes cannot decide t_p in general. For every
K>=1 the groups A=(Q_p/Z_p)^2 and B=(Z/p^K Z)^2 have the same p^k-torsion
orders p^(2k) for every 0<=k<=K. A is infinite and B is finite. RAD checks
this family for K through 12; the displayed formulas prove it for every K.

Even proving all primary parts finite would not suffice for global finiteness.
The abstract group direct_sum_p (Z/pZ)^2 is infinite, has a finite primary
part at every prime, and has finite n-torsion of size rad(n)^2 for each n.
A uniform bound also has to rule out nonzero primary parts at infinitely many
primes. This example has an alternating perfect pairing on each primary part.

### X5 and X6 retain the leading-coefficient content

X5 supplies rationality and positivity of the normalized leading coefficient
in higher rank. The integer module formulas contain no comparison with that
real analytic quantity. This audit does not purport to prove X5.

Once A_E is positive rational and Sha is finite, X6 for every prime is
equivalent to A_E=#Sha. Unique factorization proves this equivalence exactly
as in the supplied conclusion. Checking a finite set of primes cannot replace
the quantifier: multiplying a positive rational candidate by q^2 for an
unchecked prime q leaves all the checked valuations unchanged. Squaring also
preserves its rational square class.

## A stronger elementary consolidation: all-prime comparison implies finiteness

Here is a useful lemma that can replace the *deduction* of finiteness if an
independent leading-term comparison becomes available. It does not prove that
comparison and does not fill X4 unconditionally.

**Lemma.** Let A be a torsion abelian group with finite A[n] for every n>=2.
Suppose a is a positive rational number and, for every prime p,

    lim_(k->infinity) log_p #A[p^k] = v_p(a).

Then A is finite and #A=a.

**Proof.** Each sequence on the left is a nonnegative, nondecreasing integer
sequence with finite limit, hence is eventually constant. Since the groups
A[p^k] are nested, they eventually stabilize as groups, and their union A[p^infty]
has order p^v_p(a). In particular all valuations of a are nonnegative, so a is
an integer. Only finitely many are positive. Every torsion element decomposes
into its primary components, so A is the direct sum of those finitely many
finite primary subgroups. Its order is product_p p^v_p(a)=a. QED.

Applied using Kummer, the valuation-comparison assertion and rationality alone
would give both Sha finiteness and the formula. However, in the current draft
A_E is introduced after rank equality, whose proof uses X4. Using X6 to fill
X4 in that same order would be circular. An independent comparison theorem
would need to avoid that dependency.

## A concrete sufficient certificate for X3 from the cited literature

Kim's Theorem 1.8 applies when p>=5, the residual representation is surjective,
the Manin constant is prime to p, and the Kurihara-number collection is nonzero.
It identifies s_p with

    ord(delta)=min{nu(n): n in N_1 and delta_n != 0},

where nu(n) counts the prime factors of squarefree n and N_1 is the paper's
specified set of admissible auxiliary products.

Consequently the following **conditional certificate theorem is proved**:
choose p additionally of good reduction, as required in X3. If those
hypotheses hold and there is an admissible n with delta_n!=0 and
nu(n)<=m, then s_p<=m and hence the X3 inequality holds at some finite k.
This does not use X4. It also does not require knowing all Kurihara numbers:
one correctly certified nonzero value at sufficiently small nu(n) suffices.

The missing universal assertion would be existence of such a low-order
nonvanishing certificate for every curve in the X3 range, including an
appropriate separate treatment of CM curves. Surjectivity to GL_2(F_p) is
not an admissible blanket CM hypothesis. Nonvanishing somewhere, with no
bound on nu(n), is not this assertion. The source's rank-zero converse is not
a higher-rank analytic-to-Selmer comparison.

No modular symbols or Kurihara numbers for an actual curve were computed in
this run. No numerical nonzero value has been silently treated as certified.

## What the abstract WHY witnesses establish

* m=4,r=2,t_p=0,F_p=0: all the displayed primary structural formulas and
  parity hold, while X1's dimension conclusion fails. The low-rank converse
  antecedents s_p=0 or 1 are absent.
* m=2,r=4,t_p=0,F_p=0: the same formulas hold and primary Sha is trivial,
  while every possible X3 growth certificate fails.
* m=2,r=2,t_p=2,F_p=0: residual torsion exponents grow as 2k, so a uniform
  primary torsion bound fails despite finite torsion at each level.
* m=2,r=2,t_p=0,F_p=(Z/p^3)^2: the growth bound fails at k=2 and succeeds
  at k=3. One failed finite-level certificate does not disprove X3.

These witnesses show precisely why parity, finite-level orders, and abstract
duality alone cannot derive the desired conclusions. They do not establish
independence from the full arithmetic theory of elliptic curves.

## Primary sources checked

* [Dokchitser–Dokchitser, Annals 2010](https://annals.math.princeton.edu/2010/172-1/p11):
  the abstract explicitly states p-parity over Q for every prime.
* [Kim, final version, Theorem 1.8 and Corollary 1.11](https://arxiv.org/html/2203.12159v6):
  precise residual, Manin, and nonvanishing hypotheses; the Selmer-corank
  formula; and the rank-zero converse. Section 1.9 distinguishes these
  structural results from the full higher-rank BSD comparison.

This is a targeted audit of the X-lemmas and the cited inputs used above,
not a complete audit of all period/height normalizations and references in
the supplied manuscript.
