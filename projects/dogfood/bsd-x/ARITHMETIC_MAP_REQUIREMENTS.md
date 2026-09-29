# What an arithmetic comparison would have to prove

No web search was used. This is an independently proposed sufficient argument
and a RAD diagnostic for candidate maps, not a construction of such a map.

Let S_p be the rational Tate-module Selmer space from the user's argument,
of Q_p-dimension s_p=r+t_p. One sufficient route to X3 would be to construct:

1. A finite-dimensional Q_p-vector space J_(E,p) with a proved bound
   dim J_(E,p)<=ord_(s=1) L(E,s).
2. A Q_p-linear arithmetic map Phi_(E,p): S_p -> J_(E,p).
3. A proof that Phi_(E,p) has zero kernel on the FULL Selmer space.

Then s_p<=dim J_(E,p)<=m by elementary linear algebra, which supplies X3
at an eligible good prime p>=5 using the previously proved certificate
equivalence. This deduction does not require prior global Sha finiteness.

The target J and the map Phi are NOT known constructions supplied here.
Calling Q_p^m an analytic space by definition proves nothing about an
arithmetic map into it. Abstract existence of an injection S_p -> Q_p^m
is itself equivalent to s_p<=m; it must not be treated as a solved sublemma.
Nor may complex Taylor coefficients be silently interpreted as p-adic
coefficients. Any comparison between these settings needs an actual theorem.

## The trick: return the direction an observation misses

The RAD application accepts small integer matrices as candidate linear
observation maps. It derives exact rank from minors and, whenever a kernel
exists, constructs an integer vector v!=0 and verifies every coordinate
of A v=0. WHY exposes that witness and the unverified arithmetic status.

For

    A = [[1,0,1], [0,1,1]],

the first two columns are independent, but the full map has kernel vector
(-1,-1,1). Thus independently observing a chosen two-dimensional subspace
does not prove that all Selmer directions are detected. In an eventual
arithmetic construction, the kernel argument must also handle any divisible
Sha directions, not just the rational-point subspace.

Adding the observation [0,0,1] makes this example injective, but changes the
target dimension from 2 to 3. It cannot establish a bound of 2 on the source
dimension. The program reports injectivity and the dimension budget separately.

For A=5I_2, reduction modulo 5 has rank zero, while A has rank two over Q
and Q_5. Thus rank loss modulo p is not a proof of a kernel over Q_p.
A full-rank minor modulo p suffices for injectivity; its absence does not
disprove injectivity. Integer entries let the application distinguish these
cases exactly, without relying on floating-point rank tests.

For A=I_2, both linear-algebra conditions pass. Even then the application
does NOT claim X3: no actual arithmetic map or analytic dimension theorem
has been attached. Its resolver rejects a fabricated claim of that binding.

## Scope and execution

The 729 forks exhaust 2-by-3 matrices with entries -1,0,1. They are synthetic
linear algebra, not Selmer matrices or elliptic-curve examples. The program's
m=2 dimension budget is a diagnostic parameter, not a proof of analytic rank.
The sufficient arithmetic theorem above has arbitrary m; the program does
not formally prove it in a foundational kernel.

The current input is limited to at most 3-by-3 integer matrices with entries
between -5 and 5. Those limits are explicit. Rational matrices could be
cleared of denominators in a separate implementation; arbitrary p-adic
entries would require certified precision and valuation bounds.

Run from the repository root:

    python projects/dogfood/bsd-x/verify_observability.py

The verifier uses independent fraction-based Gaussian elimination and finite
field elimination, checks every emitted kernel, compares one/four workers,
replays a recorded execution and checks forged arithmetic-status rejection.

The result is a diagnostic for a proposed arithmetic argument, not evidence
that such an argument has been discovered. No universal X is filled.
