# Moving radial cutoffs do not repair the triaxial affine core

This is a written analytic deduction for the explicit family below, not a
general Navier–Stokes theorem. The finite polynomial instances are checked
in RAD. No novelty or independent peer verification is asserted.

Let A=diag(1,2,-3), y=x/R(t), q=|y|^2, Q=y dot Ay, and H=y cross Ay.
Let S(t) and R(t)>0 be smooth before T. At every time choose a smooth
radial function chi(t,q), equal to 1 for q<=1/4 and 0 for q>=1. Its shape
may otherwise change arbitrarily in time; monotonicity is not required.
Define

    V(t,y) = curl_y[-chi(t,q) (y cross Ay)/3],
    u(t,x) = S(t) R(t) V(t,x/R(t)).

This family is divergence-free, has the affine core u=S(t)Ax, and is
compactly supported at each time. Suppose u solves the forced equation

    u_t + (u dot grad)u - nu Delta u + grad p = f.

Then, with the maximum taken over space and the three Cartesian components,

    max_i ||(curl f)_i(t)||_infinity >= (140/729) S(t)^2.

Consequently unbounded strain S in this family is incompatible with a force
having uniformly bounded first spatial derivatives through T. This includes
the smooth forces admissible in the [Clay problem, conditions (5) and (9)](https://www.claymath.org/wp-content/uploads/2022/06/navierstokes.pdf).
The bound is independent
of the support radius and of viscosity. It also excludes a nonzero member
of this family as an exact unforced solution, even without blowup.

## The angular component that time variation cannot cancel

Primes below differentiate q, with t held fixed. Direct differentiation gives

    V = a Ay + b Q y,
    a = chi + (2q/3)chi',   b = -(2/3)chi',
    curl_y V = c H,   c = 2(a'-b) = (14/3)chi' + (4q/3)chi''.

Each component of H is a harmonic homogeneous polynomial of degree two.
Therefore Delta_y(cH)=(4q c''+14c')H. Time differentiation of
curl_x u=S c(t,q)H, including the changing R, is also a scalar multiple
of H. These two contributions cannot produce a component along AH
independent of H.

Since div V=0, the vorticity transport identity gives

    curl_y[(V dot grad_y)V] = d Q H - 2ac AH,
    d = 2c'(a+bq)+bc.

To verify this, H is perpendicular to both y and Ay, and
y cross A^2 y = -AH because tr A=0. Expanding
(V dot grad)(cH) - (cH dot grad)V gives the displayed expression.
In physical variables the nonlinear curl has the prefactor S^2; all
radius factors cancel.

Choose y=sqrt(q/3)(1,1,1). Then H=(q/3)(-5,4,1), and the linear
functional L(F)=4F_1+5F_2 annihilates H, but L(AH)=20q/3. Pressure
has zero curl. Thus the full force, including all time and viscous terms,
satisfies the exact identity

    L(curl_x f) = -(40/3) q S^2 a(q)c(q).

This projection eliminates arbitrary time variation of the radial cutoff,
not just the self-similar dilation considered in the earlier calculation.

## A uniform lower bound across all such cutoffs

Put epsilon=max_{q in [1/4,1]} |a(q)c(q)|. Since a(1/4)=1 and a(1)=0,
let q_* be the first point where a=1/2. On [1/4,q_*], a>=1/2,
so |c|<=2 epsilon. The formula for c implies

    (q^(7/2) chi')' = (3/4) q^(5/2)c.

Using chi'(1/4)=0, integration yields |chi'|<=3 epsilon/7 on that
interval. Also a'=c/2-(2/3)chi', so |a'|<=9 epsilon/7. It follows that

    1/2 <= (9 epsilon/7)(q_*-1/4) <= 27 epsilon/28,
    epsilon >= 14/27.

At a point maximizing |ac|, q>=1/4. If
M=max_i ||(curl f)_i||_infinity, the angular identity gives

    9M >= |L(curl f)| >= (10/3) S^2 epsilon,
    M >= (10/27)(14/27) S^2 = (140/729) S^2.

All estimates are at one time. No uniform bounds on derivatives of chi
were assumed to obtain this lower bound.

## What this tells the construction search

Allowing only S, R, and a radial cutoff to change is insufficient. A viable
correction must introduce additional angular velocity structure (or depart
from this affine core family) so it can cancel the AH contribution. An
outer flow could supply such structure, but must solve its own coupled
equations and preserve the required forcing bounds. This argument neither
constructs that correction nor excludes nonradial or general changing flows.

## RAD evidence and its boundary

`cutoff_polynomial.rad` implements bounded exact integer polynomial
addition, multiplication, differentiation, curl, and evaluation. It evaluates
chi=(1-q)^m for m=0,1,2 in separate forks, checks solenoidality, and compares
the angular projections of linear, nonlinear, and viscous terms. These
polynomials represent local collar jets; they are not globally smooth
compact cutoffs. m=0 is a control, not a blowup candidate.

The two nonconstant examples have nonzero nonlinear projections while the
linear and viscous projections vanish. The resolver recomputes each
polynomial result, rejects repeated cases, and connects the aggregate to
the proposals via `why()`. A separate SymPy calculation compares the
finite values. These computations do not formally check the general
time-dependent identities, the integration argument, or the continuum PDE.

`verify_angular_identity.py` also checks the divergence, curl, nonlinear,
and diffusion identities symbolically with an arbitrary radial function
chi, rather than substituting particular polynomial examples. That is an
additional external algebra check, not a formal proof of the lower-bound
integration or the PDE interpretation.
