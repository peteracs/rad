# What converges, and what has not been proved about blowup

For the globally localized initial velocity specified in
GLOBAL_PRESSURE_MATCHING.md, the integral equation has a convergent
Picard iteration on an explicit positive interval

    0 <= t <= T,   T = nu * 2^(-90).

This is a deliberately conservative short-time bound. It neither implies
blowup afterward nor implies that the solution ceases to exist at T.
The argument is a quantitative instance of classical local well-posedness,
not a new resolution of the Millennium problem. No valid blowup mechanism
for this initial datum has been established in this project.

The functional analytic argument is written below. RAD checks the finite
arithmetic supporting its norm bound, contraction factor, and error rate;
it does not evaluate the spatial Picard iterates or formalize the PDE proof.

## Why more Taylor coefficients do not establish convergence

The chosen cutoff is C-infinity, compactly supported, and nonzero. For
such a datum v, even the linear heat Taylor series

    sum_{n>=0} (nu t)^n Delta^n v / n!

has zero radius of convergence in L2. To see this, suppose its radius
were positive. The norms of its coefficients would give, for some a>0,

    sum_{n>=0} a^n ||Delta^n v||_2 / n! < infinity.

In Fourier space this bounds the partial sums of
exp(a|xi|^2) v_hat in L2, and Fatou's lemma gives
exp(a|xi|^2) v_hat in L2. Fourier inversion and Cauchy–Schwarz then
extend v to an entire function of the spatial coordinates: the Gaussian
weight dominates exp(b|xi|) for every finite b. A nonzero entire function
cannot have compact support. This is a contradiction.

This heat-equation argument does not itself prove divergence of the
nonlinear Navier–Stokes Taylor series, whose coefficients also contain
nonlinear terms. It establishes why smoothness and finitely many checked
coefficients cannot justify that series. A heat-semigroup iteration avoids
requiring analyticity at the initial time.

## Use the globally projected integral equation

Let P be the Leray projection from GLOBAL_PRESSURE_MATCHING.md. Seek

    u(t)=exp(nu t Delta)U_0
          - integral_0^t exp(nu(t-s)Delta) P div(u tensor u)(s) ds.  (1)

Pressure is recovered by the global Riesz-transform formula, so this
does not reinstate the arbitrary local polynomial pressure. The general
method and its Euclidean-space formulation are described in
[Tao's local well-posedness notes, Section 4](https://terrytao.wordpress.com/2018/09/16/254a-notes-1-local-well-posedness-of-the-navier-stokes-equations/).
The normalization and explicit constants here are derived independently.

Use the unitary Fourier transform with kernel exp(-ix dot xi) and norm

    ||v||_H2 = ||(1-Delta)v||_2
              = ||(1+|xi|^2) v_hat||_2.

Vector and tensor norms use Euclidean/Frobenius norms of their entries.
For w(xi)=1+|xi|^2, w(eta+zeta)<=2[w(eta)+w(zeta)]. Weighted Young
and Cauchy–Schwarz inequalities, together with

    integral_R3 (1+|xi|^2)^(-2) dxi = pi^2,

give

    ||u tensor v||_H2 <= [4pi/(2pi)^(3/2)] ||u||_H2 ||v||_H2
                       <= ||u||_H2 ||v||_H2.                       (2)

The last inequality uses pi>2. The heat multiplier and the orthogonal
projection give

    ||exp(nu t Delta)P div F||_H2 <= (nu t)^(-1/2) ||F||_H2.       (3)

Indeed sup_{r>=0} r exp(-nu t r^2)=(2e nu t)^(-1/2), which is smaller
than the constant used in (3). Thus on X=C([0,T];H2), the bilinear
integral B in (1) satisfies

    ||B(u,v)||_X <= K ||u||_X ||v||_X,  K=2 sqrt(T/nu).            (4)

It remains to supply an actual upper bound for ||U_0||_H2.

## A conservative bound for the specified smooth cutoff

Keep the exact cutoff from GLOBAL_PRESSURE_MATCHING.md:

    chi(q)=[1-q S_(1/4,3/8)(q)] [1-S_(5/8,1)(q)].

Here S is built from rho(t)=exp(-1/t) for t>0, zero otherwise. On [0,1],
the bounds t^(-k)exp(-1/t)<=k! yield bounds

    |rho|<=1, |rho'|<=2, |rho''|<=36, |rho'''|<=1584.

For d(t)=rho(t)+rho(1-t), one argument is at least 1/2, so
d>=exp(-2)>1/8. Also |d'|<=4, |d''|<=72, |d'''|<=3168. Differentiating
1/d gives the bounds 8,256,20992,2660352 at orders zero through three.
Leibniz's rule applied to S=rho/d therefore bounds its first three
derivatives by 272,22304,2826624. These bounds extend across the endpoints
because the smooth step is flat there.

Both rescaled transition intervals have inverse width at most 8. Hence
their derivative bounds are

    B_1=2176, B_2=1427456, B_3=1447231488.

Throughout 0<=q<=1, |1-qS_inner|<=1 and |1-S_outer|<=1. Applying the
product rule to chi gives

    |chi|<=1,
    K_1=1+2B_1=4353,
    K_2=4B_1+2B_2+2B_1^2=12333568,
    K_3=6B_2+2B_3+6B_1^2+6B_1B_2=21568303104,

where |chi^(j)|<=K_j. Outside this interval the relevant derivatives vanish.

Write U_0=aAx+bQx as before. Direct differentiation gives

    Delta U_0=alpha Ax+beta Qx,
    alpha=4q a''+10a'+4b,  beta=4q b''+18b'.

Using a=chi+(2q/3)chi' and b=-(2/3)chi', the derivative bounds imply

    |a-alpha|+|b-beta| <= L,
    L=1+26K_1+40K_2+8K_3=173039880731.

On the unit ball, |Ax|<=3 and |Qx|<=3. Since U_0 is supported there,

    ||U_0||_H2^2 <= (4pi/3)*9L^2 < 49L^2,
    ||U_0||_H2 <= 7L = 1211279165117 < 2^41.                    (5)

The estimate is intentionally loose but analytic; it does not depend on
numerical quadrature of a derivative norm.

## Convergence and an explicit error bound

Set M=2^41 and T=nu/(256M^2)=nu*2^(-90). Then KM=1/8. In the closed
ball ||u||_X<=2M, the right side Phi of (1) obeys

    ||Phi(u)||_X <= M+4KM^2 <= 2M,
    ||Phi(u)-Phi(v)||_X <= 4KM ||u-v||_X <= (1/2)||u-v||_X.

It preserves divergence-free fields. The contraction theorem therefore
gives a unique fixed point in that ball. Starting from the heat evolution
v_0(t)=exp(nu t Delta)U_0 and iterating v_(n+1)=Phi(v_n),

    ||v_1-v_0||_X <= KM^2 <= M/8,
    ||u-v_n||_X <= (M/4)2^(-n) = 2^(39-n).                    (6)

These are bounds on the mathematical Picard iterates, not errors of a
spatial discretization or of computed floating-point PDE trajectories.
Standard persistence of regularity for the mild solution with smooth
initial data yields the local smooth solution and its pressure. The
classical local energy identity holds on this interval.

## Blowup is still an open obligation for this construction

The argument proves regularity for a short interval. Failure of the same
contraction estimate at a later time does not imply a singularity. Nor does
the nonzero residual of a truncated jet say that the exact solution blows up.

An upper differential estimate such as X'<=C X^3 is also insufficient:
the constant function X=1 satisfies such an inequality forever. A comparison
ODE with finite-time blowup supplies no blowup lower bound for X.

To claim a singularity we would need a dynamical lower bound that survives
all coupled angular modes, the global pressure, viscosity, and the exterior
flow, along with control of the solution up to the claimed singular time.
None has been established. The data constructed here might remain smooth
globally. The calculations do not decide that question.

## Executed checks

`local_convergence.rad` recomputes the integer derivative and norm bounds,
checks twelve norm/time/error certificates in isolated forks, and records
their causal ancestry. `verify_local_convergence.py` independently recomputes
the product-rule bounds and rational contraction inequalities, rejects
insufficient norm bounds and excessive timesteps, compares worker counts,
and verifies replay. The analytic proof, Sobolev inequalities, and infinite
Picard limit remain outside the RAD kernel.
