# RAD audit of the supplied Navier–Stokes construction

2026-09-09. Source: `D:/Downloads/NavierStokesAndEuler-main`.
The Lean setup was stopped at the user's request. No Lean proof was built
or checked. All execution reported below uses RAD, with Python orchestration.

## Change of direction

The supplied directory explicitly claims the forced Navier–Stokes alternatives
C and D for every positive viscosity. The top-level route is
`ComparatorSolution -> ComparatorTheorem -> ActualCandidateAssembly.selected_candidate`
for the periodic result, with `ComparatorR3Theorem -> R3ActualCandidate` for
the whole-space result. The actual candidate contains a smooth force through
time one, zero initial velocity, and unbounded speed approaching time one.
Those are source claims, not conclusions of this audit.

Static navigation finds 2,486 Lean files and 580 local modules reachable from
`NavierStokes.ComparatorSolution`. No challenge-placeholder module is in that
local import closure. File hashes and edges are in
`source_construction_imports.json`. An import scan cannot establish proof
correctness, rule out all extra assumptions, or validate definitions.

This is a more relevant candidate than transferring the local Euler manuscript:

1. `TangentProjection.projectedRhs` includes **physical damping** `-delta*v`.
   `NativePrincipalEquations` explicitly identifies delta with
   `epsilon * frequency^2 * |normal|^2`; the principal solve includes it.
2. `RadialHeatProfile`, `HeatedOutgoing`, and `PhysicalHeatCoordinates` build
   a radial heat continuation and match it to the outer angular velocity.
   Three compensation bumps restore the modified moments. Their construction
   and all differentiated bounds are still to be independently audited.
3. The correction cycle improves **residual order**, not necessarily the energy
   fraction in a receiving Fourier band. Its accuracy is sigma_J=1/5+J/10.
4. A common diagonal cutoff schedule assembles potential increments, direct
   angular increments, and pressure. Its finite-stage requirements include
   actual nonlinear residual bounds, not just individual correction costs.

The previous Euler damping obstruction concerned the unchanged Euler scales.
It does not exclude this distinct viscous construction.

## Symbolic heat identity checked in RAD

For a>1 and z>=0, the source proposes

    F(a,z) = Gamma(a)^(-1) integral_0^infinity
             exp(-v) v^(a-1) (1+z*v)^(1-a) dv.

Let K be that integrand and G=exp(-v)v^a(1+z*v)^(-a).
RAD's exact polynomial algebra checks, after clearing (1+z*v)^2,

    z^2 K_zz + (1+2*a*z)K_z + a(a-1)K = (a-1) G_v.

This yields the profile ODE when differentiation under the integral and
vanishing boundary flux are justified. The source has analytic arguments for
these steps; **the RAD port currently checks the algebra, not those arguments**.

With s=r^2/2, tau=1-t, and b=1/2-a, put

    U(t,r) = s^b F(a,2*tau/s).

RAD also checks the exact coefficient identities

    b^2 - 1/4 = a(a-1),       1-2b = 2a.

Together with the profile ODE and the chain rule these are the radial angular
heat balance `U_t=U_rr+U_r/r-U/r^2`, for r>0. This is a physical diffusion
balance, unlike the auxiliary viscosity in the Euler PDF. It does not assert
regularity at r=0; the source uses the heat profile in the outer construction.

## Correction-margin port and WHY

`source_construction_audit.rad` ports the literal minimum formulas from
`ExponentLedger.lean`, including particular-wave, signed-wave, mean and defect
updates. It runs 36 isolated `fork_with`/`simulate_many` cases, varying six
stages and six loss parameters, including both sides of a strict-margin boundary.
At the source value kappa=1/100000, exact margins are:

| Obligation | Margin |
| --- | ---: |
| Particular gain beyond the next 1/10 step | 0.3 |
| Signed gain beyond that step | 0.29999 |
| Signed bar gain beyond the reserved 0.17 | 0.00998 |
| Completed mean gain beyond the next step | 0.07 |
| Completed defect gain beyond the next step | 0.79996 |

These are residual-order margins, **not velocity amplification factors**.
The bar-residual reserve is the tightest of these checks. Its strict inequality
fails at kappa=1/200 in the tested family. This is sensitivity of the arithmetic
only: it does not license changing the construction's kappa.

The resolver recomputes every result, checks fork isolation, and rejects an
injected claim that the analytic estimates or blowup are proved. WHY records
the provenance of those checks; it does not discover the missing analytic proof.
The run is deterministic with one and four workers, and record/replay passes.
No RAD kernel changes were needed.

Run:

```powershell
python projects/dogfood/navier-stokes/verify_source_construction_audit.py
```

Receipts: `source_construction_audit.json`, `source_construction_audit.why.txt`.

## Exact next obligation, now located in the supplied source

`MixedCandidateAssembly.StageEstimates.finite_residual` asks, schematically,

    |D^m R(U_J,P_J)| <= C_(J,m) q^(g_J-L_m),
    g_J=h*J/10 -> infinity,

near the singular endpoint. The derivative loss L_m must be independent of J;
the constants may depend on J and m and are handled by the diagonal selection.
This is the concrete proposed answer to the previous all-order force-budget
problem. Increasing residual order can eventually overcome each fixed derivative
loss. Proving that the actual fields satisfy the bound remains essential.

The source traces this obligation through
`GluedStageEstimates.actualStageEstimates`,
`ActualCycleResidualBounds.finite_residual_rates`, and
`ActualCyclePreservation`, with physical coordinate and jet bounds underneath.
The next port should audit that actual finite residual, including the viscous
term, old/new cross interactions, mean corrections, and cutoff derivatives.
It must then justify the infinite diagonal limit and endpoint force extension.

The source's closed theorem declaration and the successful finite RAD checks
do not supply an independently checked full proof. No Millennium solution or
new discovery is claimed here; the construction belongs to the supplied source.
