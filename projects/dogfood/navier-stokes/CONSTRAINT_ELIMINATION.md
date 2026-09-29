# An explicit solution of the entrance-force/join constraint block

The previous diagnosis suggested a broad joint search. For the particular
entrance constraint that was blocking the fixture, that is unnecessarily
general: the relevant equations form an exactly solvable triangular block.

`constraint_elimination.rad` now constructs that solution and has WHY record
the independently checked result. It preserves the predecessor's initial
velocity and **all** its initial derivatives, as well as the shared velocity
and the complete smooth join. This improves on `backpropagate_jet`, which
changed the predecessor's initial data.

This solves the zeroth entrance-force equation and the associated join
constraints in the tested grammar. It does not solve every entrance force
derivative, a whole-interval small-force bound, or finite-time blowup.

## The useful structural trick

In u=A(s)+theta(s)B(s), the current stage's B has zero influence on its
entrance derivatives because theta is flat and zero there. Those parameter
directions should be eliminated from the entrance equation.

At the **preceding stage's exit**, theta is flat and one. Its B can therefore
change exit derivatives without changing any entrance derivative. This
provides an explicit right inverse for the missing endpoint adjustment.

For a unit-duration derivative-only correction delta v at the shared state,
use, on the predecessor,

    delta u_left(s) = theta(s) (s-1) delta v.

At s=0 every time derivative of this correction vanishes. At s=1 its value
is zero, its derivative is delta v, and its higher endpoint derivatives are
zero. On the current stage, change A1 by delta v. The two corrections have
matching endpoint germs. Both original endpoint velocity values are preserved.

The current nonlinear and viscous entrance terms depend on A0, which has
not changed. Therefore changing its physical derivative by delta v changes
the entrance force by exactly delta v. If r is the entrance force minus
the chosen target force, the correction is simply

    delta v = -r.

This is an exact identity, not a first-order approximation to a nonlinear
optimization problem. It requires the correction to be represented in the
chosen divergence-free Fourier space; the implementation verifies the full
generated-mode residual before accepting it.

## The invertible block

Let hR and hL be the current and previous inverse durations. Let delta a1
be the change of the current normalized linear coefficient. Let delta b1
be the predecessor's linear change, with delta b0=-delta b1 so the shared
velocity stays fixed. For a previously matched derivative, the equations
for each scalar Fourier coordinate are

    [ hR    0 ] [delta a1] = [-r]
    [-hR   hL ] [delta b1]   [ 0].

The determinant is hR*hL>0. The exact solution is

    delta a1 = -r/hR, delta b1 = -r/hL.

Thus this constraint does not require a haystack search. The invisible
parameter block has been removed, and the remaining block is inverted
explicitly. The flatness and endpoint-transfer facts are written analytic
inputs, not mathematical facts discovered by the provenance ledger.

## General quadratic endpoint linking

`link_predecessor(upstream, downstream)` preserves the upstream A and sets
its B so that

    A_left(s)+B_left_new(s) = A_right((hR/hL)(s-1)).

It first checks that the original shared velocity agrees with the requested
downstream entrance. The identity then preserves that shared velocity and
matches all derivatives of the polynomial endpoint germs in physical time.
The fixed flat switch makes the actual trajectories match smoothly to
every order. Nonrepresentable rational coefficients are rejected, not rounded.

Since upstream A is unchanged, an already smooth join at its entrance is
unaffected. The correction changes the upstream trajectory in its interior;
it does not leave the entire previous solution unchanged.

## Executed evidence

The application solves three explicitly prescribed entrance-force targets:
zero, the stationary base force, and twice the stationary base force. These
are three requests to the exact solver, not a random parameter search.
For each, it verifies the full projected Fourier force equation, preserves
the initial germ and shared velocity, and checks the full join.

`verify_constraint_elimination.py` independently verifies the force equations
and four polynomial endpoint identities, including a separate duration and
curvature-change test. It checks the symbolic triangular inverse, rejection
of a forged whole-interval small-force claim, deterministic worker-count
output, and recorded-world replay. All checks pass.

WHY records

    EliminationResult <- VerifyElimination <- JetSolved <- SubmitJetSolution.

The output explicitly retains `whole_interval_small_force_proved: false`
and `blowup_proved: false`. For example, the zero-target repair has computed
conservative force upper bounds 672 and 294 on the preceding and current
intervals. Those are upper bounds, not impossibility witnesses, but they
do not certify the desired small residual budget.

## The remaining mathematical problem

Endpoint force equations and smooth joins can now be built into proposals
rather than rediscovered by search. The unresolved constraint is the force
throughout the modified intervals, including mixed derivatives, and its
behavior under indefinitely repeated refinement. A correction can solve
its endpoint equation while moving a large force cost into the interior.

The next force derivative also imposes a separate acceleration equation.
Neither the triangular solve nor its causal trace supplies convergence of
an infinite jet construction or a repeatable amplification theorem.

Run with SymPy available:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_constraint_elimination.py
```
