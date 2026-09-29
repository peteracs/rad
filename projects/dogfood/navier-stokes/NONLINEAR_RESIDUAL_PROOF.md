# Nonlinear residual stability: proof and RAD audit

2026-09-09. This proves the general stability inequality below and audits its
differential algebra in RAD. It does **not** establish the supplied construction's
finite-prefix residual estimates: its actual native wave estimates and their
preservation have not been independently proved here. No Lean execution is used.

## The exact physical identity

On an open spacetime set, let u,w be smooth vector fields and p,r smooth scalar
fields. For fixed viscosity nu>0 define

    R(u,p) = u_t - nu Delta u + (u . grad)u + grad p.

Expanding this operator, with no PDE assumption on either field, gives

    R(u+w,p+r)-R(u,p)
      = w_t - nu Delta w + grad r
        + (u . grad)w + (w . grad)u + (w . grad)w.                 (1)

Both cross terms are necessary. Pressure is an actual scalar field in this
identity; the computation does not select a pressure or bound its nonlocal solve.

## All-order bound on actual derivatives

Use the maximum product norm on spacetime R x R^3, Euclidean norms on the target
spaces, and operator norms on iterated Frechet derivatives. At a point z suppose

    ||D^k u(z)|| <= A   (0 <= k <= m+1),
    ||D^k w(z)|| <= W   (0 <= k <= m+2),
    ||D^k r(z)|| <= P   (0 <= k <= m+1),

where A,W,P are nonnegative. Then

    ||D^m [R(u+w,p+r)-R(u,p)](z)||
      <= (1+3nu)W + 3P + 2^m(2AW+W^2).                         (2)

**Proof.** Time evaluation has operator norm at most one. Each of the three
coordinate contractions making up the spatial Laplacian has norm at most one,
so its operator norm is at most three. The spatial gradient has norm at most
three by its three coordinate summands. Thus the first three terms of (1) cost
at most W, 3nu W, and 3P after m derivatives.

The evaluation map (L,v) -> L(v), with L a spatial derivative, is bilinear of
norm at most one. Its m-th derivative is the sum over all subsets of the m
derivative directions, assigning one subset to L and the complement to v.
There are 2^m subsets. Each term for (u . grad)w is bounded by AW, each term
for (w . grad)u by AW, and each self-interaction term by W^2. This remains true
for arbitrary unit derivative directions, so it bounds the multilinear operator
norm, not just coordinate derivatives. Summing proves (2) for every m>=0. QED.

The constant depends on m and viscosity, but not on a correction-stage index.
No finite-mode truncation, neglected tail, divergence-free assumption, or
prescribed derivative substituted for an actual solution is involved in (2).
This is a residual estimate; it is not an evolution-existence theorem.

## Consequence for a flat tail

Suppose 0<q<=1 and, for a fixed m, the actual jets obey

    A <= C_A q^(-a),  W <= C_W q^b,  P <= C_P q^c,

with a,b>=0. Formula (2) yields

    ||D^m delta R|| <= C q^min(b-a,c),
    C=(1+3nu)C_W+3C_P+2^m(2C_A C_W+C_W^2).                    (3)

Indeed b>=b-a and 2b>=b-a, so each velocity contribution has at least the
stated exponent. In particular, if all background derivatives have fixed
power growth and all velocity/pressure tail derivatives decay faster than
every power of q, every derivative of the actual nonlinear residual difference
also decays faster than every power. For a desired exponent N, choose the
velocity-tail exponent b>=N+a and pressure-tail exponent c>=N before applying
(3). This handles the nonlinear cross terms without requiring bounded background
velocity. It requires actual all-order tail bounds, not independent interior
corrections labeled as a tail.

## Relation to the supplied construction

The source's `ResidualStability.residualDifference_jet_bound` has this structure,
with abstract fixed operator norms. The estimate needed for the finite prefixes
is stronger and includes cancellation of the old residual:

    ||D^m R(U_J,P_J)|| <= C_(J,m) q^(h J/10-L_m),
    L_m=m(m+2)+5/2+2h+2hm.                                    (4)

The formula for L_m follows from `PhysicalResidualJetBounds.physicalLoss`,
`PhysicalMeanJetBounds.loss`, `PhysicalGraphBounds.graphLoss=m(m+2)`,
`CoordinateAlgebra.A=1/2+h`, and the source's phase-loss coefficient beta=2h.
It is independent of J. If (4) holds, then for any fixed m and target power N,
choosing J>10(N+L_m)/h makes its exponent exceed N. Constants and neighborhoods
may depend on J,m; this does not by itself justify an infinite sum or a uniform
bound for all stages. Those require the source's simultaneous diagonal schedule.

The source's proof route for (4) is explicit:

1. `ActualCyclePreservation.state_runInvariant` uses induction on the actual
   correction state, with `next_runInvariant` increasing sigma by 1/10.
2. `ActualCycleResidualBounds.native_residual` reconstructs the complete
   residual from oscillatory, mean, base, Gaussian, and alias contributions.
3. `selected_residual_jetRate` transfers the native bounds through the actual
   polar charts, and uses equality on neighborhoods with the base fields
   outside the active set. This includes chart and radial-boundary coverage.
4. `finite_residual_rates` obtains (4) from those invariant and realization
   results, weakening an extra positive 7h/10 in the exponent.

The unverified step in this RAD port is **not** the arithmetic hJ/10-L_m.
It is supplying the actual correction output and its native bounds to the
inductive step. In particular, bounding delta R by its absolute magnitude in
(2) does not prove that it cancels R(U_J,P_J). The inhomogeneous principal solve,
signed covariance update, mean update, and their cross terms must accomplish
that cancellation while preserving the weighted derivative classes.
The source claims to construct them; this port has not independently established
those claims. Promoting the source theorem names to RAD boolean assertions would
not prove (4).

## RAD implementation and validation

`residual_jets.rad` adds a bounded free differential-polynomial algebra for
spacetime jets. It expands the physical residual directly, differentiates by
the product rule, and combines equal monomials. It is research tooling, not
a foundational continuum proof kernel. Field derivatives are formal symbols;
the all-order justification for applying the algebra to smooth functions is
the proof above.

`nonlinear_residual_audit.rad` checks all 35 spacetime multiindices of total
degree at most three, across all three velocity components (105 identities).
Every case compares differentiated R(u+w,p+r)-R(u,p) to the six-term expression
in (1). It also checks the complete Leibniz coefficient sum 2^m. These are
symbolic identities, not sampled velocity states. Higher orders in (2) follow
from the subset proof, not extrapolation from these finite checks.

The verifier checks an independent nonlinear polynomial substitution with
mixed time and spatial derivatives, rejects deletion of a cross term, rejects
a forged actual-stage estimate, and checks one/four-worker determinism and
record/replay. All pass. It writes `nonlinear_residual_audit.json` and the WHY
receipt. There are no running background jobs from this audit.

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python projects/dogfood/navier-stokes/verify_nonlinear_residual_audit.py
```

Result: (1)–(3) are justified; the construction-specific bound (4), its diagonal
limit, smooth-force extension, and blowup remain unverified in this RAD work.
