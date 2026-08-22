# Long-shift fourth-trace route: rigor audit

This note isolates the analytic route consumed by
`analytic_theorem_prover.rad`. It does **not** currently upgrade the
fourth-trace scout to an asymptotic theorem. Three novel analytic obligations
are listed below. All limits in the proposed route are taken with fixed
bandwidth `3/4 < lambda < 1`; only after that limit is established is `lambda`
allowed to tend to one from below.

The external analytic input is K. Matomaki, M. Radziwill, and T. Tao,
*Correlations of the von Mangoldt and higher divisor functions I. Long shift
ranges*, arXiv:1707.01315v3: specifically Corollary 3.2, Lemma 2.15,
Propositions 5.4 and 6.1, and the Type `d1`/`d2`/II estimates summarized in
Remark 7.1. The zero-side
signature and trace setup is the paper *More than two thirds of the Riemann
zeta zeros lie on the critical line* (August 10, 2026), Sections 2--5 and 7.5.

## 1. The shift range is already long

Put `X = T^lambda` (harmless factors of `2*pi` are suppressed) and `Y=X^2`.
The non-diagonal term in the fourth trace has additive shifts

```text
|h| <= X^2/T = Y^alpha,
alpha = (2*lambda-1)/(2*lambda).
```

The published long-shift threshold `alpha>8/33` is equivalent to
`lambda>33/50`.  The proof below deliberately uses only the older, simpler
Type-I/II range `alpha>1/3`, equivalent to `lambda>3/4`.  Thus every bandwidth
sufficiently close to one is strictly inside the range, with room to spare;
none of the delicate estimates used to break the `1/3` barrier are needed.

## 2. Averaged correlations for the truncated Lambda convolution

The fourth-trace expansion contains the truncated convolution

```text
A_X(n) = sum_{ab=n, a<=X, b<=X} Lambda(a)Lambda(b).
```

After a smooth dyadic partition in `a`, `b`, and `n`, write a typical component
as `A_{P,Q}` with `a` and `b` restricted to dyadic intervals of lengths `P`
and `Q`, where `PQ` is the scale of `n` and `P,Q<=X`.  For every fixed
`epsilon>0`, `B>0`, and
`Y^(1/3+epsilon) <= H <= Y^(1-epsilon)`, the circle-method proof of the
averaged-correlation theorem of Matomaki--Radziwill--Tao applies to each such
component (and to cross-correlations between two components) and
gives its expected major-arc asymptotic for all but
`O_B(H log(Y)^(-B))` shifts `|h-h0|<=H`.  Here is the extension check, since
their displayed theorem lists `Lambda` and fixed divisor functions rather than
truncated Lambda convolutions.

1. Apply the fixed-order Heath--Brown identity to each of the two Lambda factors
   in `A_{P,Q}`, then dyadically decompose every convolution variable.  Each
   resulting coefficient is a fixed convolution of `1`, `log`, and truncated
   `mu` factors.  Logarithmic scalar losses are harmless because the
   circle-method estimates save an arbitrary prescribed power of `log Y`.

2. The proof of the combinatorial decomposition is generic after this
   preliminary expansion: multiply adjacent factors until either a block has
   size in `[Y^epsilon,H0]` (Type II), or all unabsorbed factors exceed `H0`.
   Taking `m=3` and `H0>=Y^(1/3+epsilon)`, at most two unabsorbed factors can
   remain.  Thus only Type `d1`, Type `d2`, and Type II components occur.  The
   estimates for precisely these three component classes are the generic
   estimates used in the proof above the `Y^(1/3+epsilon)` range; their proofs
   depend on divisor boundedness, support, and good cancellation, all preserved
   by the doubled Heath--Brown expansion.

3. On a major arc `a/q+beta`, the character twist of a dyadic component
   factorizes into two truncated logarithmic derivatives.  Before truncation
   the identity is

   ```text
   sum (Lambda*Lambda)(n) chi(n) n^(-s) = (-L'/L(s,chi))^2.
   ```

   Perron inversion and partial summation give the same leading term component
   by component, with the dyadic cutoffs retained.  The principal character's
   additive-character coefficient
   is `mu(q)/phi(q)` at leading order: terms in which a fixed prime divisor of
   `q` is itself one of the two convolution factors have only order `x` and do
   not alter that leading coefficient.  For non-principal characters the
   standard zero-free-region/Siegel-theorem argument used by the cited major
   arc proposition gives an arbitrarily large logarithmic saving.  Summing the
   principal contributions over `q` gives the usual prime-pair singular series
   `S(h)`.  Consequently the leading correlation term is

   ```text
   sum_{Y<n<=2Y} A_X(n)A_X(n+h)
       = S(h) M_X(Y) + o(M_X(Y))
   ```

   where `M_X(Y)` is the explicit component-summed diagonal main term.  This is
   uniform after smooth dyadic partition and partial summation.  The standard
   mean-value identity for the singular
   series says that its average against every fixed compactly supported
   piecewise-C2 weight is one; this recovers the unweighted leading constant
   after the trace sums over `h`.  Lower-degree major-arc polynomials do not
   affect the normalized fourth trace.

4. The exceptional shifts are harmless even with arbitrary bounded smooth
   weights.  Indeed

   ```text
   sum_{Y<n<=2Y} A_X(n)^2 <= sum_{Y<n<=2Y} (Lambda*Lambda)(n)^2
       << Y log(Y)^3,
   ```

   by expanding the two fixed divisor sums (proper prime powers contribute a
   lower order term).  Cauchy--Schwarz gives the same bound for every shifted
   correlation. Choose `B` larger than two plus the fixed logarithmic loss from
   the Heath--Brown and dyadic component pairs. The union of their exceptional
   sets then contributes

   ```text
   O(H log(Y)^(-B) * Y log(Y)^3)
       = o(H Y log(Y)^2).
   ```

Steps 1--4 describe the intended extension, but MRT's stated theorem does not
contain this balanced truncated coefficient. In particular, citing its generic
Type estimates is not a substitute for proving that the doubled Heath--Brown
decomposition satisfies every support/cancellation hypothesis, nor for a
uniform weighted major-arc calculation. These are open obligations 1 and 2.

## 3. Transfer to the fourth trace

Expand `tr(G^4)` from the prime-side representation of `G`, use the Poisson
identity for the complete Gabor grid, and remove the two endpoint strips by the
same `L1/L2` kernel bounds used for `tr(G^2)`.  Terms containing an
archimedean remainder or `Pi_X` are lower order by Cauchy--Schwarz and the
pointwise bounds already used in Propositions 5.3--5.7 of the base argument.
The exact multiplicative diagonals give the Rudnick--Sarnak contributions.
The only new non-diagonal is

```text
sum_h W_T(h) sum_m A_X(m)A_X(m+h),
A_X = (Lambda 1_[1,X]) * (Lambda 1_[1,X]),
|h| <= X^2/T,
```

where `W_T` is the effective shift weight obtained from the four window
factors. Split it into a fixed smooth core at scale `H` and dyadic tails. The
core is a bounded piecewise-C2 weight to which Section 2 applies after a smooth
partition in `m`; the tails are bounded by the same generalized Hilbert
inequality used for the second trace and form a convergent geometric error.
Boundary values and the `h=0` term are negligible because `H` is a positive
power of `Y`.

If the correlation extension and quantitative localization above are proved,
it would follow, for every fixed `3/4 < lambda < 1`, that the first four normalized
traces are the sine-process Gram moments.  These moments are finite integrals
of compactly supported window products and hence are continuous as
`lambda -> 1-`.  At the endpoint they are

```text
m0=1, m1=1, m2=4/3, m3=2, m4=13/4.
```

The passage from the correlation to this trace formula, including endpoint
strips and shift tails at fourth order, is open obligation 3. No assertion at a
moving bandwidth should be used: first take `T -> infinity` at each
fixed bandwidth, then take the supremum over fixed bandwidths approaching one.

## 4. The exact conditional 70% deduction

For moments `1,1,4/3,2,C`, the degree-two Christoffel function at zero is

```text
Lambda_2(0) = (9C-28)/(36C-108).
```

At `C=13/4` this is `5/36`.  The one-sided moment bound gives at least
`1-5/36=31/36` positive eigenvalues.  The unconditional signature conversion
charges an off-line pair once but two zeros in `N`, so the simple-on-line
proportion is

```text
2*(31/36)-1 = 13/18 = 72.222...%.
```

For the requested threshold alone, `C<=59/18` would give exactly `70%`.
The limiting value `13/4` has the strict margin `59/18-13/4=1/36`.
