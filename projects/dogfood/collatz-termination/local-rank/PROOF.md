# Exact obstructions to two Collatz ranking families

These results concern specified proof templates. They do not prove or
disprove the Collatz conjecture. No priority claim is made.

## Encoding and rewrite rules

Use the shortcut map on positive integers:

\[
T(n)=\begin{cases}n/2&n\text{ even},\\(3n+1)/2&n\text{ odd}.\end{cases}
\]

For a binary representation beginning with 1, replace that leading bit by
`c`, replace the remaining 0 and 1 digits by `a` and `b`, and append `d`.
More generally, valid mixed words have form `c D* d`, where
`D={a,b,e,f,g}`. Starting from value 1 at `c`, reading `a,b` applies
`x -> 2x, 2x+1`; reading `e,f,g` applies `x -> 3x, 3x+1, 3x+2`.
The end marker contributes no arithmetic operation.

The mixed-base rules used here are from
[Yolcu, Aaronson, and Heule](https://emreyolcu.com/research/rewriting-collatz.pdf):

| Index | Rule | Uses on the witness path |
| ---: | --- | ---: |
| 0 | `ad -> d` | 20 |
| 1 | `bd -> gd` | 34 |
| 2 | `ae -> ea` | 34 |
| 3 | `af -> eb` | 38 |
| 4 | `ag -> fa` | 52 |
| 5 | `be -> fb` | 24 |
| 6 | `bf -> ga` | 27 |
| 7 | `bg -> gb` | 67 |
| 8 | `ce -> cb` | 14 |
| 9 | `cf -> caa` | 11 |
| 10 | `cg -> cab` | 9 |

On a binary word other than `cd`, apply rule 0 or 1 according to its final
bit. In the odd case, move the resulting ternary digit left using rules
2 through 7 and eliminate it using rule 8, 9, or 10. The result is the
binary encoding of `T(n)`. The first two rules implement the shortcut map;
the remaining rules preserve the represented integer. The terminal word
`cd` represents 1 and has no rewrite.

## A literal orbit obstruction

For a word `w`, let `H_k(w)` contain the number of occurrences of every
contiguous factor of lengths 1 through `k`. Counts include boundary
markers and overlapping occurrences. Let `V(w)` be its represented integer.

**Theorem.** There is no real-valued rank of the form

\[
R(w)=F\bigl(H_4(w),V(w)\bmod24\bigr),
\]

with arbitrary function `F`, that weakly decreases under every valid
rewrite and strictly decreases under every application of any fixed one
of the eleven rules.

**Proof.** Direct shortcut iteration gives the following segment:

```text
674,337,506,253,380,190,95,143,215,323,485,728,364,182,91,
137,206,103,155,233,350,175,263,395,593,890,445,668,334,167,
251,377,566,283,425,638,319,479,719,1079,1619,2429,3644,
1822,911,1367,2051,3077,4616,2308,1154,577,866,433,650
```

There are 54 transitions, of which 34 are odd. No state equals 1.
The endpoint encodings are `cababaaabad` and `cabaaababad`.
Their common nonzero factor counts are:

| Length | Factors and multiplicities |
| ---: | --- |
| 1 | `a`:6, `b`:3, `c`:1, `d`:1 |
| 2 | `aa`:2, `ab`:3, `ad`:1, `ba`:3, `ca`:1 |
| 3 | `aaa`:1, `aab`:1, `aba`:3, `baa`:1, `bab`:1, `bad`:1, `cab`:1 |
| 4 | `aaab`, `aaba`, `abaa`, `abab`, `abad`, `baaa`, `baba`, `caba`: one each |

Both integers are 2 modulo 24. Thus their ranks are equal. Expanding the
segment by the canonical normalization above gives 330 rewrites, with the
positive rule counts in the first table. Weak decrease on each rewrite,
together with strict decrease on the occurrence of the selected rule,
would give a strict decrease between equal endpoint ranks. Contradiction.

No lower-bound condition on the rank was used. The proof also excludes
features obtained by discarding some of these counts or replacing modulus
24 by a divisor. The same argument applies to a rank that is required to
strictly decrease at every shortcut step. It does not exclude ranks that
permit increases, only decrease over differently selected blocks, retain
longer factors or digit order, or use other information. The endpoint
counts differ at length five. In particular, this is not an integer cycle.

[check_alias.rad](check_alias.rad) checks the orbit, odd count, endpoint
features, and residue. [literal.py](literal.py) separately reconstructs
every rewrite and checks binary normalization after every shortcut step.

## Linear factor ranks and finite exact certificates

The linear subclass is

\[
R(w)=\sum_{1\le |u|\le k}\lambda_u N_u(w),\qquad\lambda_u\in\mathbb R.
\]

All weights can be signed and are unrestricted in magnitude. For a
rewrite, factor occurrences wholly outside the replaced substring cancel.
Every affected factor reaches at most `k-1` symbols into either context.
It is therefore enough to enumerate all contexts of length at most `k-1`,
retaining actual boundary markers. If a context is longer, keep its nearest
`k-1` digits; any newly introduced distant boundary affects only factors
that cancel. This proves completeness of the finite context constraints.
Using shorter contexts alone does not prove an all-word assertion.

For selected strict rules, the generated inequalities have right-hand
side 1; other rules have right-hand side 0. Because the complete collection
is finite, any real weights producing positive gaps on all selected rows
can be scaled to satisfy the unit-gap version.

To search for a bounded-below rank with signed weights, the generator also
adds potential inequalities on the finite suffix graph. A state is a
digit word `s` of length `k-1`. Appending digit `a` contributes a cost
`g(s,a)` equal to the sum of the weights of all new suffix factors. With
`s'` the new suffix, impose

\[
g(s,a)+h(s)-h(s')\ge0.
\]

Summing telescopes, so interior costs are bounded below. Initial and
terminal factor contributions range over finite sets, giving a uniform
lower bound on all valid words. These inequalities are a sufficient
lower-bound certificate; they are not needed by any published obstruction
in this directory.

Write the resulting system as `A lambda >= b`. A certificate lists positive
integer multipliers `z_i` for specific rows, satisfying

\[
\sum_i z_i A_i=0,\qquad \sum_i z_i b_i>0.
\]

Adding those inequalities gives `0 >= c` for a positive integer `c`, so
the system is infeasible over the reals. The checkers reconstruct every
row from rule/context descriptors and check exact cancellation. They do
not depend on trusting the original solver's UNSAT response.

For the literal path, the 330 rewrites combine into 239 distinct contextual
rows, saved in [literal.csv](certificates/literal.csv). With strict rules
8, 9, and 10, the exact contradiction is **0 >= 34**. The general nonlinear
theorem above is stronger than this linear certificate.

The remaining factor certificates establish infeasibility for memory one
with all three left-boundary rules strict, and memories two and three
with that choice or any single boundary rule in `{0,1,8,9,10}` strict.
All these certificates use only rewrite rows. Their conclusions therefore
hold even if the rank's lower-bound requirement is dropped.

## Prefix-residue weighted ranks

This is a separate, order-sensitive finite-state model. Fix a modulus `m`.
Starting in residue `1 mod m`, reading a digit updates the residue by its
mixed-base arithmetic operation. Define

\[
R(c s_1\cdots s_\ell d)
=\sum_{i=1}^{\ell}w(r_{i-1},s_i)+t(r_\ell),
\]

where the edge weights `w` and terminal weights `t` are arbitrary reals.
This model remembers the prefix residue at every digit, rather than only
the final residue. It is not covered by the endpoint-histogram theorem.

For converter rules 2 through 10, the two sides end in the same arithmetic
residue, so all following costs cancel. For rules 0 and 1, the rewrite is
at the right boundary, so its constraint must include the terminal weight.
Every prefix residue is attainable by a positive integer's binary word;
thus checking all residues for rules 0 through 7 is justified. Rules 8
through 10 occur only at the leading marker and use residue `1 mod m`.
Graph potentials `w(r,s)+h(r)-h(r') >= 0` can again certify boundedness
below, but the saved obstructions do not use these extra rows.

For each `m` in `{2,3,5,7,15,31}`, the published certificate proves that no
such rank weakly decreases on every rule while decreasing by a positive
amount on all three rules 8, 9, and 10. Finitely many constraints again
justify normalization to a unit gap. No coefficient search bound is part
of this conclusion. No such conclusion was obtained for modulus 127, and
these modular certificates do not by themselves exclude every other
choice of strict rules.

## Verification scope

[accept.py](accept.py) rechecks the 20 certificates named by
[manifest.json](certificates/manifest.json) using RAD integer arithmetic
and independent Python arbitrary-precision arithmetic. RAD input limits
bound coefficient accumulation within signed 64-bit arithmetic. The
literal witness stays within the checked trajectory arithmetic limits.
The acceptance also checks positive controls, worker replay, and deliberate
certificate corruption; [evidence.json](evidence.json) records the run.

This combines an elementary mathematical argument with independently
checked finite identities. It is not an end-to-end proof in a theorem
prover, and it does not certify the unsearched space of termination
arguments. Floating-point suggestions and timed-out searches provide no
additional theorem.
