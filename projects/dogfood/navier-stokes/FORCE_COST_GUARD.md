# Force cost that cannot be removed by changing the interior blend

`force_cost_why.rad` computes a shape-independent necessary derivative cost
from the actual force endpoint data. It charges the **total physical force**,
including any part assigned to a smooth background. WHY traces the boundary
data, derivative requirement, and accounting check.

This prevents hiding a cost in another interval or reference-force column.
It does not make an unavoidable cost disappear or supply a blowup mechanism.

## Exact lower bound

Take one normalized Fourier coefficient of one force component and call its
imaginary part phi(t). Since Fourier coefficient magnitude is at most the
spatial supremum norm, a lower bound on a derivative of phi is also a lower
bound on the corresponding time derivative norm of the force.

For a stage [a,a+delta], Taylor's integral remainder gives, for every m>=1,

\[
 R_m=\phi(a+\delta)-\sum_{j=0}^{m-1}\frac{\delta^j}{j!}\phi^{(j)}(a),
 \qquad
 \|\partial_t^m f\|_\infty\ge\frac{m!}{\delta^m}|R_m|.
\]

Indeed, R_m is the integral of phi^(m)(a+s) against
(delta-s)^(m-1)/(m-1)!, whose integral is delta^m/m!.
The argument applies to any C^m connecting path with those boundary data,
not only a chosen polynomial or switching profile. The analytic theorem
is written mathematics; RAD checks its exact arithmetic instances.

## The actual repaired fixture

The previous endpoint/join repair preserves a stationary entrance germ of
the predecessor. Every positive time derivative of its entrance force is
therefore zero. The full Fourier operator computes

    phi(a)=1/2, phi(a+delta)=0

at k=(1,0,1), component y. At the implemented unit duration this is an
actual trajectory witness. For a proposed transition with the same force
boundary data compressed to any other duration, the necessary bound is

\[
 \|\partial_t^m f\|_\infty\ge\frac{m!}{2\delta^m}.
\]

The other duration rows are boundary feasibility tests, not assertions
that a Navier–Stokes trajectory at each duration has been constructed.

Changing the blend cannot reduce this lower bound while preserving the
stated force endpoint data. A sequence retaining this nonzero change as
delta tends to zero cannot have a uniformly bounded first time derivative,
let alone an admissible smooth force at its accumulation time. This excludes
that proposed repeated transition, not smooth-forced blowup in general.

## The compatibility condition the generator should enforce

If the proposed m-th derivative budget is M_m, the allowed Taylor mismatch is

\[
 |R_m|\le\frac{M_m\delta^m}{m!}.
\]

For constant entrance force germs, R_m is the force coefficient change.
For nonconstant entrance germs, the initial derivative terms must be
included; requiring every force change to be super-algebraically small
would otherwise impose an unwarranted restriction. The checker includes
a nonconstant quadratic-force example to exercise this distinction.

The application reports the exact maximal force change compatible with an
illustrative unit derivative cap in the constant-germ case. That cap is a
diagnostic request, **not a Clay requirement**. For m=4 and delta=1/16,
the allowable change under that cap is at most 1/1572864; the requested
change of 1/2 violates it by a factor of 786432.

Passing this necessary endpoint test does not establish a sufficient bound
inside the interval. A force can satisfy compatible endpoint data and still
have large interior derivatives.

## Background forcing cannot hide physical cost

The app tests two allocations of exactly the same endpoint change:

    background change = 0, residual change = -1/2;
    background change = -1/2, residual change = 0.

Both have the same total-force Taylor remainder and lower bound. A forged
certificate that drops the background contribution is rejected. This does
not prohibit using a smooth background; it requires its real derivatives
to be included in the total forcing budget. Merely renaming a term does
not alter the physical trajectory's force.

## What this resolves, and what it does not

The generator can reject incompatible force boundary transitions before
searching over their interior shape. It can choose a globally compatible
smooth force schedule, then use the earlier endpoint solver to derive the
required velocity derivatives instead of selecting them independently.

However, a chosen admissible force does not guarantee that a proposed
velocity trajectory solves Navier–Stokes with that force. The full residual
must still equal it throughout every interval, and amplification must be
proved. The repeated-stage mechanism remains missing.

## Validation

`verify_force_cost.py` checks:

- Actual full-force entrance/exit data against independent complex algebra.
- Forty exact constant-germ derivative costs and three nonconstant-germ costs.
- The Taylor integration kernel for orders 1 through 8.
- Eight causal accounting cases, including unchanged costs under relabeling.
- Rejection of hidden background cost, deterministic worker output and replay.

All checks pass. The generic integer routine `unavoidable_cost_n` supports
orders 1..8, inverse durations 1..16, and checked rational endpoint/jet
numerators. It is a bounded computational implementation of the written
all-order inequality, not a kernel formalization of that inequality.

Run with SymPy available:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_force_cost.py
```

The decision explicitly retains `whole_interval_bound_proved: false` and
`blowup_impossibility_proved: false`.
