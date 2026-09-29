# Signed correction and preservation of edge flatness

This ports two operator-level mechanisms from the supplied local source. It is
not a verification of the complete Navier–Stokes construction. Lean was read,
not executed. RAD checks exact polynomial identities and derivative expansions;
the all-order argument below is a mathematical induction, not a RAD kernel proof.

## Signed cancellation with a retained quadratic error

Let H be an invertible real 2-by-2 matrix, Y=H^-1 T with Y_j>0,
Z=H^-1 R, a_j=sqrt(Y_j), and b_j=Z_j/(2a_j). Then

    H(2 a_j b_j)_j = R,
    H((a_j+b_j)^2-a_j^2)_j = R + H(b_j^2)_j.

To cancel a residual, take R to be its negative. This does not remove
the second term. The matrix need not be symmetric or positive definite.
RAD verifies the adjugate identity and the scalar square expansion as free
polynomial identities; invertibility and positivity justify their divisions.

If Y_j>=l>0, |Z_j|<=d, and |H_ij|<=K, then

    b_j^2 <= d^2/(4l),
    |[H(b_j^2)]_i| <= 2 K d^2/(4l).

For nonnegative squared masks summing to one, the same bound holds for their
weighted sum, without multiplying by the number of labels. An additional
nonnegative multiplier bounded by Lambda multiplies this bound by Lambda.
These inequalities follow by positivity and the triangle inequality; the RAD
polynomial calculation alone does not certify their analytic premises.

Source: `NavierStokes/SignedCovariance.lean`, `cross_reconstruct` (line 62),
`squareColumn_abs_bound` (766), `mask_average_bound` (782).

## Why inverse square roots need not destroy flatness

For smooth scalar Z,Y with Y>0, every term of the m-th derivative of
b=Z/(2 sqrt(Y)) has the form

    c Z^(r) product_i Y^(s_i) Y^(-k-1/2),
    r + sum_i s_i = m,  s_i >= 1,  k = number of factors.

This holds at m=0. Differentiation either increases r, increases one s_i,
or adds a Y' factor while changing the inverse exponent to -k-3/2.
Those three cases prove the formula for every finite m by induction.
Mixed coordinate derivatives have the same factor count, with multiindices.

Suppose at a point every needed derivative of Z and Y is bounded by a
constant times w>0, while Y>=c0*w for c0>0. Each term is then bounded by

    C w^(1+k) / w^(k+1/2) = C sqrt(w).

The constants depend on derivative order and the supplied bounds. They are
not asserted to be independent of iteration stage. Polynomial losses in an
edge coordinate can also be retained: for w=exp(-c/t), any fixed polynomial
in 1/t times sqrt(w) tends to zero at t=0. Uniform local bounds in the other
coordinates give zero limits for every mixed derivative. Successive use of
the fundamental theorem of calculus then gives a smooth zero extension.

Crucially, this argument differentiates Z and Y themselves. It does not
differentiate Z/w or assume that such normalized functions have bounded jets.
The same w is used only to estimate all factors at the evaluation point.

`weighted_quotient.rad` implements the exact derivative recurrence through
order 8; `verify_signed_covariance.py` independently compares every expansion
with SymPy differentiation of arbitrary functions Z and Y. Finite execution
is evidence for that implementation, not an extrapolation to all orders.

Source: `WeightedQuotients.lean` and
`SignedCovariance.extendedIncrement_regular` (line 871). The source theorem
explicitly requires weighted jets of the actual inverse solves and a weighted
lower bound, as well as smoothness and the strict cone condition.

## Remaining construction obligation

We have not independently established those weighted premises for the actual
infinite family of H,T,R. Nor does this lemma bound the transport, localization,
viscous, pressure, and inter-wave terms of the complete physical residual.
The next dependency is preservation of the strict covariance cone and weighted
inverse-solve bounds in the actual correction iteration. Only after connecting
these to the physical residual estimates and the common cutoff schedule would
this give smooth force through the singular time. The supplied source claims
that broader construction; this port has not certified it.

Run `python verify_signed_covariance.py` with SymPy available. It also checks
RAD record/replay and rejects deliberately discarding the quadratic error.
