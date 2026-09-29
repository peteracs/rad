# A sharp additive envelope at the first Collatz contraction

This investigation proves an explicit extremal formula and a uniform bound
with a sharp additive constant. It also provides executable finite-horizon
certificates. It does not prove Collatz or establish historical priority.

Use the shortcut map `T(n)=n/2` for even n and `(3n+1)/2` for odd n.
For a length-j parity word with q odd steps write

\[
T^j(n)=\frac{3^q n+C}{2^j}.
\]

Let tau(n) be the first j with `3^q < 2^j`, and t(n) the first j with
`T^j(n) < n`. A first-contraction word has `3^{q_s} >= 2^s` at every
proper prefix and `3^q < 2^j` at its end. Always `t(n) >= tau(n)`.
Equality for every n >= 2 is Terras's coefficient stopping time (CST)
conjecture, a separate open conjecture related to Collatz.

## 1. The exact extremizer

Set alpha = log_2(3). For q >= 1, every first-contraction word with q
odd steps has length `j = ceil(q alpha)`. Among these words the exact
largest affine offset is

\[
C_q=\sum_{h=0}^{q-1}3^{q-1-h}2^{\lfloor h\alpha\rfloor}.
\tag{1}
\]

It is attained uniquely by putting the odd steps at the zero-based positions

\[
i_h=\lfloor h\alpha\rfloor,\qquad 0\le h<q,
\]

and making all remaining steps even. In particular,

\[
C_0=0,\qquad C_{q+1}=3C_q+2^{\lfloor q\alpha\rfloor}.
\tag{2}
\]

**Proof.** The last step must be even: an odd step multiplies the coefficient
by 3/2 and cannot take it below one. Thus `2^{j-1} <= 3^q < 2^j`, which
fixes j. Immediately before the (h+1)st odd step, prefix survival requires
`2^{i_h} <= 3^h`; hence `i_h <= floor(h alpha)`. Expanding the affine
recurrence gives the sum in (1), with each `floor(h alpha)` replaced by
the actual position. Every summand increases strictly with that position.

All the upper positions can be attained simultaneously. Their successive
differences are one or two. After h odd steps, all even prefixes before the
next odd step have length at most `floor(h alpha)` and still have coefficient
at least one. Following the last odd step, the same holds through j-1;
the final even step first crosses the boundary. This proves feasibility,
maximality and uniqueness. For q=0 the only first contraction is the
one-letter even word, whose offset is zero. QED.

The extremizer maximizes C. It need not minimize the positive integer in
the corresponding residue class. Confusing those two optimizations would
incorrectly turn an envelope theorem into a proof of CST.

## 2. A uniform quarter-slope bound, with sharp intercept

Every first-contraction word satisfies

\[
\boxed{\frac{C}{3^q}\le\frac q4+\frac{11}{108}.}
\tag{3}
\]

The constant 11/108 is the smallest possible when the coefficient of q
is fixed at 1/4. Equality holds for q=3 and the parity word `11010`:
the trajectory `11,17,26,13,20,10` has C=23 and `23/27 = 3/4+11/108`.

**Proof by a finite rotation certificate.** Define

\[
f(x)=2^{-\{x\}},\quad
F_m(x)=\sum_{i=0}^{m-1}f(x+i\alpha),\quad
M_m=\max_{0\le x<1}F_m(x).
\]

Between its discontinuities, F_m is strictly decreasing. Its maxima
therefore occur at the m points `x={-h alpha}`, `0 <= h < m`.
The value of the ith summand at such a point is exactly

\[
f((i-h)\alpha)=
\begin{cases}
2^{\lfloor k\alpha\rfloor}/3^k,&k=i-h\ge0,\\
3^{-k}/2^{\lceil(-k)\alpha\rceil},&k=i-h<0.
\end{cases}
\]

No logarithm approximation is needed: the floors and ceilings are the
bit lengths of small integer powers of three. Checking the 78 candidate
phases for m=1 through 12 gives:

| m | M_m | maximizing h |
|---:|---:|---:|
| 1 | 1 | 0 |
| 2 | 7/4 | 1 |
| 3 | 23/9 | 0 |
| 4 | 119/36 | 1 |
| 5 | 319/81 | 0 |
| 6 | 1213/256 | 5 |
| 7 | 5581/1024 | 6 |
| 8 | 14501/2304 | 5 |
| 9 | 64565/9216 | 6 |
| 10 | 159181/20736 | 5 |
| 11 | 695773/82944 | 6 |
| 12 | 2349463/262144 | 11 |

In particular, `M_12 < 9`, and `M_r <= 3r/4 + 11/36` for 1 <= r < 12.
For r=0 the empty sum is zero and obeys the same inequality.
Partition any q-term sum into blocks of length 12 and a final block of
length r. The bounds hold for every initial phase, so

\[
F_q(x)\le \frac{3q}{4}+\frac{11}{36}.
\]

Equation (1) gives `C_q/3^q = F_q(0)/3`, proving (3), including its
sharpness. The table uses only rational arithmetic and can be checked
without RAD; `rotation.rad` and Python's `Fraction` independently check it.
QED.

For comparison, prefix survival alone gives the elementary `C/3^q <= q/3`.
Equation (3) reduces that leading coefficient by 25%. The coefficient 1/4
is convenient, not asymptotically optimal. Irrational rotation
equidistribution applied to (1) gives

\[
\frac{C_q}{q3^q}\longrightarrow
\frac13\int_0^1 2^{-x}\,dx=\frac{1}{6\log 2}.
\]

The all-size proofs above, rather than finite trajectory counts, support
these statements. The rotation reduction supplies the link between the
finite rational table and the universal inequality.

## 3. Turning envelopes into descent certificates

At a first contraction of length j,

\[
T^j(n)<n\quad\Longleftrightarrow\quad
n>\frac{C}{2^j-3^q}.
\tag{4}
\]

Equation (1) gives a sharp uniform offset envelope for each q; equation (3)
gives a compact bound for all q. Neither alone proves that (4) holds for
every compatible positive integer.

For a finite horizon J define

\[
B(J)=1+\max_{1\le q,\,\lceil q\alpha\rceil\le J}
\left\lfloor\frac{C_q}{2^{\lceil q\alpha\rceil}-3^q}\right\rfloor.
\]

Every n >= B(J) with tau(n) <= J has t(n)=tau(n). Notice the strict
inequality: replacing floor+1 by a ceiling fails when the ratio is integral.
The first example is n=1, with C=1 and denominator gap 1.

RAD evaluates (2) through J=4096 with arbitrary nonnegative integers
implemented in ordinary RAD. Its records include:

| first-contraction length | odd steps | integer cutoff |
|---:|---:|---:|
| 485 | 306 | 72059 |
| 1539 | 971 | 238671 |
| 2593 | 1636 | 420842 |
| 3647 | 2301 | 620859 |

No later record occurs through 4096, so B(4096)=620859.
RAD and an independent Python implementation also follow all 620857 starts
`2 <= n < 620859` through their first coefficient contraction. All descend
at that step. The run uses 2165601 literal shortcut steps, with largest
coefficient stopping time 173 and largest visited value 12324038948.

Consequently, **for every integer n >= 2, if tau(n) <= 4096, then
t(n)=tau(n)**. This result uses the local computation and written proof;
it does not import a published numerical verification range. It does not
assert that every n has tau(n) <= 4096, or that every tau(n) is finite.
This finite-horizon result is a reproducibility exercise, not a record claim.

## 4. Two much larger horizon corollaries

Here we deliberately use the simpler bound `C/3^q <= q/3` to keep the
certificate short. Let positive integers a,b,c,d satisfy

\[
\frac ab<\alpha<\frac cd,\qquad cb-ad=1.
\]

If a fraction j/q lies strictly between a/b and c/d, the positive integers
`bj-aq` and `cq-dj` obey

\[
j=c(bj-aq)+a(cq-dj)\ge a+c.
\]

Thus a contracting word of length `j < a+c` has `j/q >= c/d`.
Let `delta=j log 2-q log 3 > 0`. Since `exp(delta)-1 > delta`, (4) has
threshold strictly less than

\[
\frac{q}{3\delta}\le
\frac{d}{3(c\log 2-d\log 3)}.
\tag{5}
\]

RAD bounds both logarithms using 192 terms of
`2 atanh(1/r) = 2 sum_{h>=0} 1/((2h+1)r^{2h+1})`, with r=3 for log 2
and r=2 for log 3. At scale S=2^192, summing the floored terms gives L;
the exact logarithm lies between L/S and (L+385)/S. The 192 term-rounding
errors total less than 384 scale units. The remaining geometric tail is
at most `2S / (385 r^385 (1-r^-2)) < 1` scale unit. Python independently
checks these rational bounds, the Farey determinants, and the strict margins.

| a/b | c/d | starting floor N | last certified j |
|---|---|---:|---:|
| 103768467013/65470613321 | 10439860591/6586818670 | 2^71 | 114208327603 |
| 9809721694/6189245291 | 630138897/397573379 | 28000000000000000000 | 10439860590 |

In both cases (5) is strictly below N. These are unconditional implications
for starts n >= N. Two further conclusions explicitly use outside results:

1. Using the published convergence verification below 2^71, any *least*
   Collatz counterexample must keep its coefficient at least one through
   114208327603 shortcut steps. An earlier first contraction would make it
   visit a smaller integer, contradicting leastness. The external floor is
   from [Barina, 2025](https://link.springer.com/article/10.1007/s11227-025-07337-0).
2. Using the published CST verification for `2 <= n <= 2.8*10^19`, **any CST
   counterexample must have tau(n) >= 10439860591**. Smaller starts are
   covered by that result; larger starts are covered by this certificate.
   The external CST result is Corollary 5.2 of the
   [Rozier–Terracol preprint](https://arxiv.org/html/2502.00948v2).

These numbers come from standard rational approximants to log_2(3), also
used in established cycle bounds. We claim neither a new cycle-length
record nor an independent re-verification of the cited enormous ranges.
The certificate checks arithmetic implications of those external results.

## 5. Scope and related work

Affine parity formulas and stopping-time methods go back to Terras.
Rozier and Terracol study the additive remainder, its change under parity
interchanges, and paradoxical sequences. Their harmonic-mean bounds are
closely related to the logarithmic inequality in (5). These are established
ingredients, not inventions of this project. The potentially distinct part
here is the explicit extremizer under the *first-contraction prefix
constraint* and the 12-step proof of the sharp quarter-slope intercept.

The [Niu preprint](https://arxiv.org/html/2605.13886v1) also studies parity
vectors and finite-length paradoxical counting. A recent informal
[finite-observation note](https://note.com/inari2254172/n/n0163ca820bd3)
discusses first crossings and separates additive and congruence constraints.
We inspected these connections; we have not completed a priority review of
the older stopping-time literature. No "first", "breakthrough", or
publication-level originality claim follows from not finding an exact match.

The remaining mathematical obstacle is precise. A bound on C alone does
not control how small a positive integer in the compatible parity residue
class can be, and the logarithmic gap can be very small. Nor does this work
force an arbitrary fixed positive integer ever to have a contracting
coefficient. An infinite noncontracting 2-adic parity path need not represent
a positive integer. These are the places where a proof of Collatz would
need an additional idea.
