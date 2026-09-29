# Exposing return-map unknowns by solving for an error metric

This application solves one previously unspecified design variable: a weighted
error norm for the known coupled amplitude equations. It then exposes the
remaining return-map quantities as named proof obligations. It does not
coerce RAD into accepting an unproved Navier-Stokes statement.

## The inverse calculation finds a metric

From `COUPLED_EVOLUTION.md`, the known projected vector field is

    G_a=-(3/2)a-(39/20)ab+b^2/2304,
    G_b=2048a^2-(485/117)b-(160/351)ab.

The actual amplitude equations also have prescribed force F_a(t) and bounded
remainders R_a,R_b. First consider the Jacobian J=DG of G alone:

    J11=-3/2-39b/20,        J12=-39a/20+b/1152,
    J21=4096a-160b/351,     J22=-485/117-160a/351.

At (a,b)=(1,0), the Euclidean error direction (1,1) has positive instantaneous
quadratic growth. That does not establish instability in every metric.
Instead of guessing weights, choose P=diag(p,q) and solve for cancellation
of the large a-dependent off-diagonal term:

    (39/20)p=4096q.

Taking q=117/5 gives p=49152. The remaining off-diagonal of PJ+J^T P is
exactly 32b. On the certified rectangle 3/4<=a<=5/4, 0<=b<=1/16,

    J11<=-3/2, J22<-4, |32b|<=2.

For any real vector h, these imply

    h^T(PJ+J^T P+2P)h
      <= -p h_1^2 +4|h_1 h_2|-6q h_2^2 <=0,

because p*6q>4. Therefore

    PJ+J^T P <= -2P

uniformly on the rectangle. The rectangle is convex, so the same bound,
integrated along line segments between states, proves contraction of the
two-dimensional G flow in the norm ||h||_P=sqrt(h^TPh), as long as both
solutions stay in the rectangle. A common time-dependent prescribed force
cancels in their difference. The norm decay rate is at least 1.

This is an actual finite-dimensional result, not a chosen value of rho.
It does not yet apply to the full fluid because the remainder depends on
all the other modes and can differ between solutions.

## A finite error bound for the actual projected trajectory

Let x=(a,b) be the actual projection and let x_bar solve
x_bar'=G(x_bar)+(F_a(t),0) with the same initial coordinates (1,0).
Both stay in the rectangle through T=2^-98. For x this was established
previously. For x_bar, bounds |a_bar'|<5 and |b_bar'|<4000 on the rectangle,
and b_bar'>0 at b_bar=0, prevent exit on this tiny interval by a standard
first-exit argument.

The already proved actual remainder bound is |R_a|,|R_b|<=2^61 t.
Since sqrt(p+q)<256, ||R||_P<=2^69 t. The contraction inequality gives

    ||x(t)-x_bar(t)||_P
      <= integral_0^t exp(-(t-s)) 2^69 s ds <=2^68 t^2.

This is a new finite shadowing estimate for the actual projected trajectory.
It does not justify setting the omitted modes to zero in the fluid or
assuming their effect is identical for different entrance states.

## The unknowns are now explicit proof obligations

The application emits the following named quantities with status `unproved`:

| Symbol | What must actually be bounded |
| --- | --- |
| Gamma | An error-decay estimate for the full velocity perturbation, including arbitrary tails, valid on the admissible profile family at every scale. The projected rate 1 is not this estimate. |
| eta | The propagated defect of the chosen approximate trajectory under the fixed force, integrated in the full normalized error norm. |
| kappa | The error amplification caused by changing from the endpoint coordinates to the next profile's coordinates, retaining the original flow and tails. |
| zeta | The mismatch between the evolved central profile and the next central profile. |
| theta0, L | The actual band-energy retention bound theta0-L e for that same solution map. |
| all-stage inclusion | Initial membership, stage existence, admissible fixed forcing and applicability of the estimates at every scale. |

For example, if the full error E satisfies E'<=-Gamma E+h, then

    eta=integral_0^tau exp(-Gamma(tau-s)) h(s) ds.

An endpoint coordinate change with Lipschitz constant kappa and central
mismatch zeta would give

    e_next <= rho e+d,
    rho=kappa exp(-Gamma tau), d=kappa eta+zeta.

Combined with the previous repetition criterion, one sufficient design gate is

    rho<1,
    L*(kappa eta+zeta)<(1-rho)*(theta0-lambda^-2).

These formulas expose which missing estimates determine closure. They are
not values derived for the Navier-Stokes return map. A direct bound on the
composite normalized solution-map derivative may be sharper than the product
of a flow bound and a coordinate-change bound; the factorized gate is only
one sufficient approach.

In particular, dividing a newborn band by a small power of time can amplify
errors during normalization. A contracting projected flow in physical
coordinates does not automatically yield a contracting normalized map.

## How to obtain evidence for the full-error obligation

For a proposed divergence-free central trajectory U under the prescribed
force, form its actual residual

    r=U_t-Delta U-N(U)-f.

The true error obeys e_t=Delta e+N(U+e)-N(U)-r. A full-error proof must
control this equation, including its quadratic term and all Fourier tails.
The finite metric above suggests a possible starting block for an error
norm, but provides no tail blocks or uniform full-operator estimate.
Constructing and proving those missing blocks is substantive PDE work.

This separates a solved algebraic unknown (the donor/receiver metric ratio)
from unresolved analytic ones. The earlier arbitrary contraction number
has not been silently promoted to a theorem for the fluid.

## RAD behavior and validation

`return_map_holes.rad` solves the metric equation in exact arithmetic,
checks the uniform comparison form and the finite remainder constant, then
returns the open-bound ledger alongside the closure formula. WHY shows the
resolver and law that checked that evidence. The ledger is explicit data;
WHY is not a symbolic theorem oracle and has not inferred values for holes.

`verify_return_map_holes.py` independently differentiates G, verifies the
matrix identity, the comparison determinant, the Euclidean expansion witness,
the rational remainder arithmetic and the proof-status rejection tests.
The continuum implications are written analytic lemmas, not formalized in
the RAD kernel. No missing bound is treated as an axiom.

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_return_map_holes.py
```

The full return map and the Millennium problem remain unproved.
