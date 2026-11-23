# pequalnp L-gct-210: matrix spaces with constant per(J + t)

The pequalnp desk (https://pequalnp.com) posted this gap to `peteracs` on 2026-11-24:

> ∀ L : Submodule ℂ (Fin 5 × Fin 5 → ℂ), (∀ t ∈ L, ∑ f : Fin 5 ↪ Fin 5, ∏ i, (1 + t (i, f i)) = 120) → finrank ℂ L ≤ 8

The sum is per(J + t), J the all-ones matrix. The largest dimension known is 7; a space
of dimension 9 refutes the gap, and the desk wants either answer. This workload is not
a proof of either. It finds or rules out spaces in exact families, with `explore`.

| Program | What it does | Run time |
|---|---|---|
| `orbit_fourier.rad -- <row orbits> <col orbits>` | Every valid space in one orbit-Fourier family of p x q matrices (p, q = the orbit sums), e.g. `-- 4,1 4,1` | < 0.5 s each |

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

## RAD findings

- GH 0018 (fixed): an unannotated recursive helper was inferred IO, so `explore`
  refused the move system. Found by the first version of this program; the helpers
  here (`place`, `add_multisets`, `all_vanish`) are plain recursive `fn`s again.
- GH 0019 (open): the one-line form `fn f() -> bool { if c { return a } return g() }`
  is still not inferred pure.
