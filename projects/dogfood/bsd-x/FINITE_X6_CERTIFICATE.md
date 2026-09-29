# A conditional finite certificate for the leading-term comparison

This is a proved implication from explicit additional hypotheses, not a proof
of the universal X6 lemma. No elliptic-curve instance is certified here.
RAD checks the finite arithmetic and records WHY provenance; the mathematical
proof for arbitrary integers and torsion groups is given below.

## Certificate theorem

Fix an elliptic curve E/Q with algebraic rank r equal to analytic rank m.
Let A be its normalized analytic leading coefficient, with exactly the
period, regulator, torsion and Tamagawa conventions in the proposed BSD proof.
Suppose the following are independently established:

1. A = a^2 for a positive integer a. Approximate numerical agreement is insufficient.
2. A finite real bound B satisfies, for every integer n >= 2,

       #Sel_n(E/Q) / (n^r #E(Q)[n]) <= B.

3. For every prime p dividing A, some finite level k_p >= 1 satisfies

       v_p(#Sel_(p^k_p)) - k_p r - v_p(#E(Q)[p^k_p]) >= v_p(A).

Then if B < 4A, the Tate-Shafarevich group is finite and has order A.
Consequently every equality required by X6 follows.

**Proof.** Kummer exactness identifies the quotient in (2) with #Sha[n].
Sha is torsion. If it had more than B elements, choose an integer N > B
distinct elements and a common multiple n >= 2 of their orders. Then
#Sha[n] >= N, a contradiction. Thus Sha is finite. Taking n divisible by
all element orders gives q := #Sha <= B.

Condition (3) implies v_p(q) >= v_p(A) for every p dividing A, so A divides q.
For an elliptic curve, Cassels-Tate duality supplies a nondegenerate alternating
pairing on finite Sha; hence q is a square. This includes the 2-primary part.
Both q and A are integer squares, and A divides q. Prime factorization
therefore gives q = A j^2 for an integer j >= 1. Since q <= B < 4A,
j < 2 and j = 1. Thus q = A. Finally Sha[p^k] stabilizes at its full
p-primary subgroup, giving the X6 limit v_p(A) for every prime p. QED.

The square-order input is specific to elliptic curves here. See
[Poonen-Stoll, The Cassels-Tate pairing on polarized abelian varieties](https://math.mit.edu/~poonen/papers/sha.pdf),
abstract and introduction; the corresponding claim needs care for general
principally polarized abelian varieties.

## Exact local information increases the permissible global bound

Suppose also that for each p in a finite set S one has proved
v_p(q) = v_p(A). Let p_* be the smallest prime outside S. Then the
certificate theorem remains valid with B < A p_*^2 in place of B < 4A.

**Proof.** The preceding argument gives q = A j^2. Each exact comparison
forces p not to divide j. If j > 1, a prime divisor of j lies outside S,
so j >= p_* and q >= A p_*^2. The strict bound excludes this. QED.

Exact primary information can itself be certified at finite levels after
the global finiteness conclusion and rank equality have been established.
Write the finite p-primary group as a sum of paired cyclic groups of orders
p^(a_i). For C_p(k) = #Sel_(p^k)/#E(Q)[p^k],

    C_p(k+1)/C_p(k) = p^[r + 2 #{i : a_i > k}].

Thus C_p(k+1) <= p^(r+1) C_p(k) forces every a_i <= k. If the residual
valuation at that level equals v_p(A), the entire primary order equals it.
A valuation observed at just one level, without saturation, is insufficient.
The paired-group argument applies at p=2 as well under these hypotheses.

## What WHY diagnoses

The application varies a target order, a known divisor, a global bound and
exact local prime data in isolated forks. Its resolver recomputes the candidate
set before accepting any proposed equality. WHY and why_field expose the
accepted arithmetic and provenance; they do not supply missing analytic input.

For a divisor D, the least square multiple is

    M(D) = product_p p^[2 ceil(v_p(D)/2)].

Every square multiple of D is uniquely M(D) j^2: subtract the even exponent
of M(D) from the even exponent of the square. The differences are even and
nonnegative, proving the assertion by prime factorization. This is the
all-integer justification behind the finite enumeration in RAD.

Illustrative arithmetic witnesses, not elliptic-curve data:

| Target A | Known divisor D | Bound B | Exact primes S | Conclusion |
|---|---|---|---|---|
| 36 | 36 | 143 | empty | Only order 36 |
| 36 | 36 | 144 | empty | Orders 36 and 144 both possible |
| 36 | 4 | 143 | empty | Five square orders possible |
| 12 | 12 | 143 | empty | Only order 36; target 12 is not proved |
| 36 | 36 | 899 | 2, 3 | Only order 36 |
| 36 | 36 | 900 | 2, 3 | Orders 36 and 900 both possible |

The threshold is sharp given only these arithmetic hypotheses: q=A p_*^2
is itself a square divisible by A and has the same valuations at primes in S.
This does not claim that each numerical witness occurs for an elliptic curve.

## Status of the X's and the remaining arithmetic input

This theorem can replace the all-prime comparison step **when its hypotheses
are proved**. It does not establish those hypotheses for every E/Q. In
particular, positivity and rationality from X5 do not imply that A is an
integer square. The global bound is a quantitative uniform Selmer bound,
stronger than merely knowing there exists an unspecified finite bound.
It is not supplied by finitely many local computations or by WHY.

For the revised four-placeholder argument, X1 asks for independent rational
points, X3 for a Selmer-growth certificate, X5 for higher-rank rationality and
positivity, and X6 for all-prime comparison without assuming Sha finite.
None of these universal statements has been proved by this application.
The earlier README's exterior-Selmer X1 and separate X4 refer to the original
five-placeholder draft and must not be silently substituted for the revision.

Run from the repository root:

    python projects/dogfood/bsd-x/verify_leading_term_certificate.py

The runner checks 400 forks and six witnesses by independent enumeration of
square integers, compares one and four workers, verifies recorded replay and
rejects a forged equality. See leading_term_verification.json and
leading_term_certificate.why.txt. These are execution checks, not foundational
formalization of Kummer exactness, duality or the conditional theorem above.
