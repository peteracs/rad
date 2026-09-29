# Exact length and command costs under one deletion

This closes the all-size optimality question left open in the first experiment.
The argument below is a mathematical proof. The backward RAD search and the
symbolic checks are supporting audits, not the source of its quantifiers.
Historical priority is not established.

## Theorem

For every n >= 3, every common-target one-deletion-resilient word in C_n has

    number of a's >= (n-1)^2 + 1,
    number of b's >= 2n-2.

The word from RESULTS.md,

    W_n = b a^(n-1) (b a b a^(n-2))^(n-2) a b,

attains both bounds simultaneously. Consequently its length n^2 is optimal
for every n >= 3, not just the sizes searched by the program.

More generally, if an a costs alpha and a b costs beta, for any nonnegative
alpha, beta the minimum cost is exactly

    alpha ((n-1)^2 + 1) + beta (2n-2).

Thus extra merging commands cannot buy fewer rotations, or conversely.

## Only zero can be a resilient target

A resilient word consisting only of b's has rank n-1 > 1. Otherwise write
it as u a b^t, using its last a. The final letter must fix the target, since
deleting it leaves the same target; therefore t >= 1. Keeping or deleting
that last a shows that the ordinary image of u is contained in

    b^-1({q}) intersect a^-1(b^-1({q})).

This intersection is empty unless q=0, when it is {n-1}. Hence q=0 is the
only possible target. This uses only one allowed deletion.

## Read the word backward

For a suffix v, let X(v) be the states sent to 0 by v with no deletion, and
Y(v) the states sent to 0 by every version of v with at most one deletion.
Then Y is contained in X. Initially, for the empty suffix,

    X = Y = {0}.

Prepending a letter x gives exactly

    X' = x^-1(X),
    Y' = x^-1(Y) intersect X.

The second term handles deleting the prepended letter. A resilient word is
reached when Y=Q. An empty Y can never become nonempty, so all the intermediate
Y's of a successful backward path are nonempty. Stop at the first Y=Q; any
remaining letters of the original word only add nonnegative command counts.

## The interval rule

Before that final step, Y is a proper cyclic interval. Write it as

    Y = {L, L+1, ..., R} modulo n,

where L is its unique left endpoint. The following rules prove preservation
of this property inductively; they require only Y subset X.

* On a, the preimage shifts Y back one position. Every shifted point except
  possibly L-1 already belongs to Y and hence X. If L-1 belongs to X, Y'
  has the same length and left endpoint L-1. Otherwise Y' loses one point
  and has the unchanged left endpoint L. Call the latter a **stationary
  rotation**. It cannot occur when Y is a singleton on a successful path.
* On b, only membership of n-1 can change. A growth is possible only when
  0 belongs to Y, n-1 does not belong to Y, and n-1 belongs to X. Then
  L=0, the length increases by one, and the new left endpoint is n-1.
  A shrink can only remove the right endpoint n-1; the left endpoint stays
  fixed. All remaining cases leave Y unchanged.

In particular, b never moves a nonempty interval's left endpoint except
during growth. Each growth begins at left endpoint 0 and resets it to n-1.
Between any two consecutive growths there must therefore be at least n-1
nonstationary rotations. Stationary rotations do not contribute to that
necessary travel around the circle.

Let G be the total number of growths, H the number of stationary rotations,
F the number of rotations before the first growth, and A the total number
of a's. Since Y grows from size one to size n,

    G >= n-1,
    A >= F + (G-1)(n-1) + H.

No shrink is possible before the first growth, because Y is a singleton.
Its left endpoint starts at 0, and that first growth also requires endpoint
0. Hence F is a nonnegative multiple of n.

## The two cases

If F >= n, the preceding inequalities give

    A >= n + (n-2)(n-1) = (n-1)^2 + 1.

If F=0, the first growth occurs without any preceding a. Starting at
({0},{0}), the first b produces ({n-1,0},{0}), and the second produces

    X = Y = {n-1,0}.

Further b's leave this pair unchanged. Since n>=3, Y is not yet Q, so an a
must eventually occur. That a changes Y to {n-1}; it is a stationary
rotation and loses one point. Thus H>=1 and G>=n. It follows that

    A >= (n-1)(n-1) + 1 = (n-1)^2 + 1.

This exhausts all cases, including paths with temporary shrinkage, redundant
letters, or more than the minimum number of b's. Combining the rotation
bound with the independent 2n-2 merging-command bound in RESULTS.md proves
the theorem.

## Complete length-versus-fault classification

Let R_k(n) be the minimum resilient word length, with infinity denoting
nonexistence. The construction, this lower bound, and the prior obstruction
give the entire table:

| Family | Deletion budget | Exact minimum |
| --- | --- | --- |
| n=2 | any k>=0 | k+1 |
| n>=3 | k=0 | (n-1)^2, the classical ordinary threshold |
| n>=3 | k=1 | n^2 |
| n>=3 | k>=2 | infinity |

The exact price of protecting C_n against one deletion is therefore 2n-1
additional commands over its ordinary shortest reset. In the audit's
notation, rho(C_2)=infinity and rho(C_n)=1 for n>=3.

## Computational consequence

X is also a cyclic interval: rotations preserve intervals, and a b preimage
can only add n-1 immediately before 0 or remove the right endpoint n-1.
There are n(n-1)+1 nonempty cyclic intervals, including Q. Thus the backward
graph fits inside [n(n-1)+1]^2 pairs, a polynomial bound. It is unnecessary
to enumerate the exponentially many general forward uncertainty pairs.

`backward.rad` searches this graph with independent nonnegative letter costs.
It checks interval shape and nesting before encoding every successor and
relaxes all reachable distances, including zero-cost transitions. Its numeric
result is computed from graph paths, without consulting the theorem's formula.
`optimality.rad` evaluates the three objectives (total length, rotations only,
merges only) in isolated parallel worlds. `backward_oracle.py` uses a heap and
literal frozenset preimages, without interval pruning or RAD's buffer encoding.

The symbolic verifier checks the local interval rules and both arithmetic
cases with n left arbitrary. Its scope remains the encoded lemmas; the
induction, endpoint travel, and connection to words are proved in this note.
