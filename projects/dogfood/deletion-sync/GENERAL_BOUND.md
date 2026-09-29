# A sharp local deletion-capacity bound for every transformation

The original counting lemma assumed permutation or idempotent commands.
That restriction can be removed. The relevant quantity for a command f is
the rank of its (k+1)-st power, not necessarily the rank of f itself.
Priority of this independently derived statement remains unverified.

## Exact one-step theorem

Let f:Q->Q be any transformation of an n-element set, and let
S_0 subset ... subset S_k be nested uncertainty sets. Write

    S_j' = f(S_j) union S_(j-1),     S_-1 = empty,
    Phi = sum_j |S_j|.

Then

    Phi - Phi' <= n - rank(f^(k+1)).

Moreover, for every f and k this bound is attained by some nested chain.
Thus it is the exact maximum possible one-step decrease, not just an upper
estimate. Attaining chains need not be reachable in a particular automaton.

## Proof of the upper bound

Work over the rational numbers. Let V have basis vectors e_q for q in Q,
and let F:V->V be the linear map F(e_q)=e_(f(q)). Distinct output states
are distinct basis vectors, so rank(F^j)=rank(f^j).

On V^(k+1), define the block linear map

    T(v_0,...,v_k) = (Fv_0, Fv_1+v_0, ..., Fv_k+v_(k-1)).

Its kernel is in bijection with ker(F^(k+1)). Indeed, a kernel vector obeys

    v_(j-1) = -Fv_j,
    v_j = (-1)^(k-j) F^(k-j) v_k,
    F^(k+1) v_k = 0.

Every v_k satisfying the last equation determines exactly one such vector.
Consequently,

    dim ker(T) = n - rank(f^(k+1)).

Let U be the coordinate subspace spanned by e_q in layer j for q in S_j.
Its dimension is Phi. The image T(U) is supported inside the coordinate
subspace associated with S_j' in each layer, whose dimension is Phi'.
Applying rank-nullity to T restricted to U gives

    Phi' >= dim T(U)
         = dim U - dim(U intersect ker T)
         >= Phi - (n-rank(f^(k+1))).

This proof actually establishes the upper bound without requiring nesting.

## Proof of sharpness

Take the nested chain

    S_j = f^(k-j)(Q),     j=0,...,k.

Images of successive powers are nested. Here
S_j' = f^(k-j+1)(Q): for j>0 the deletion term S_(j-1) equals f(S_j),
and j=0 has no deletion term. The potential decrease telescopes:

    sum_(h=0)^k (rank(f^h)-rank(f^(h+1)))
        = n-rank(f^(k+1)).

This proves equality for every transformation and every nonnegative k.

## Consequence for arbitrary deterministic automata

For a word w whose commands act on n states, common-target resilience to
k deletions requires

    sum over letter occurrences x in w of (n-rank(x^(k+1)))
        >= (k+1)(n-1).

Initially every uncertainty layer is Q; finally every layer is the same
singleton. Sum the one-step bound over the word. Temporary increases in
uncertainty cannot invalidate this telescoping argument.

For a permutation, rank(x^(k+1))=n, so its contribution is zero. For an
idempotent, rank(x^(k+1))=rank(x), recovering the earlier lemma exactly.
For a general command, its full rank profile enters the bound. As k grows,
the profile eventually stabilizes at the number of states on cycles of x.

The power is necessary. Let n=3 and f=(0,0,1). With k=1, take S_0={0,1}
and S_1=Q. The potential drops from 5 to 3, a decrease of 2. The rank
deficiency of f alone is only 1, while n-rank(f^2)=2. This is an explicit
counterexample to extending the old formula unchanged to arbitrary letters.

The resulting whole-word inequality is necessary, not sufficient. Its
coefficients are individually sharp; this does not assert that an arbitrary
automaton can arrange to saturate them at every step.

## A permutation can replace the skipped command

There is also an exact extension to a specified permutation-valued fault.
Suppose correct execution applies f, while a fault applies a permutation g.
The update is S_j'=f(S_j) union g(S_(j-1)). Put h=g^-1 composed with f.
Applying g^-1 to each successor set preserves cardinalities and transforms
the update to h(S_j) union S_(j-1). Therefore its exact local capacity is

    n-rank((g^-1 composed with f)^(k+1)).

The sharp chain is S_j=h^(k-j)(Q). The corresponding sum bound over a word
holds when each command has its own specified permutation fault outcome.
Ordinary deletion is the case g=identity. This does not cover arbitrary
noninvertible fault outcomes or several alternate outcomes per command.

## Sharp whole-word examples

The supplied audit suggested the following useful example. For 1<=i<n,
let e_i send i to i-1 and fix every other state. Each e_i is idempotent of
rank n-1. The word

    e_(n-1)^(k+1) e_(n-2)^(k+1) ... e_1^(k+1)

survives any k deletions: every block retains an occurrence, and idempotence
makes the surviving block equal to a single e_i. Their descending product
maps all states to 0. Its length, and total deficiency cost, are exactly
(k+1)(n-1). Hence the whole-word bound is also sharp across automata.

## Verification scope

`rank_capacity.rad` exhausts all transformations on two, three, and four
states, for budgets zero through three, and all nested uncertainty chains.
It checks the upper bound, the exact maximum, and the image-power witness.
An independent Python implementation checks the resulting counts. These
finite checks support the linear-algebra proof; they do not replace it.
