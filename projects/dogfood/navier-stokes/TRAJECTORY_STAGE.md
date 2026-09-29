# Complete finite trajectories, force smoothness and stage joins

**Implemented and tested:** a bounded repair grammar of whole smooth
trajectories, exact full Fourier force representations, sufficient mixed
derivative bounds, and all-order smooth joins between adjacent stages.
**Not proved:** a uniform infinite-stage force budget or a blowup construction.

The fixture below is a software/analytic verification example. It is not a
new Navier–Stokes singularity mechanism or evidence that a large trajectory
search has succeeded. No kernel changes were needed.

## Trajectory grammar

For a stage [t0,t0+1/h], put s=h(t-t0) and define

\[
 u(t,x)=A(s,x)+\theta(s)B(s,x),
 \quad A=\sum_{i=0}^2s^i A_i(x),\quad B=\sum_{i=0}^2s^i B_i(x).
\]

The coefficients are divergence-free real finite Fourier fields. Define the
fixed smooth step using rho(s)=exp(-1/s) for s>0 and rho(s)=0 otherwise:

\[
 \theta(s)=\frac{\rho(s)}{\rho(s)+\rho(1-s)}.
\]

It is zero for s<=0, one for s>=1, increasing, and has every positive-order
derivative zero at 0 and 1. These elementary smoothness facts are analytic
inputs, not formalized in the RAD kernel. Unlike a flat clock applied to an
entire path, this representation retains A's and A+B's generally nonzero
endpoint derivatives. It does not impose u_t=0 at each join.

`SmoothStage` currently supports quadratic paths on a common ordered bank of
at most 12 frequencies with coordinates in [-3,3], integer coefficient
numerators in [-8,8] over denominator two, integer inverse durations 1..4,
and viscosity 1..16. Interpolation evaluations must also fit the underlying
Fourier operator's checked coefficient domain. These are implementation
limits, not assumptions about a potential blowup theorem. A union bank with
zero entries can represent different active supports between adjacent stages.

## The derivative comes from the trajectory

It is no longer an independently proposed array:

\[
 u_t=h[A_s+\theta B_s+\theta' B].
\]

With the periodic Leray projection P, the chosen divergence-free force is

\[
 f=G(s,\theta(s),x)+h\theta'(s)B(s,x),
\]
\[
 G(s,z)=h(A_s+zB_s)+P[((A+zB)\cdot\nabla)(A+zB)]
              -\nu\Delta(A+zB).
\]

The complementary gradient determines the global periodic pressure. Every
generated Fourier frequency is retained. G has degree at most four in s
and two in z. RAD recovers its coefficients exactly from the 5 by 3 integer
interpolation grid. This is a polynomial identity argument, not a claim that
15 arbitrary samples validate a general function. The actual stage uses
theta(s), not the interpolation ordinate z.

The separate switching term h theta' B is essential and is never dropped.
`StageForceMode` stores all 15 complex polynomial coefficients for each
frequency, their rational denominator and their coefficient l1 sum.

## Actual derivative bounds on a whole interval

Let M_G be the sum of absolute real and imaginary coefficients of G,
including all vector components and Fourier modes. Let M_B be the analogous
sum for B. Let K>=1 bound every Fourier coordinate appearing in G or B.
For each fixed m define universal, finite constants

\[
 C_m=\max_{0\le i\le4,\,0\le j\le2}
       \|\partial_s^m(s^i\theta(s)^j)\|_{L^\infty([0,1])},
 \quad
 H_m=\max_{0\le i\le2}
       \|\partial_s^m(s^i\theta'(s))\|_{L^\infty([0,1])}.
\]

Direct differentiation of the finite Fourier representation gives

\[
 \|\partial_x^\alpha\partial_t^m f\|_\infty
 \le K^{|\alpha|}\left(h^m C_m M_G+h^{m+1}H_m M_B\right).
\]

This supplies a sufficient bound for **every** mixed derivative, conditional
only on the fixed smooth-step lemma, with the actual computed coefficients
as its stage-dependent inputs. Numerical values for arbitrary C_m,H_m have
not been generated. The existence of these fixed constants proves finite
stage smoothness; it does not bound their stage-dependent multipliers as
the number of stages tends to infinity.

For order zero, C_0<=1 and H_0<=64. One conservative proof of the latter:
rho'(s)<=4/e^2 for s>0, each rho factor on [0,1] is <=1, and the denominator
rho(s)+rho(1-s)>=e^-2. The quotient rule yields theta'<=8e^2<64.
`stage_bound` therefore computes an actual uniform force upper bound
M_G+64 h M_B, rounding M_G upward per Fourier mode. Exact rational
coefficient data remain available when a sharper estimate is needed.

The convention is the 2pi-periodic torus. The usual Navier–Stokes parabolic
rescaling transfers a finite trajectory and its force to the unit torus;
the corresponding frequency and force factors must be included there.

## Stage-to-stage closure

At the right endpoint the full smooth germ of u agrees to every order with
A_left+B_left. At the next left endpoint it agrees to every order with
A_right. Matching these polynomial germs in **physical time** is sufficient
for every velocity derivative to match, including all spatial derivatives.
Pressure and force then match to every order by their full-equation formulas.

`stage_join` checks the value and first two physical-time derivatives. This
is enough here because the endpoint germs are quadratic and every derivative
of the flat switching correction vanishes. It would not be enough for an
arbitrary smooth trajectory grammar.

`carry_stage` generates the next A by exact translation and duration scaling
of the previous A+B. It preserves all endpoint jets. A duration change that
requires finer rational coefficients than the current grid is rejected,
not rounded. A proposal can choose a new B while carrying the inherited A.

This is smooth concatenation of specified adjacent trajectories. It is not
closure of an invariant class under infinitely many amplifying transitions.
The implementation's coefficient bounds do not permit silently iterating
beyond their domain.

## Executed causal search and validation

`trajectory_search.rad` evaluates two whole two-stage proposals in isolated
forks. One resets the inherited time derivatives; one carries them exactly.
The resolver recomputes the evidence and selects the smoothly joined path.
WHY records the selection through `SubmitTrajectory` and `ChooseTrajectory`.

The selected fixture has enstrophy E(0)=1 and E(2)=73/2. Its enstrophy is
nondecreasing on both full intervals: independent symbolic calculation gives
nonnegative coefficients for both partial derivatives of E(s,z), and
s,theta(s),theta'(s) are nonnegative. Its conservative force bounds are 282
and 340. This is ordinary prescribed forced growth, not a small-residual or
finite-time blowup result.

`verify_trajectory_stage.py` checks four stages, including a change of
duration, against independent symbolic Fourier algebra. All 76 complete
force polynomials match exactly, including the switching derivative.
Endpoint polynomial germs match identically. A physical derivative mismatch
is detected. Forging an infinite-budget claim is rejected. One/four-worker
output is identical and recorded-world replay verifies.

Run from the repository root with SymPy available:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_trajectory_stage.py
```

The force on the finite closed interval admits a smooth periodic extension
with compact time support, by extending the outer polynomial germs and then
applying a later smooth cutoff to the force. The displayed velocity is only
asserted to solve the equation on its constructed interval; it is not a
claimed global solution for that cutoff extension.

## Remaining infinite-stage obligation

For an infinite construction, the coefficient bounds above must imply the
required summable residual-increment estimates at every fixed derivative
order, with compatible force endpoint jets at the accumulation time. That
requires quantitative control of M_G,q, M_B,q, h_q and K_q for the actual
constructed sequence (or sharper bounds exploiting cancellations), together
with convergence and a singularity lower bound. The current fixture does
not satisfy or establish those requirements. The decision explicitly records
`infinite_stage_closure_proved: false`.
