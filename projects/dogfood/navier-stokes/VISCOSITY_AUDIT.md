# Viscosity audit and attempted transfers

## 1. Prize target and the force endpoint

Use the actual three-dimensional incompressible equation

    u_t + (u · grad)u + grad p = nu Delta u + f, div u = 0, nu > 0.

Clay alternative C permits smooth force on R^3, with rapid spatial and
temporal decay of all derivatives, and smooth rapidly decaying initial data.
Alternative D uses periodic space. A force smooth on a finite closed interval
with fixed compact spatial support can be extended smoothly past the endpoint
and multiplied by a later smooth temporal cutoff. This requires a smooth
extension argument; simply assigning zero at and after the endpoint is invalid
unless every endpoint jet vanishes. Such extension preserves the old solution
on its preterminal interval.

The supplied Lean `SmoothForce` bounds all joint derivatives on [0,T).
These bounds provide endpoint limits: each derivative is uniformly Cauchy in
time because its next time derivative is uniformly bounded. Compatibility of
the limits supplies a smooth one-sided extension, with the same compact support.
A standard smooth extension across a half-space boundary, followed by a time
cutoff, supplies future values. The original Lean function's arbitrary value
at T need not be preserved. This is a mathematical extension argument, not a
Lean proof produced by this project. The paper's Lemma 13.2 uses endpoint
limits for its actual force series.

Thus the half-open statement is not the primary obstruction to a prize result.

## 2. A bounded-velocity obstruction

Theorem 1.1 bounds Gamma = r u_phi and both meridional velocity components.
The swirl is supported away from r=0. Hence u_phi = Gamma/r is bounded, and
the entire velocity has a uniform spatial supremum bound before T.
Lemma 13.1 and Section 13.2 establish this for the complete corrected fields,
including their cutoff regions, not just the leading profile.

Proposition (standard continuation consequence). A classical finite-energy
Navier–Stokes solution with smooth compactly supported initial data and
admissible smooth force cannot develop a finite-time singularity while
remaining bounded in L-infinity in space and time.

An energy estimate makes the obstruction explicit. Let

    X = ||grad u||_2^2, Y = ||Delta u||_2^2,
    U = ||u||_infinity, F = ||f||_2.

For a smooth decaying solution, test the equation against -Delta u. The
pressure term disappears by incompressibility. Cauchy–Schwarz gives

    (1/2) X' + nu Y <= U sqrt(X) sqrt(Y) + F sqrt(Y).

Apply ab <= (nu/4)b^2 + a^2/nu separately to the two right-hand terms:

    X' + nu Y <= (2/nu) U^2 X + (2/nu) F^2.

Bounded U on a finite interval and square-integrable F give a uniform H^1
bound by Gronwall, together with the ordinary L^2 energy estimate. The
standard local strong-solution continuation theorem in H^1 then extends
the solution through T. Equivalently, the L^2_t L-infinity_x regularity
criterion excludes blowup. The integration and continuation facts are
analytic inputs, not checked by `frozen_mode.rad`.

Consequences for three attempted transfers:

1. **Keep the Euler velocity and set f_NS = f_E - nu Delta u.** This solves
   the NS equation algebraically with the original pressure. It cannot give
   a Clay-admissible force for this singular bounded velocity. Otherwise the
   continuation proposition gives a contradiction. This argument avoids
   claiming that vorticity blowup alone implies a particular Laplacian norm
   blows up; that implication would require additional analysis.
2. **Keep all boundedness estimates while changing corrections.** Also
   impossible if the resulting force is admissible and viscosity positive.
3. **Apply one fixed space/time dilation.** Finite constant dilation preserves
   bounded velocity. It cannot fix the first obstruction or turn positive
   viscosity into zero. With u(x,t)=a U(bx,ab t), effective viscosity for U
   is nu b/a, positive for fixed positive a,b.

This rules out these transfers, not all improvements of the research program.

There is a separate geometric obstruction under the usual suitable
finite-energy solution framework: an axisymmetric Navier–Stokes singularity
cannot occur at positive distance from the symmetry axis. Rotation would
turn one off-axis singular point into a whole spatial circle of singular
points at the same time. That circle has positive one-dimensional parabolic
Hausdorff measure, contradicting the zero-measure conclusion of partial
regularity. Smooth admissible force is compatible with that partial
regularity theorem. One uses a suitable weak continuation and strong/weak
uniqueness to identify it with the classical solution before its first
singular time.

Consequently, abandoning the bounded-velocity estimate alone is insufficient
if the singularity still forms on the paper's fixed off-axis ring. An
axisymmetric attack must involve the axis; alternatively it must break
axisymmetry. Moving toward the axis also invalidates the paper's uniform
bounds for reciprocal radial coefficients and demands new axis-compatible
smoothness conditions. This is not achieved by setting its radius to zero.

The zero-measure statement with smooth forcing is described on pages 3–4
of Clay's official statement. For the explicit axisymmetry consequence see
the introduction of Seregin, *A Note on Local Regularity of Axisymmetric
Solutions to the Navier–Stokes Equations* (2022):
https://link.springer.com/article/10.1007/s00021-022-00667-6

Reference for continuation/regularity context:
https://terrytao.wordpress.com/2018/09/16/254a-notes-1-local-well-posedness-of-the-navier-stokes-equations/
https://terrytao.wordpress.com/2019/08/15/quantitative-bounds-for-critically-bounded-solutions-to-the-navier-stokes-equations/
The latter displays the unforced criteria; the estimate above retains force.

## 3. Exact diffusion operators in the paper's coordinates

Use physical, unmagniﬁed coordinates y1=z, y2=r^2/2, and

    Gamma = r u_phi, xi = omega_phi/r, v = (u_z,r u_r),
    D_v = partial_t + v · grad_y, W = (2y2)^(-2).

The axisymmetric NS equations become

    D_v Gamma = nu D_Gamma Gamma + r f_phi,
    D_v xi = W partial_y1(Gamma^2) + nu D_xi xi
             + (partial_z f_r - partial_r f_z)/r,

where

    D_Gamma = partial_y1^2 + 2y2 partial_y2^2,
    D_xi    = partial_y1^2 + 2y2 partial_y2^2 + 4 partial_y2.

Derivation: partial_r = r partial_y2 and
partial_r^2 = partial_y2 + r^2 partial_y2^2. Circulation diffuses by
partial_z^2 + partial_r^2 - r^(-1)partial_r; reduced azimuthal vorticity
diffuses by partial_z^2 + partial_r^2 + 3r^(-1)partial_r. Substitution gives
the displayed operators. These formulas include geometric first-order terms.

Both have principal positive symbol

    q_nu(y,zeta) = zeta1^2 + 2y2 zeta2^2 = 2y2 kappa,

with kappa = zeta1^2/(2y2) + zeta2^2 from the paper. Off the axis, this is
positive for nonzero zeta. The extra first derivative in D_xi is lower order
in carrier frequency, but must still be controlled in a complete argument.

## 4. A frozen Fourier-mode calculation, not a PDE proof

Freeze the older fields, material metric, and coefficients in equation (3.4).
For a single Fourier harmonic exp(i N p · a), the second-order diffusion
contributes -d to both principal diagonal entries, with

    d = nu N^2 q_nu.

On an Euler growth interval with positive off-diagonal entries b,c, the
principal two-component model is

    R' = [[-d,b],[c,-d]] R.

Its eigenvalues are -d +/- sqrt(bc). A positive eigenvalue exists precisely
when bc > d^2; equality is neutral. This is exactly the rational comparison
in `frozen_mode.rad`. A nonnormal stable matrix can still exhibit transient
norm growth; this eigenvalue test is not a claim excluding every transient.

For fixed b,c and positive nu,q_nu, sufficiently large N makes both
eigenvalues negative. Small fixed viscosity delays this threshold; it does
not remove it. When coefficients depend on the layer or time, their actual
values and integrated effects must be estimated again.

The original paper uses a smooth periodic profile F that is linear near
zero, not a single sine. It has multiple Fourier harmonics. Diffusion acts
on harmonic k with (kN)^2. Accordingly the frozen model is not the exact
viscous replacement of (3.4), and its scalar damping cannot simply be
substituted into every existing correction equation.

## 5. What must change to create a viable attack

The force recovery operator, material-coordinate bookkeeping, causal
construction, and diagonal all-derivative summability may be reusable ideas.
They do not establish the missing positive-viscosity estimates.

A candidate must supply all of the following mathematical inputs:

- unbounded physical velocity, escaping the obstruction in Section 2;
- axis involvement or broken axisymmetry, escaping the off-axis ring obstruction;
- amplification estimates for the viscous equations, including geometric
  drift, all harmonics, cutoff derivatives, and material deformation;
- a localization and matching construction compatible with finite energy;
- bounds for the actual physical force at every mixed derivative order;
- one infinite parameter sequence satisfying these bounds simultaneously;
- a limiting classical solution, valid initial data, and genuine blowup
  excluding any globally smooth continuation in Clay's class.

The useful first lemma to seek is a viscous replacement for the growth and
return mechanism that does not preserve uniform velocity boundedness.
This likely requires a new amplitude regime, spatial geometry, or both.
No such lemma is proved in this audit. Finite-mode growth alone cannot
replace it, even when the finite-mode arithmetic is exact.
