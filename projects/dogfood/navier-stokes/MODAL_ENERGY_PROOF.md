# A uniform modal energy bound, with a sharper constant

2026-09-09. This proves an energy estimate for the actual two-by-two operator
defined in `GrowingMode.modalOperator` and used by `PrimaryODE.FrameData`.
It does not certify the full construction or its primitive coefficient bounds.

Write the source's modal operator as

    A_j = diag(lambda,-lambda) - j^2*v I + E,

where lambda>=0, v>=0, j is a nonzero integer, and each entry of E has
absolute value at most c=C/S. Suppose S>0, C>=0, and
v>=v_ref-D/S. Then for every z in R^2,

    <z,A_j z> <= [lambda-v_ref+(D+2C)/S] |z|^2.          (1)

This improves the source's `D+4C` to `D+2C` under exactly these coefficient
inequalities.

## Proof covering every state and every nonzero harmonic

First z^T E z <= c(|z_1|+|z_2|)^2 <= 2c|z|^2. Also
lambda(z_1^2-z_2^2)<=lambda|z|^2, and j^2*v>=v>=v_ref-D/S.
Adding these three inequalities proves (1). In particular, no finite harmonic
cutoff is needed for the energy estimate. The coefficient 2 in the independent
entrywise error bound is sharp: E has every entry c and z=(1,1).

RAD independently checks the quadratic inequality by sum-of-squares identities.
At each of the 16 vertices of the normalized entry box [-1,1]^4, write the
entries a,b,c,d and set t=(b+c)/2 in {-1,0,1}. The exact certificate is

    2(x^2+y^2)-[a x^2+(b+c)xy+d y^2]
      = |t|(x-t y)^2
        +(2-a-|t|)x^2+(2-d-|t|)y^2.

All coefficients on the right are nonnegative. The inequality at an arbitrary
point of the coefficient box follows because that point is a convex combination
of its vertices and the left side is affine in the four entries. These are
not 16 sampled fluid states: each certificate holds for all real x,y.
For C=0 the error matrix is zero and the conclusion is immediate.

## Uniform weighted propagator

Let P>0 satisfy P'/P=lambda-v_ref along a fixed path. For z'=A_j z, (1) gives

    d/dt (|z|^2/P^2) <= 2(D+2C)/S * (|z|^2/P^2).

Integrating, for 0<=s<=t<=L and L<=MS,

    |z(t)|/P(t) <= exp((D+2C)(t-s)/S) |z(s)|/P(s)
                 <= exp((D+2C)M) |z(s)|/P(s).             (2)

The bound is uniform in the physical band and nonzero harmonic when D,C,M
are the fixed reference-family constants. The elementary differential inequality
also covers zero states by applying it directly to squared norms. The polynomial
RAD certificates verify the quadratic algebra; the integrating-factor argument
here supplies its analytic consequence.

For an inhomogeneous source g, variation of constants gives

    |z(t)|/P(t) <= exp((D+2C)M)
      [|z(0)|/P(0)+integral_0^t |g(s)|/P(s) ds].          (3)

Differentiated equations require bounds on the differentiated coefficient and
source terms. Formula (3) is the uniform propagator estimate those equations
use; it does not itself provide their input bounds.

## The premise was traced to its constructor

`ActualParticularControl.selected_energy` reads coefficient estimates from
`PhaseConstruction`; that record alone is not a proof that the actual fields
satisfy them. The source constructs the record in
`BasePhaseGeometry.construction`, with

    C=modalConstant M u,
    D=dampingConstant M,
    M_reference=outputBound M.

Its `modal_errors` theorem uses actual coordinate-error comparisons and
reference-profile bounds; `damping_error` uses the actual normal comparison
and viscosity normalization. Both require the selected base-field assumptions
and `LargeBand`. This traces the premise to mathematical input rather than
inventing a numerical energy constant. Those base/phase estimates, all-order
input jets, localization errors, and subsequent correction preservation have
not been independently established in this RAD audit.

## Validation

Run `python projects/dogfood/navier-stokes/verify_modal_energy_sos.py`.
All 16 polynomial certificates pass. Record/replay passes. Replacing the sharp
factor 2 by 1 is rejected. Receipts: `modal_energy_sos.json` and
`modal_energy_sos.why.txt`. The full-construction certificate remains false.
