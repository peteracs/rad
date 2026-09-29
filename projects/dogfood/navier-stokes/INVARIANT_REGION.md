# Candidate region: full vector-field boundary checks

**Status: disproved and retired.** Exact direct RAD Fourier convolution finds
an outward boundary state; see `OUTWARD_COUNTEREXAMPLE.md`. The favorable
checks below are retained as historical calculations, not evidence of
invariance. This extends the instantaneous amplification calculation in
`AMPLIFICATION_ATTACK.md`. The setting remains the periodic three-dimensional,
unforced Navier–Stokes equation, with normalized volume and positive viscosity.

## Rejected region

Write

\[
 K=\tfrac12\|u\|_2^2,\quad E=\tfrac12\|\nabla u\|_2^2,
 \quad D=\|\Delta u\|_2^2,\quad S=E'+\nu D.
\]

The candidate inequalities are

\[
 B=S-2\nu D\ge0,\qquad Q=KD/E^2\le20/9,
 \qquad 0<K\le K_0.
\]

These allow the characteristic frequency to move, unlike fixed-mode support.
If they held for the entire smooth evolution, then

\[
 E'=S-\nu D\ge\nu D\ge2\nu E^2/K\ge2\nu E^2/K_0.
\]

Here the middle inequality is Fourier Cauchy–Schwarz,
\(D\ge2E^2/K\). Thus persistence alone would force finite-time loss of
smoothness, no later than \(K_0/(2\nu E(0))\). This conditional implication
does **not** establish persistence. The upper bound on Q controls the ratio
of dissipation to \(E^2/K\); it does not bound D absolutely.

## Exact reference calculation

Use the five nonzero waves of the prior six-mode family, all amplitudes one,
with sixth amplitude zero. The exact full vector field gives

\[
 K=3,\quad E=9/2,\quad D=15,\quad S=1,
\]
\[
 K'=-9\nu,\quad E'=1-15\nu,\quad D'=6-54\nu,
 \quad S'=53/4-4\nu.
\]

At viscosity \(\nu=1/30\), the state is on both candidate boundaries:

\[
 B=0,\quad Q=20/9,\quad
 B'=3851/300>0,\quad Q'=-38/405<0,
 \quad (E/K)'=19/60>0.
\]

Thus both boundaries point inward **at this state**. Nonlinear enstrophy
production is positive while nonlinear kinetic-energy production is zero;
the mean squared frequency increases even including viscosity. This is not
a lower bound on flux across every shell or on an advancing cascade front.

The full derivative has 30 nonzero Fourier modes from 10 input modes. To
compute the derivatives above, the generator uses

\[
 v=F(u),\qquad F'(u)v=-P[(v\cdot\nabla)u+(u\cdot\nabla)v]+\nu\Delta v,
\]
\[
 E''=\sum_k|k|^2|v_k|^2+
       \sum_k|k|^2\operatorname{Re}(\bar u_k\cdot(F'(u)v)_k).
\]

The first sum includes **all generated frequencies**. The contribution from
frequencies absent in u is 113/4. Omitting it would give an incorrect boundary
derivative. Leray projection includes global periodic pressure throughout.

## What the search checked

`build_invariant_boundary.py` computes exact rational/polynomial derivatives
for 18 specified shapes, including the outgoing sine mode at (1,0,1).
For each it chooses \(\nu=S/(2D)\) to test the production boundary. Sixteen
have positive viscosity and positive B'; two have negative viscosity and
are inadmissible. These are discrete boundary probes with different Q
values, **not** verification of the entire candidate region or its Q boundary.

`invariant_boundary.rad` checks five rational viscosities in [1/96,1/30]
at the reference shape. `verify_invariant_boundary.py` independently compares
its boundary arithmetic with the symbolic full Fourier derivative. All five
pass. RAD checks these finite integer calculations; SymPy supplies the
Fourier algebra. Neither constitutes an analytic PDE proof kernel.

Run from the repository root, with SymPy available:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/build_invariant_boundary.py
python -X utf8 projects/dogfood/navier-stokes/verify_invariant_boundary.py
```

## Why phase and tail constraints remain essential

Under u -> -u, K, E, D and every modal energy are unchanged, but S changes
sign. Consequently no nonempty region specified solely by those quadratic
quantities can guarantee positive enstrophy production for every member.
The B constraint explicitly distinguishes phase correlations.

For the proposed region, the missing theorem is a uniform inward bound
on B' wherever B=0 and Q<=20/9, and on Q' wherever Q=20/9 and B>=0,
including arbitrary admissible higher-frequency tails. The current checks
prove neither. Additional phase constraints would themselves require inward
boundary bounds under the full evolution. Compactness/regularity and a valid
invariance argument in the chosen infinite-dimensional function space also
remain to be supplied.

Strict signs persist in a sufficiently small smooth neighborhood of the
reference state and for a short interval by continuity. That does not stop
the trajectory from leaving that neighborhood through another boundary.
There is no certified residence time sufficient for blowup, no iterated
infinite cascade, and no claim to the Millennium Prize.
