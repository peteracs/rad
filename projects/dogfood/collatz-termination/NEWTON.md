# Integer-valued rankings and structural search reductions

The target remains termination of the complete eleven-rule system in
[PROOF_TARGET.md](PROOF_TARGET.md). No complete certificate has been found.
This extension expands the functions the search can express; it is not a
theorem asserting Collatz convergence.

## Exact certificates for an infinite integer domain

Write a letter interpretation in the classical binomial basis:

`f(x) = sum(i=0..p, c_i * binomial(x,i))`, with integer `c_i`.

These are integer-valued polynomials. Coefficients may be negative. For
example, coefficients `[0,7,-6,6]` give `(x-2)^3+8`, whose values at
`0,1,2,3,4` are `0,7,8,9,16`. The function is nonnegative and monotone
on nonnegative integers although its second forward difference is initially
negative. Earlier nonnegative-coefficient templates exclude this function.

For a polynomial P of degree at most D, let `Delta P(x)=P(x+1)-P(x)`.
At any integer origin H the Newton identity is

`P(H+t) = sum(i=0..D, Delta^i P(H) * binomial(t,i))`.

It holds for every nonnegative integer t. One proof uses Pascal's identity:
the right side has the same initial forward differences as P; their
difference is a polynomial of degree at most D with D+1 distinct zeros.

Thus the following finite certificate proves `P(x)>=0` for **every**
nonnegative integer x:

* Check `P(0),...,P(H-1)>=0`.
* Check `Delta^i P(H)>=0` for `0<=i<=D`.

For `P(x)>=1`, additionally require the prefix values and `P(H)` to be at
least one. Subtracting one changes only the zeroth Newton coefficient.

These are polynomial identity certificates, not extrapolation from positive
samples. For example, `100-x` is positive on its first hundred arguments
but fails the first-difference condition. The sequence `0,1,2,2` has
nonnegative values and first differences but a negative higher difference.

With H unrestricted, this method can certify every polynomial nonnegative
on the nonnegative integers. A nonconstant such polynomial has positive
leading coefficient; each of its nonzero iterated differences is eventually
positive. Choose H beyond their finitely many sign changes and check the
finite prefix. The zero and constant polynomials are immediate cases.
The implemented search bounds H; this completeness observation does not
turn a bounded search rejection into an unrestricted impossibility result.

## Closure, monotonicity, and strict boundary steps

For every letter, independently check `f(0)>=0` and certify
`Delta f(x)>=0` on all nonnegative integers. Pascal's identity gives
`Delta f(x)=sum(i=0..p-1,c_(i+1)*binomial(x,i))`, so the same checker applies.
These conditions prove both closure of the domain and preservation of weak
order. A negative coefficient is allowed only when the full certificate
still proves these properties.

For each rewrite rule, apply the certificate to its interpreted difference.
A composition of L letters has degree at most `p^L`; this independently
determines how many forward differences the verifier must check. The model
cannot supply a smaller degree bound or merely a list of favorable samples.

Every active rule must decrease weakly. Selected boundary rules decrease
by at least one on every integer, including the prefix before H. When all
selected boundary rules are strict, Section 2 of the main proof target
establishes termination. Partial removal remains a partial certificate.

## Grounding the terminal boundary

In the forward system, d occurs only as the terminal symbol in `ad -> d`
and `bd -> gd`. Given any acceptable scalar interpretation, replace d by
the constant `d(0)`. The new dynamic comparisons are the old ones evaluated
at zero. Every other rule and every selected strict boundary rule is
unchanged. The constant map is closed and monotone. The same argument in
the reversed system replaces c by `c(0)`.

This reduction preserves the coefficient bound: the retained value is the
original constant coefficient. It also preserves a finite-difference
certificate for the affected rules, which now have constant differences.
Consequently the current Newton search grounds the terminal boundary without
losing a model from its searched certificate family.

Natural and rational matrices permit the same reduction at the zero vector.
For max-plus matrices use the carrier point `(0,-infinity,...,-infinity)`.
Its image has coordinates `max(A_i0,b_i)`, still within the original finite
weight bound or minus infinity; the first coordinate is finite and
nonnegative. The `--ground` matrix option fixes the terminal linear part
to zero (minus infinity in max-plus). The existing matrix checker validates
the result without relying on this search reduction.

## Why uniform degree increases are inefficient

Suppose the five digit polynomials are nonconstant, and let their degrees
be alpha, beta, epsilon, phi, gamma for a,b,e,f,g respectively. Their
leading coefficients are positive because they map nonnegative integers
to nonnegative integers. Weak rule inequalities imply

`alpha*phi >= epsilon*beta`, `gamma >= phi`,
`epsilon >= phi`, and `beta*phi >= gamma*alpha`.

Multiplying the first and last inequalities gives `phi^2>=epsilon*gamma`.
The middle two give the reverse inequality. Therefore

`epsilon=phi=gamma` and `alpha=beta`.

Since c is nonconstant whenever a forward boundary rule is strict, the
`cf -> caa` comparison also forces `phi>=alpha^2`. If d is nonconstant,
`bd -> gd` further forces `alpha>=phi`, and hence all five digit degrees
equal one. These statements require the stated nonconstancy hypotheses;
they do not exclude the constant-boundary cases being searched.

The encoder adds the more general necessary degree comparisons directly,
including zero degrees for constant functions. One targeted profile uses
quadratic a,b and quartic e,f,g, a constant d, and the identity c. It
requires **all three** forward boundary rules to be strict. In that profile,
word compositions have degree at most eight after literal zero coefficients
are removed. This is a sufficient proof template, not an exhaustive
classification of polynomial interpretations.

## Implementation and checks

`newton.py` has two exact integer encodings. The first introduces variables
for binomial evaluations and uses the integral recurrence
`i*C(y,i)=(y-i+1)*C(y,i-1)`. It timed out even on the reduced control system.
`newton_expansion.py` clears factorial denominators before composing and
expresses forward differences as integer linear combinations of monomial
coefficients. Its first matching control search completed in about 0.73
seconds, and a full-system quadratic search returned UNSAT in about two
seconds, against a 90-second timeout for the first encoding. These are
observed run times, not a general performance guarantee.

The targeted quartic profile also has a signed bit-vector backend. Its width
is derived from an absolute arithmetic bound, not chosen empirically. Write
each letter with denominator 24. A degree-r letter with coefficient bound M
has numerator coefficient norm at most `24*M*(r+1)`: the sum of absolute
coefficients of the i-th falling factorial is `i!`. For an inner numerator
norm U and denominator V, composition gives bounds

`U_new = 24*M*(r+1)*max(U,V)^r`, `V_new = 24*V^r`.

The identity c has the tighter numerator norm 24. Cross multiplication for
a rule has difference norm at most `U_left*V_right+U_right*V_left`. If its
degree is D, every finite-difference weight through order D at H is bounded
by `2^D*(H+D+1)^D`. Multiplying these bounds and choosing two extra signed
bits covers every coefficient, partial sum, and composed numerator in the
encoding. The implementation removes literal leading zeros before using
the profile degrees. Tests compare this signed encoding with unbounded
integer expansion, including negative coefficients. SAT models still pass
the independent arbitrary-precision checkers.

RAD's `newton.rad` evaluates binomial polynomials using arbitrary-precision
natural numbers, separately accumulating positive and negative terms. It
checks exact division in the binomial recurrence. Its forward-difference
triangle rejects any negative entry. This is equivalent to checking its
first diagonal: Pascal summation reconstructs every other entry from that
diagonal with nonnegative integer coefficients.

Python instead uses `math.comb`, unbounded signed integers, and the direct
alternating binomial formula for each difference. Tests compare the symbolic
expansion with this independent evaluation, including reversed word orders,
unequal word lengths, and vanished leading coefficients. A separate RAD
probe checks a 286-bit composed value. The acceptance runner also tests
omitted rules, forged completeness, invalid domains, fork isolation,
snapshot restoration, and replay under changed worker counts.

Saved SMT files define the historical queries. Source changed during the
campaign; some queries predate signed coefficients, degree pruning, and
grounding. Receipt counts distinguish solver reports from checked concrete
models.
