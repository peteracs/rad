# An explicit additional angular strain mode

The identified nonlinear term can be canceled at a reference time by a
divergence-free velocity correction. The construction below also explains
why adding only this correction does not solve the full evolution.

## Choose the missing direction

Keep A=diag(1,2,-3), q=|y|^2, H=y cross Ay, and

    V = curl_y[-chi(q) H/3],
    a = chi+(2q/3)chi',
    c = (14/3)chi'+(4q/3)chi''.

The troublesome nonlinear curl is -2ac AH. Introduce the independent
trace-free symmetric matrix

    B = A^2 - (tr A^2/3)I = diag(-11/3,-2/3,13/3).

Then y cross By = -AH. Define the additional angular strain field

    Z(y) = curl_y[-psi(q) (y cross By)/3].

It is divergence-free by construction, and

    curl_y Z = -[(14/3)psi'+(4q/3)psi''] AH.

Thus choose psi to solve

    (14/3)psi' + (4q/3)psi'' = -2a(q)c(q).                 (1)

This gives curl Z=+2ac AH, exactly the missing opposite term. B supplies
a second angular strain mode, not a rigid rotation of the original core.

For dimensionless frozen scales, set u(t,y)=V(y)+t Z(y). At t=0 the
contribution curl u_t cancels the identified part of curl[(V dot grad)V].
For physical reference scales S_0,R_0 the corresponding added velocity is

    (t-t_0) S_0^2 R_0 Z(x/R_0).

Its time derivative has the required physical curl prefactor S_0^2.
Other components of the original residual are not canceled by this step.

## Explicit local polynomial

On the quadratic collar chi=(1-q)^2, the right side of (1) is

    h(q) = 56/3 - (776/9)q + (1112/9)q^2 - 56q^3.

A particular solution is

    psi(q) = 4q - (194/27)q^2 + (556/99)q^3 - (21/13)q^4.

The RAD workload represents this polynomial exactly with denominator
216216. Its integer numerators, in ascending degree, are

    [0, 864864, -1553552, 1214304, -349272].

These are local polynomial identities. Extending this polynomial to all
space would give a growing field and would not meet finite-energy conditions.

## A global finite-energy correction for a genuine smooth cutoff

Now take chi smooth, equal to 1 for q<=1/4 and 0 for q>=1. Then
h=-2ac is smooth and supported in that annulus. Equation (1) has the
regular, decaying solution

    psi(q) = -(3/10)[q^(-5/2) integral_0^q s^(5/2) h(s) ds
                              + integral_q^infinity h(s) ds].       (2)

At q=0 interpret the first term by its zero limit. Differentiation gives

    psi'(q) = (3/4) q^(-7/2) integral_0^q s^(5/2)h(s) ds,
    (q^(7/2)psi')' = (3/4) q^(5/2)h,

which verifies (1). Near the origin psi is constant, so Z is a smooth
affine B field there. Outside the annulus psi is a constant times q^(-5/2),
so Z=O(|y|^(-4)); Z has finite energy. Its vorticity vanishes outside
the annulus, making the exterior tail irrotational as well as divergence-free.

This preserves compact vorticity, but generally introduces both an affine
B contribution in the inner core and a velocity tail outside it. It does
not assert compact velocity support or Schwartz decay for Z. Taking the
correction amplitude zero at the initial reference time retains the
original compact initial velocity.

The polynomial particular solution and (2) need not agree on a collar:
their difference there can include the homogeneous terms C_0+C_1 q^(-5/2).
Those terms have zero vorticity in that collar, so both solutions cancel
the same targeted curl, but their subsequent nonlinear interactions differ.

## The full residual after adding this mode

For u=V+tZ at frozen scales, the curl of the momentum residual is

    curl Z + curl[(V dot grad)V] - nu Delta curl V
    + t{curl[(V dot grad)Z+(Z dot grad)V] - nu Delta curl Z}
    + t^2 curl[(Z dot grad)Z].

The constant term's AH contribution is canceled. The linear and quadratic
terms are new obligations, not zeros implied by (1).

For the explicit polynomial particular solution above, take
y=(1/2,1/2,1/2) and L(F)=4F_1+5F_2. Exact differentiation gives

    L(curl residual)
      = t[435505/54912 + (2245/9)nu]
        - t^2[9263275/3953664].

The constant coefficient is exactly zero, but the coefficient of t is
positive for every nu>0. Therefore this single linear-in-time correction
does not maintain even the targeted projection's cancellation. These
specific coefficients concern the local polynomial particular solution,
not a universal assertion about the global solution (2).

The next mathematical obligation is a coupled evolution for these angular
modes, including their cross interactions, viscosity, pressure, and exterior
matching. Repeated Taylor corrections alone would not establish convergence,
an infinite-stage blowup construction, or admissible all-order force bounds.

## Executed checks and limits

`angular_correction.rad` constructs coefficient certificates for chi=(1-q)^m,
m=0,1,2, in separate worlds using `fork_with` and `simulate_many`. The causal
resolver verifies (1) coefficient by coefficient, independently of the
construction function, rejects duplicate cases, and records `why()` ancestry.
It also checks speculative write isolation and serialized-world preservation.

`verify_angular_correction.py` compares the certificates with independent
symbolic derivatives, checks the three-dimensional divergence and full-vector
angular cancellation, and computes the residual above. Wrong-sign and
altered-coefficient certificates are rejected. One/four-worker outputs match
and the recorded final world verifies on replay.

RAD verifies finite coefficient arithmetic. SymPy checks the spatial algebra.
The global integral formula and its finite-energy interpretation are written
analytic arguments; no PDE existence or blowup theorem is kernel-verified.
