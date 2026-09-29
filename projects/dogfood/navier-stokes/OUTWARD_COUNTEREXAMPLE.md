# Retired: the proposed moment/production region is not invariant

This is a counterexample to the proposed region, not to Navier–Stokes
regularity. The viscosity is 1/30 and the kinetic-energy cap is 3, exactly
as in the original reference calculation.

On the periodic torus use the smooth divergence-free field

\[
u=\cos x(0,1,1)+\cos y(1,0,1)
 +\tfrac12\sin(x+y)(1,-1,1)+\cos z(1,1,0)
 +\tfrac12\sin(y+z)(1,1,-1).
\]

With normalized volume, the full pressure-projected Navier–Stokes vector
field gives

\[
 K=15/8<3,\quad E=9/4,\quad D=6,\quad S=1/2,
 \quad K'=-3/20,\quad E'=3/10,\quad D'=12/5.
\]

Consequently

\[
 B=S-2\nu D=1/10>0,\qquad Q=KD/E^2=20/9,
 \qquad Q'=16/135>0.
\]

The state satisfies every proposed inequality. Local smooth evolution exits
Q<=20/9 immediately. The strict production and energy inequalities persist
for a short time by continuity. Thus the requested universal inward-control
statement is false, independently of what happens to the production boundary.

## RAD verification

`spectral_rate.rad -- outward-boundary` evaluates full Fourier convolution
and Leray projection directly in integers, without importing the closed
moment formulas. It checks two amplitude-scaled witnesses at viscosity one.
The scaling u_new(t,x)=A u_old(A t,x), nu_new=A nu_old gives:

| A | Original viscosity | Production constraint | Scaled Q' |
|---|---|---|---|
| 24 | 1/24 | equality | 76/27 |
| 30 | 1/30 | strict | 32/9 |

All integer intermediates in this bounded two-case calculation fit int64.
`verify_outward_boundary.py` independently evaluates the symbolic full
Fourier field and compares all six moment/rate outputs and Q'. It also
checks the unscaled values above. Both witnesses pass. The existing
225-case amplification verifier also passes after the new diagnostic branch.

Unoccupied modes contribute zero to the **first** derivative of a quadratic
moment at this initial state, so pairing against occupied modes computes
the full derivative exactly. This does not assume unoccupied modes stay
zero. The direct calculation separately detects an outgoing (1,0,1) mode.

## Higher-frequency tails do not rescue the universal claim

There is also a written analytic extension beyond zero tails. Replace the
two 1/2 amplitudes by theta. At zero tail,

\[
 Q(\theta)=\frac{2(1+\theta^2)(1+4\theta^2)}{(1+2\theta^2)^2},
 \qquad \partial_\theta Q(1/2)=8/27\ne0.
\]

In the divergence-free periodic H4 space, Q is continuously differentiable
near this field. B and the full vector-field derivative Q' are continuous
there: F(u) lies in H2, which suffices for differentiating these quadratic
moments. The implicit-function theorem therefore supplies theta(w) near
1/2 for every sufficiently small divergence-free H4 perturbation w, keeping
Q=20/9. The strict inequalities B>0, K<3 and Q'>0 persist by continuity.

In particular, w can be smooth and supported at arbitrarily high Fourier
frequencies, provided its H4 norm is sufficiently small. This establishes
outward states with such tails, not just the finite-support witness.
The functional-analytic argument is written mathematics, not a RAD kernel
formalization; the derivative 8/27 is independently checked symbolically.

## Research decision

Retire this region. Do not attempt to prove its false boundary inequalities
or use additional passing samples as a repair. A stronger region would need
new justified restrictions and a new invariance argument.

The next viable construction must supply a quantitative transfer estimate
between successive frequency scales, a bound on viscous loss over that
transfer time, and control of the phase correlations and generated modes
needed to repeat the estimate. A finite-mode growth calculation alone does
not meet that gate. No such repeatable transfer estimate is proved here.
