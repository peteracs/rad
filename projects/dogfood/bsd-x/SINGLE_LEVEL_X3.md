# One-level certificate for the revised X3

This proves an exact reduction of X3, not its universal existence assertion.
There is no claim that RAD has proved BSD or computed an actual elliptic curve.
The proof below works without parity, Cassels-Tate square orders, or finiteness
of the full Tate-Shafarevich group.

## The certificate

Fix E/Q, a prime p, and a nonnegative integer m. Set

    C(k) = #Sel_(p^k)(E/Q) / #E(Q)[p^k],  C(0)=1.

If for some N >= 1,

    C(N) < p^[N(m+1)],

then for every k >= N-1,

    C(k+1) <= p^m C(k).

In particular, when m is the analytic rank and p>=5 is of good reduction,
this supplies the revised X3 certificate at k=N-1.

If additionally m independent rational points have been certified, then

    rank E(Q) = m,
    Sha[p^infinity] = Sha[p^(N-1)].

The second conclusion is a proof of the entire primary tail's saturation,
not merely a finite sample of later levels. At N=1 it means this primary
component is trivial.

## Proof

Let G=Sha(E/Q). Each G[p^j] is finite by Kummer theory and finiteness of
finite-level Selmer groups. For j>=0 put

    H_j = G[p^(j+1)] / G[p^j].

For j>=1, multiplication by p induces an injective map H_j -> H_(j-1).
It is well defined since p G[p^j] lies in G[p^(j-1)]. If the image of x is
zero, then px lies in G[p^(j-1)], so p^j x=0; hence x already belongs to
G[p^j] and represents zero in H_j. This proves injectivity.

Each H_j is a finite elementary abelian p-group. Thus the integers

    h_j = log_p #H_j

are nonnegative and nonincreasing. Write r=rank E(Q). The Kummer order
identity gives, with e_j=log_p C(j),

    e_0=0,
    g_j := e_(j+1)-e_j = r+h_j.

Therefore g_j is an integer, g_j>=r, and g_j is nonincreasing. Consequently

    N g_(N-1) <= sum_(j=0)^(N-1) g_j = e_N.

The certificate hypothesis e_N < N(m+1) implies g_(N-1)<m+1, so integrality
gives g_(N-1)<=m. Monotonicity gives g_k<=m for every k>=N-1, proving the
first assertion.

If m independent points exist, then r>=m. Hence

    m <= r <= g_(N-1) <= m.

Thus r=m and h_(N-1)=0. Monotonicity and nonnegativity force h_j=0 for
every j>=N-1. Every successive inclusion G[p^j] -> G[p^(j+1)] is then an
equality starting at j=N-1, so their union is G[p^(N-1)]. QED.

## Exact equivalence with the revised existential X3

For a fixed eligible p and m,

    exists k>=0: C(k+1)<=p^m C(k)

is equivalent to

    exists N>=1: C(N)<p^[N(m+1)].

The reverse implication was just proved. For the forward implication,
g_k<=m and monotonicity give, for N>=k,

    e_N <= e_k + (N-k)m.

Choose an integer N>=max(1,k) with N>e_k-km. Then e_N<N(m+1).
This proves the equivalence without an assumed decomposition of G.

**What is not proved:** for each actual E of analytic rank m>=2 there is
an eligible p and a level N meeting this strict inequality. This is exactly
the remaining existential content of X3, not an easier result claimed solved.
X1, X5, and X6 are not supplied by this calculation either.

## RAD scope and WHY

single_level_certificate.rad explores 720 isolated resource-bound forks of
abstract groups (Q_p/Z_p)^t plus finite cyclic p-groups. Their finite cyclic
factors need not be paired; some test states therefore cannot be elliptic-curve
Sha groups. A theorem valid for all these torsion groups applies to the actual
Sha groups as well, but a failing abstract state is not a counterexample to BSD.

The program derives the average-floor upper bound, checks its propagation,
and checks the additional rank and primary-closure conclusions. WHY and
why_field report provenance through a resolver that recomputes all evidence;
they do not discover the proof of injectivity or certify analytic rank.
The universal tail argument is the mathematical proof above, not the finite
loop in the program. The test includes finite and divisible tails that give
identical diagnostics when the strict certificate fails; neither is accepted
as a closure certificate.

Run:

    python projects/dogfood/bsd-x/verify_single_level.py

The runner independently counts filtration layers, checks one/four-worker
agreement, replays a recorded execution and rejects a forged certificate.
It writes single_level_verification.json and single_level_certificate.why.txt.

The broader arithmetic comparison remains distinct from structural Selmer
identities; see [Kim, The structure of Selmer groups and the Iwasawa main
conjecture for elliptic curves](https://arxiv.org/html/2203.12159v6), introduction
and section 1.9, for that distinction in the research literature. No additional
theorem from that paper is assumed in the elementary deduction above.
