# Resetting a machine when commands can disappear

Status: exact all-size length and weighted-cost formulas proved, and the
counting lemma extended to arbitrary transformations with a sharp local
coefficient. Start with [OPTIMALITY.md](OPTIMALITY.md) and
[GENERAL_BOUND.md](GENERAL_BOUND.md) for these advances. The original
construction and obstruction proofs remain below. This is not a claim to
settle the Cerny conjecture or establish historical priority.

## Problem

Let `C_n` have states `0,...,n-1`, with `a(q)=(q+1) mod n` and
`b(n-1)=0`, while `b(q)=q` otherwise. A word is **k-deletion-resilient to q**
if every word obtained by deleting at most k positions maps **every** initial
state to the **same fixed target q**. The common target across different
deletion patterns is essential; merely synchronizing each corrupted word is
a different problem.

Indeed, under that weaker requirement arbitrary finite deletion budgets are
always tolerable: repeat any ordinary reset word k+1 times. At most k deleted
positions leave at least one copy intact, and that intact factor synchronizes
the corrupted word. Its eventual target can depend on the deletion pattern.
The common-target requirement is what makes the sharp obstruction below
possible and relevant to recovering a *known* machine state.

## Exact finite results

RAD's full breadth-first search and an independent Python frozenset search
agree on every result, including shortest witness, target, and graph counts:

| States n | No-loss minimum | One-loss minimum | Two-loss reachable states, all rejected |
| --- | --- | --- | --- |
| 3 | 4 | 9 | 17 |
| 4 | 9 | 16 | 58 |
| 5 | 16 | 25 | 194 |
| 6 | 25 | 36 | 636 |
| 7 | 36 | 49 | 2,051 |
| 8 | 49 | 64 | 6,529 |
| 9 | 64 | 81 | 20,573 |
| 10 | 81 | 100 | 64,302 |

The two-state minima for zero, one, and two deletions are 1, 2, and 3.
The two-loss searches exhaust the reachable graph; they do not stop at a
chosen word-length cutoff. The proof below explains their failure for all n.

Both one-worker and four-worker runs passed independent verification. Each
recording replayed under the other worker count with an identical world
digest. The causal audit also recorded and replayed successfully: at n=6 its
three evidence lanes contain 312 expanded one-loss search states, 222 literal
trajectories, and 636 exhaustively rejected two-loss states. Reversing the
proposal order preserves its verdict. The complete acceptance run also passed
with Python assertions disabled (`python -O`).

## Theorem: one deletion costs exactly n^2; two are impossible

For every `n >= 3`, `C_n` has a one-deletion-resilient reset word of length
`n^2`, but no two-deletion-resilient reset word of any length. For `n=2`,
`b^(k+1)` is resilient to k deletions, and its length is optimal.

The length `n^2` is optimal for every n>=3. The new backward-interval proof in
[OPTIMALITY.md](OPTIMALITY.md) establishes at least `(n-1)^2+1` rotations,
independently of the number of merging commands. Combined with the bound
below, this rules out every shorter word, including command-count trade-offs.

The construction does minimize the number of b commands, for every n >= 3:
exactly `2n-2` are necessary and sufficient, irrespective of how many a
commands are allowed. A proof follows the construction below.

### Construction

Use the word

    W_n = b a^(n-1) (b a b a^(n-2))^(n-2) a b.

Its length is `n + (n-2)(n+1) + 2 = n^2`.

For a prefix w, write `S_0(w)` for its image with no deletion and `S_1(w)`
for the union of images with at most one deletion. Appending a letter x gives

    S_0(wx) = x(S_0(w))
    S_1(wx) = x(S_1(w)) union S_0(w).

Set `P_m={0,...,m}`, with `P_-1` empty. After `b a^(n-1)`, the pair is

    S_0 = P_(n-3) union {n-1}
    S_1 = P_(n-3) union {n-2,n-1} = Q.

For `0 <= m <= n-3`, the block `B=b a b a^(n-2)` sends the pair

    (P_m union {n-1}, P_m union {n-2,n-1})

to

    (P_(m-1) union {n-1}, P_(m-1) union {n-2,n-1}).

Here is the complete block check. With no deletion, B sends each
`q <= n-3` to `q-1 mod n` and fixes `n-2,n-1`. Thus B applied to the
old S_1 already produces exactly the new S_1. Deletions inside B, applied
to the old S_0, add no other states:

* deleting the first b sends n-1 to n-2 and each q in P_m to q-1;
* deleting the middle a gives `b a^(n-2)`, sending P_m to P_m-2 modulo n
  and n-1 to n-2;
* deleting the second b gives `b a^(n-1)`, with image contained in the
  new S_0;
* deleting any a in the final run rotates the no-deletion image back one
  position, again inside the new S_1.

In the second bullet, `P_m-2` means `{q-2 mod n : q in P_m}`, not `P_(m-2)`.
Consequently, after n-2 blocks,
the pair is `({n-1},{n-2,n-1})`. Appending a gives `({0},{0,n-1})`, and
appending b gives `({0},{0})`. This proves the construction for every n.

### Optimal number of merging commands

The nested sets `S_0 subset S_1` start with cardinalities `(n,n)` and end at
`(1,1)`. An a command cannot reduce either cardinality: a is a permutation,
and the update of S_1 includes the whole image a(S_1).

A b command can reduce S_0 by at most one. A decrease requires both 0 and
n-1 to belong to S_0. In that case both also belong to S_1, and the update
`S_1'=b(S_1) union S_0` restores the removed state n-1; S_1 does not decrease.
Conversely, any reduction of S_1 by b is at most one. Therefore a single b
reduces `|S_0|+|S_1|` by at most one. Reducing this sum from 2n to 2 requires
at least `2n-2` b commands. W_n has precisely that many.

This counting proof remains valid when a or b temporarily increases the
uncertainty; such increases can only require additional decreases later.

### General lower bound for idempotent reset commands

The count extends beyond this automaton. Let an n-state automaton have only
permutation letters and idempotent letters (`e(e(q))=e(q)`). Every word that
resets to a common target despite k deletions satisfies

    sum over idempotent letter occurrences e of (n - rank(e))
        >= (k+1)(n-1).

To prove it, use the nested uncertainty sets `S_0 subset ... subset S_k`
and potential `Phi=sum_j |S_j|`. On a letter x,

    S_j' = x(S_j) union S_(j-1),     S_-1 = empty.

A permutation cannot decrease any |S_j|. For an idempotent e, fixed points
cannot disappear. If a nonfixed state q disappears from S_j, then
`q in S_j \ S_(j-1)`, since S_(j-1) is included in S_j'. The layers are nested,
so any given q belongs to at most one such difference. Thus at most one copy
of each nonfixed q can disappear from the entire potential. Idempotence means
the fixed points are exactly the image, so there are `n-rank(e)` nonfixed
states. Hence an e occurrence decreases Phi by at most its rank deficiency.

Initially Phi=(k+1)n and finally Phi=k+1, proving the inequality. This recovers
the sharp `2n-2` b count above and the sharp k+1 bound for n=2.

The stronger theorem in [GENERAL_BOUND.md](GENERAL_BOUND.md) removes the
restriction on letters: each occurrence x contributes
`n-rank(x^(k+1))`. This is the exact maximum one-step potential decrease for
every transformation x. The proof uses rank-nullity of a block linear map,
and a nested image-power chain attains the bound. The supplied audit's
rank-(n-1) idempotent chain example also shows whole-word sharpness across
automata; its construction is included there with attribution.

### Impossibility

First, no one-deletion-resilient word can reset to n-1. If it ends in b,
its ordinary image cannot contain n-1. If it ends in a, deleting that final
a would require a to fix n-1, which it does not. The empty word is not a
reset word.

Now suppose a two-deletion-resilient reset word w exists. It cannot consist
only of b's since b has rank n-1 > 1. Write `w=u a b^t`, using the last a;
`t >= 1`, since the last letter of a deletion-resilient reset must fix its
target and a has no fixed points. Write q for the common target.

Let `D_1(u)` be the union of images of u with at most one deletion. Keep
the last a, or delete it, in addition to that possible deletion in u. Since
`b^t=b`, both cases require

    D_1(u) subset b^-1({q}) intersect a^-1(b^-1({q})).

For q=n-1 the first preimage is empty. For `1 <= q <= n-2`, the intersection
is `{q} intersect {q-1}`, also empty. For q=0 it is

    {0,n-1} intersect {n-2,n-1} = {n-1}.

Thus u would be a one-deletion-resilient reset to n-1, contradicting the
first paragraph. This excludes every finite word, without a search bound.

## General obstruction for larger reset arcs

Replace b by `b_r`, which sends `T={n-r,...,n-1}` to 0 and fixes other
states, for `1 <= r <= n-2`. No word is resilient to r+1 deletions with a
common reset target. This statement is an upper bound on tolerable faults;
existence for exactly r faults is not claimed in this generality.

Lemma: no word sends Q into an interval I contained in T under at most
`|I|` deletions. Induct on |I|. The empty interval is impossible because
images are nonempty. A word ending in b_r cannot have its image in I, since
`image(b_r)` is disjoint from T. For a word `v a`, keeping or deleting its
last letter implies that v, with at most `|I|-1` deletions, sends Q into
`I intersect a^-1(I)`, an interval of size `|I|-1` contained in T. Apply
induction. The empty word cannot send Q into a proper interval.

For a hypothetical (r+1)-resilient reset, repeat the last-a argument above.
Any nonzero target has empty intersected preimage (or is outside image b_r).
The zero target has intersected preimage exactly T. Its prefix u would send
Q into T under at most r deletions, contrary to the lemma.

## Relation to prior work and novelty

The algebraic induction steps, preimage identities, backward interval rules,
and both rotation-count cases are checked in `symbolic_verify.py`: twenty-one
quantifier-free integer counterexample queries are UNSAT with n left arbitrary.
The counting cases involve products; the interval cases are linear. Two
deliberately stronger false claims are SAT, verifying rejection of bad lemmas.
The runner saves SMT-LIB queries, Z3 proof objects, and hashes. The surrounding
induction is the written proof in this file and OPTIMALITY.md; this is not an end-to-end Lean/Coq proof,
and the Z3 proof objects have not been checked by a second proof checker.

The Cerny automata and their ordinary reset threshold `(n-1)^2` are classical.
The existing shortest-word literature provides the power-automaton/BFS
background: [Szykula and Zyzik, ESA 2022](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ESA.2022.85).
[Synchronization and Diversity of Solutions, AAAI 2023](https://ojs.aaai.org/index.php/AAAI/article/view/26361)
studies diversity of synchronizing words and extends its treatment to
conformant planning. That connection is relevant to unknown initial states
and unobserved outcomes, beyond the paper's edit-distance objective.

[Imreh and Steinby, 1999](https://cyber.bibl.u-szeged.hu/index.php/actcybern/article/view/3514/3499)
define D1-directing words by convergence of all possible executions to one
singleton. Universal common-target synchronization is therefore established
background. Our bounded counter starts only in layer zero and need not itself
end at a common counter value, so it is not identical to synchronizing all
states of an unrestricted nondeterministic automaton.

[Jensen, Veloso, and Bryant, 2004](https://www.cs.cmu.edu/~mmv/papers/04icaps-rune.pdf)
and [Domshlak, 2013](https://cdn.aaai.org/ojs/13546/13546-40-17064-1-2-20201228.pdf)
study bounded-fault planning. At the modeling level, our state is (s,d):
command x either advances to (x(s),d), or stays at (s,d+1) when d<k.
The initial set is Q times {0}; the target is {q} times {0,...,k}. We require
one fixed word, without observing which outcome occurred. These framework
connections are prior art, not contribution claims of this experiment.

[Eppstein, 1990](https://www.ics.uci.edu/~eppstein/pubs/Epp-SJC-90.pdf)
uses preserved cyclic order for efficient ordinary reset search. Our interval
encoding adapts that established geometric approach to the coupled backward
deletion sets. Parallel execution and subset search are not novelty claims.

[Kharchenko et al., IDAACS 2021](https://doi.org/10.1109/IDAACS53288.2021.9660925),
*The Fault Tolerant Cerny Finite State Machine: a Concept and VHDL Models*,
also studies an eight-state Cerny machine and preservation of reset paths
under faults. Only its abstract was accessible during this investigation;
its precise fault model and any overlap with these results remain unchecked.

A targeted literature search did not locate this exact theorem. That is not
evidence of priority: these results should be treated as independently derived
and awaiting a fuller literature review, not a certified new discovery.
The general power-rank capacity theorem needs its own priority review; a
search confined to the Cerny family would not settle that question.
