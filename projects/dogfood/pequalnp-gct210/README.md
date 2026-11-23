# pequalnp L-gct-210: matrix spaces with constant per(J + t)

The pequalnp desk (https://pequalnp.com) posted this gap to `peteracs` on 2026-11-24:

> ∀ L : Submodule ℂ (Fin 5 × Fin 5 → ℂ), (∀ t ∈ L, ∑ f : Fin 5 ↪ Fin 5, ∏ i, (1 + t (i, f i)) = 120) → finrank ℂ L ≤ 8

The sum is per(J + t), J the all-ones matrix. The largest dimension known is 7; a space
of dimension 9 refutes the gap, and the desk wants either answer. This workload is not
a proof of either. It finds or rules out spaces in exact families, with `explore`.

| Program | What it does | Run time |
|---|---|---|
| `orbit_fourier.rad -- <row orbits> <col orbits>` | Every valid space in one orbit-Fourier family of p x q matrices (p, q = the orbit sums), e.g. `-- 4,1 4,1` | < 0.5 s each |
| `orbit_fourier.rad -- <rows> <cols> stdin <d> < classes.txt` | Is there a valid space of dimension d, imposing only the condition classes read from stdin | < 0.5 s each |
| `rad blame <trace> --output "dimension <d>: none"` | Which condition classes the cap depends on (see below) | < 5 s |

Every run is bounded by `timeout 5`.

## Reductions

**Homogeneous parts.** per(J + t) = Σ_k (5 − k)! P_k(t), where P_k is the sum of all
k × k subpermanents. L is a linear space, so per(J + λt) is constant in λ and every
P_k (k = 1..5) vanishes on L. P_1 is the sum of the entries, so no coordinate subspace
works.

**Rank-one lemma.** Let L be spanned by rank-one matrices u_v w_vᵀ (v = 1..d) and put
f_U(y) = ∏_a (1 + Σ_v u_v(a) y_v), f_W(y) = ∏_b (1 + Σ_v w_v(b) y_v). For a multiset m
of generators of size s,

    [x^m] per(J + Σ_v x_v u_v w_vᵀ) = (5 − s)! · ∏_v m_v! · [y^m] f_U · [y^m] f_W.

Proof: expand each row of J + t as 1 + Σ_v x_v u_v(a) w_v(σa). A term picks, for each
row a, either 1 or one generator o_a; its column part is Σ_σ ∏_{a picked} w_{o_a}(σa).
For a fixed choice o the sum over σ is (5 − s)! times the sum over injective placements
of the picked rows into columns, which counts each label arrangement of m ∏ m_v! times;
it does not depend on o. Summing the row parts over o gives [y^m] f_U. ∎

ℂ has no zero divisors, so **L is valid exactly when, for every multiset m of size 1..5,
[y^m] f_U = 0 or [y^m] f_W = 0.**

## Orbit-Fourier families

`orbit_fourier.rad` cuts the rows into consecutive orbits (a partition of 5) and the
columns likewise. A generator is a character of Z_|O| on one row orbit O (zero on the
other rows) times a character of Z_|P| on one column orbit P: 25 generators, linearly
independent. Each u_v lives on one row orbit, so f_U is a product over orbits and
[y^m] f_U is zero exactly when one orbit's part of m gives zero. That part is a sum,
over injective placements of its character values into the orbit's rows, of
ζ^(Σ a·row): an integer combination of roots of unity, decided by counting exponents
(all counts equal for prime orbit size; N₀ = N₂ and N₁ = N₃ for size 4). Validity
passes to subsets and is invariant under scaling each orbit's characters by a unit, so
`explore` grows canonical sets one generator at a time and "exhausted" means the whole
family was reached. One orbit per side, with the other generators unused, is the
r × s block case.

With one orbit of size 5 on both sides no generator is valid at all: e_v is rank one
with no zero entry, so [x_v^5] = 120 · ∏ entries ≠ 0. Smaller orbits avoid that,
because a part larger than its orbit has no injective placement and gives zero.

## Results

All 28 pairs of partitions (up to transposition) end `exhausted`, each well under the
5 s bound. The largest valid space in any of them has **dimension 6**, below the known
7, so these families neither refute the gap nor reach its record:

| rows \ cols | 5 | 4,1 | 3,2 | 3,1,1 | 2,2,1 | 2,1,1,1 | 1,1,1,1,1 |
|---|---|---|---|---|---|---|---|
| 5 | 0 | 4 | 4 | 4 | 4 | 4 | 4 |
| 4,1 | | **6** | **6** | 5 | 5 | 4 | 3 |
| 3,2 | | | 4 | 5 | 4 | 4 | 3 |
| 3,1,1 | | | | 4 | 4 | 3 | 2 |
| 2,2,1 | | | | | 4 | 3 | 2 |
| 2,1,1,1 | | | | | | 2 | 1 |
| 1,1,1,1,1 | | | | | | | 0 |

The (4,1) x (4,1) witness, checked by hand without the lemma: t = [[0, c], [r, 0]] with
c a column (rows 0..3, column 4) and r a row (row 4, columns 0..3), Σc = Σr = 0. A
permutation fixing 4 contributes 1 (24 of them); one sending row i to column 4 and
row 4 to column j contributes (1 + c_i)(1 + r_j) (6 of each). So
per(J + t) = 24 + 6 (4 + Σr)(4 + Σc) = 120, and dim L = 3 + 3 = 6.

For one orbit per side (an r x s block, the rest unused), the maximum is 4: the single
row (or column) of zero sum.

## 4 x 5 (gap L-gct-179: find dimension 7)

For p x q the per-type sum runs over injections Fin p ↪ Fin q; the lemma holds with the
constant (q − s)!/(q − p)! in place of (5 − s)!, and a monomial has degree at most
min(p, q). All 35 pairs (5 row partitions of 4, 7 column partitions of 5) end
`exhausted`. The largest valid space is **5**, at rows (3,1) with columns (3,2) or (4,1):
below the record 6 (one row plus one column), so a 7 needs generators whose row and
column orbits differ from generator to generator, as in the accepted L-gct-198 witness.

## Theory (no search)

**Quadratic bound, any p x q.** On C^(p x q) write t by rows r_1..r_p. Then
P_2(t) = Σ_{i<i'} r_iᵀ (J_q − I_q) r_{i'}, the quadratic form of ½ (J_p − I_p) ⊗ (J_q − I_q),
whose eigenvalues are products of {p − 1, −1} and {q − 1, −1}: none is zero, so P_2 is
nondegenerate. On the hyperplane H = {P_1 = 0} = 1^⊥ a radical vector would be a
multiple of (A ⊗ M)^(−1) 1 ∝ 1, which is not in H, so P_2 stays nondegenerate on H
(dimension pq − 1). L is totally isotropic for it, hence **dim L ≤ ⌊(pq − 1)/2⌋**. For
p = 2 this is q − 1, attained by one zero-sum row: **two-row level spaces have dimension
exactly q − 1 = p + q − 3.**

**4 x 5, row plus Z_3 characters (L-gct-179).** Take row 0 free with zero sum (4
dimensions) and add rank-one u_k ⊗ v_k with u_k ∈ {(1, ω, ω²), (1, ω², ω)} on rows 1..3.
By the lemma the row side vanishes except on a mixed pair {χ_1, χ_2} or a uniform triple.
A uniform triple forces e_3(v^(b)) = 0 for every coordinate b; since
e_3(v^(b)) = g(v_b) with g(z) = e_3 − e_2 z + e_1 z² − z³, all five coordinates are roots
of g, and matching the power sums p_1..p_3 leaves at most two nonzero coordinates.
So each character's span lies in a coordinate plane. A mixed pair forces
xᵀ(J − I)y = 0 and x_c Σy + y_c Σx = 2 x_c y_c for every column c, which leaves the
two spans at total dimension ≤ 2. **This route caps at 6, the record**; a 7 must use
other row vectors (the trivial character, or rows shared with row 0).

**Row-split spaces (Theorem A).** Let L = ⊕_i e_i ⊗ W_i (each member's row i lies in
W_i ⊂ C^q). P_k is multilinear across rows, so L is level exactly when for every set of
k distinct rows and w_i ∈ W_i the rectangular permanent of the q x k matrix [w_i] is 0.
With zero sums that is: W_i ⊂ Z = 1^⊥; W_i ⊥ W_j (dot product) for i ≠ j; then
Σ_a w_a w'_a w''_a = 0 over three distinct rows (inclusion-exclusion leaves only the
3-block, coefficient 2) and Σ_a w_a w'_a w''_a w'''_a = 0 over four (coefficient −6).

*4 x 5 row-split level spaces have dimension ≤ 6.* Z ⊂ C^5 has dimension 4 and the dot
product on it is nondegenerate, so W_j ⊂ W_i^⊥ gives dim W_i + dim W_j ≤ 4. A sum of 7
over four rows must be (2, 2, 2, 1), and then W_1 = W_0^⊥ and W_2 ⊂ W_0 ∩ W_1 force
W_0 = W_1 = W_2 = I with I = I^⊥. The triple condition on rows 0, 1, 2 makes I a plane
inside {p_1 = p_2 = p_3 = 0}, i.e. a line on the curve X = V(p_1, p_2, p_3) ⊂ P^4. X is
smooth: its Jacobian rows 1, w, w² are dependent only if w takes at most two values,
and two values with p_1 = p_2 = 0 force w = 0. A smooth complete intersection of
positive dimension is connected, so X is an irreducible curve of degree 6 (genus 4)
and contains no line. ∎ The same count caps row-split spaces at 7 on 5 x 5 (a sum of 8
again needs three rows sharing such an I, and (3,1,1,1,1) dies because the line ℓ =
W_0^⊥ would need ℓ² ∈ span(ℓ, 1) with p_1(ℓ) = p_2(ℓ) = 0, forcing ℓ = 0) and at 4 on 4 x 4.
Row-split spaces never beat the records p + q − 3.

## Counterfactual obstruction cores (`rad blame`)

A search that ends `exhausted` says *that* a dimension is out of reach, not *why*.
RAD's `rad blame` answers why for any recorded program: it replays the session with
recorded inputs withheld and reports the inputs the output depends on. Here the
inputs are the conditions themselves. With the argument `stdin` the program reads
the condition classes to impose, one per line (`k d`: the monomials of size k whose
generators touch d distinct row orbits, `classes.txt` lists all ten for degree ≤ 4), so

    rad orbit_fourier.rad --record t.radr -- 1,1,1,1 5 stdin 5 < classes.txt
    rad blame t.radr --output "dimension 5: none"

replays the search under relaxed hypotheses and returns a **minimal set of condition
classes that still rules the dimension out**, with each class marked necessary or
incidental. Withholding a class is a relaxation, so a necessary class is one whose
removal lets a bigger space through, and the minimal cause is an obstruction core.

| Family | Cap | Necessary classes | Incidental | Replays |
|---|---|---|---|---|
| rows 1,1,1,1 x cols 5 (row-split) | 4 | (1,1) (2,2) (3,3) (4,4) | the 7 classes with a repeated row | 24 in 4.6 s |
| rows 3,1 x cols 4,1 (best 4 x 5) | 5 | (1,1) (2,1) (3,1) (4,2), plus (2,2) or a stand-in | the other 6 | 24 in 2.6 s |

The row-split core is exactly the reduction in Theorem A, found by replay rather than
by hand: only conditions across distinct rows matter. In the best 4 x 5 family the
obstruction sits inside the 3-row orbit (the ℤ_3 characters, as in the hand analysis
above) plus the cross-orbit quadruples. Dropping any single nonlinear class from that
family lifts the cap to 6 but never to 7 (each relaxed search for 7 ends `exhausted`);
only dropping P_1 itself does. A 7 there is at least two condition classes away.

## RAD findings

- GH 0018 (fixed): an unannotated recursive helper was inferred IO, so `explore`
  refused the move system. Found by the first version of this program; the helpers
  here (`place`, `add_multisets`, `all_vanish`) are plain recursive `fn`s again.
- GH 0019 (open): the one-line form `fn f() -> bool { if c { return a } return g() }`
  is still not inferred pure.
