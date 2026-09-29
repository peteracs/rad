# Validation receipt — 2026-09-08

No Navier–Stokes solution or Millennium theorem is certified by these checks.

## Direct amplification lower-bound attack

`verify_amplification.py` passed:

- Full symbolic Fourier derivative is divergence-free, respects reality,
  and satisfies the kinetic-energy identity including viscosity.
- Independent pure RAD complex convolution agrees at three test states,
  including a nonzero generated mode outside the candidate support.
- All 225 search outputs agree; 96 cone samples pass the inequality.
  The smallest scanned nonnegative integer remainder failure is f=39.
- Forged acceptance of a counterexample is rejected. One/four-worker output
  agrees and recorded-world replay verifies.
- Exact rational estimates support the continuous-cone lower bound.

The H2 perturbation extension, failure of fixed-region invariance, and
need for unbounded frequency transfer are written analytic deductions.
No invariant amplification region or finite-time singularity is certified.

## Quantitative local convergence

`verify_local_convergence.py` passed:

- Independent rational product-rule arithmetic confirms the three cutoff
  derivative bounds and the conservative H2 upper bound 7L<2^41.
- Twelve norm/time/error certificates satisfy the contraction inequalities.
- Insufficient norm and excessive-time certificates fail with the intended
  diagnostics; one/four-worker output and recorded-world replay agree.

The written proof gives Picard convergence for T=nu*2^(-90), with an
explicit geometric error bound. No spatial Picard iterates were computed;
the Sobolev/semigroup argument and infinite limit are outside the RAD kernel.
There is no established blowup lower bound for the constructed datum.

## Global initial-pressure matching

`verify_global_pressure.py --quadrature` passed:

- Symbolic differentiation verifies the general pressure-source
  decomposition into harmonic degrees 0, 2, and 4.
- RAD verifies the exact local particular ODEs and homogeneous exponents,
  with causal provenance, worker-count agreement, and recorded-world replay.
- Matching constants for the specified smooth cutoff agree between 30-
  and 45-digit quadrature runs to the tested absolute tolerance 1e-20.
- The exterior monopole cancellation is consistent with the numerical
  diagnostic. Its proof uses integration by parts, not numerical zero.

`global_pressure_integrals.json` labels the numerical output as not
interval certified. The Newton/Green construction, uniqueness, harmonic
matching, finite-energy estimates, and finite-order globally matched
recurrence are written analytic arguments. No convergence or blowup is
established by this pressure matching.

The interrupted local-jet extension was also completed and checked:
`verify_coupled_evolution.py` now verifies momentum orders 0, 1, 2 and
divergence orders 0, 1, 2, 3 for nu=1,2. All three pressure corruption
probes and a pre-multiplication overflow probe are rejected. Worker-count
and replay checks pass. The old order-t^2 residual is canceled by U_3 and
P_2; an explicitly nonzero order-t^3 curl remains. These local coefficients
are separate from the globally matched recurrence.

## Coupled evolution iteration

`verify_coupled_evolution.py` passed using the current debug executable:

- All spatial coefficients of momentum orders zero and one vanish for
  viscosities 1 and 2, including pressure and nonlinear cross interactions.
- Divergence orders zero, one, and two vanish exactly.
- Independent symbolic reconstruction agrees with the checked equations.
- Corrupting either pressure coefficient fails at the intended momentum
  order; one/four-worker stdout matches and recorded-world replay verifies.
- The two-mode radial leakage identity and the nonzero curl of the
  unresolved order-two residual are explicitly confirmed.

The calculation uses the linear collar from the earlier portfolio. Its
polynomial pressure is a local choice, not an established global pressure
for a finite-energy exterior. No convergence or blowup is certified.

## Additional angular structure

`verify_angular_correction.py` passed:

- Three RAD coefficient certificates satisfy the correction ODE exactly.
- Independent symbolic differentiation verifies the full-vector curl
  cancellation and the divergence-free correction.
- The new interaction residual is explicitly nonzero at the next order;
  the complete formula is recorded in `ANGULAR_CORRECTION.md`.
- Wrong-sign and altered-coefficient proposals fail with the intended
  ODE diagnostic.
- One/four-worker outputs match; recorded final-world replay verifies.

The global inverse formula and finite-energy conclusion are written
analytic arguments. No additional VM/kernel change was needed.

## Moving-cutoff iteration

`verify_cutoff_rad.py` passed using the current debug RAD executable and
an independent SymPy calculation:

- Three exact integer polynomial cases, including a constant control.
- Reproduced the earlier force-curl coefficients 191/216 and 42.
- Both nonconstant collar examples have a nonzero nonlinear angular
  projection; the tested linear and viscous projections vanish.
- Resolver recomputation, distinct-case coverage, fork write isolation,
  serialization round trips, and `why()` ancestry passed.
- One-worker and four-worker stdout matched. Recorded execution replayed
  with the same final world digest.
- Excessive coefficient and polynomial degree probes failed with the
  intended diagnostics before integer wraparound or monomial aliasing.

`verify_angular_identity.py` passed the general divergence, curl, nonlinear,
and diffusion identities using an arbitrary radial function in SymPy.
The quantitative 140/729 lower bound is a written analytic argument in
`MOVING_RADIAL_OBSTRUCTION.md`; its integration is not kernel-verified.
The new polynomial workload uses existing RAD operations, including the
previous iteration's checker fix; no further kernel mutation was necessary.

## Language change

`cargo test -p rad-vm --lib checker::tests -j 1 -- --quiet`:
279 passed, zero failed. New regression cases exercise local indexed writes,
effectful index rejection, global mutation rejection, resolver-local marks,
and preservation of the caller's copied list.

The same test binary's `vm::settlement::` filter: eight passed, zero failed.
`cargo build -p rad-cli -j 1`: succeeded. The workload checks below used the
resulting `target/debug/rad.exe`, with strict types and denied warnings.

## Research workloads

`python projects/dogfood/navier-stokes/verify_core_scale.py`:

- 704 candidate outputs matched independently computed rational exponents.
- 76 satisfy the necessary strict scale window.
- The separate fixed-profile classifier excludes those same 76 entries
  under the assumptions in `FIXED_PROFILE_OBSTRUCTION.md`.
- Forward/reversed causal settlements produced the same aggregate.
- Each serialized candidate retained its world digest; speculative writes
  stayed within `CoreResult` and left the authoritative world unchanged.
- Duplicate, forged, and incomplete evidence failed with their expected
  diagnostics.
- One-worker and four-worker runs produced identical stdout, including
  `why()` ancestry. Recorded execution replayed with a matching final world
  digest. The verifier uses a temporary trace and deletes it after checking.

`python projects/dogfood/navier-stokes/verify_frozen_mode.py`:
15 RAD mode outputs matched exact rational arithmetic; 50 monomial checks
confirmed the stated radial coordinate identities.

`python projects/dogfood/navier-stokes/verify_cutoff.py` (requires SymPy):
the solenoidal identity, collar diffusion polynomial, and coefficients
191/216 and 42 were verified by exact symbolic differentiation. This is an
external algebra computation, not a RAD-native certificate or Lean proof.

`git diff --check`: passed.

## What remains outside these checks

`verify_calibration.py`: complete symbolic polarization model, all 9,248
fork receipts over 193 rounds, selected full Fourier receiver, exact finite
PDE-bound arithmetic, worker determinism, replay and forged-map rejection
pass. `verify_forced_fourier.py` now checks 16 cases / 516 outputs, six invalid
inputs and the int64 bound for the enlarged coefficient/scale domain. The
coupled-evolution regression passes. The actual finite-solution implications
use the analytic arguments in `CALIBRATION_RUN.md`; scale-uniform return-map
closure remains unproved.

`verify_return_map_holes.py`: inverse metric, independent symbolic Jacobian,
uniform projected contraction comparison, Euclidean expansion witness,
finite remainder/reference-ODE arithmetic, WHY ledger and forged tail/map
rejection pass. The finite analytic shadowing result is in
`RETURN_MAP_HOLES.md`. Projected contraction is not full PDE contraction;
all six return-map obligations retain explicit unproved status.

`verify_repetition_gate.py`: 80 scalar design gates, exact radius elimination,
error/energy growth margins, geometric identities, WHY provenance and forged
solution-map rejection pass. The conditional blowup implication is proved in
`REPETITION_CRITERION.md`. Its illustrative constants are not established
Navier-Stokes estimates, and no actual all-stage return map is certified.

`verify_second_transfer.py`: all 32 second-band coefficients agree with an
independent full second Taylor derivative and the coupled source; H11 Picard,
force derivatives, normalized all-mode remainder, positive second-band energy,
WHY provenance and forged error/repetition rejection pass. The shared basis
refactor passes `verify_coupled_modes.py`. The actual-PDE conclusion uses
written analytic lemmas in `SECOND_TRANSFER.md`; this proves two finite
transfers, not indefinitely repeatable amplification.

`verify_coupled_modes.py`: independent full polarized energy pairings, exact
donor/receiver projection coefficients, nonzero omitted-mode witness, all-mode
remainder arithmetic, signed amplitude derivatives, WHY provenance and forged
truncation/repetition rejection pass. The finite actual-PDE conclusions use
the written analytic lemmas in `COUPLED_EVOLUTION.md`. The projection equations
retain remainders; they are not a closed finite-dimensional fluid model.

`verify_receiver_profile.py`: independent 16-mode receiving profile, complete
self-interaction (production -7, dissipation 443, enstrophy rate -450), H7
Picard arithmetic, O(t^2) error and relative 2^-50 bound, WHY provenance and
forged production/repetition rejection pass. The continuum argument is in
`RECEIVER_PROFILE.md`. This improves the earlier coarse relative-error audit;
an autonomously amplifying receiver and a coupled repeatable cascade remain
unestablished.

`verify_transfer_closure.py`: weighted high-band energy and fraction bounds,
whole-profile versus projected relative-error arithmetic, WHY provenance,
and forged profile/repetition rejection pass. The written projection lemma
in `TRANSFER_CLOSURE.md` connects these bounds to the actual solution. Large
error upper bounds are not interpreted as lower bounds on actual error,
and the small high-band energy fraction is not a universal blowup exclusion.

`verify_evolving_transfer.py`: independent source at (2,1,0), exact evolving
heat profile, rational mode-specific and H3 error bounds, energy margins,
WHY provenance and forged source/repeatability rejection pass. The shared
seed-module refactor also passes `verify_solution_segment.py`. Written
all-mode analytic lemmas in `EVOLVING_TRANSFER.md` imply net high-band flux
with decreasing total energy on [0,2^-98]; no repeatable cascade is certified.

`verify_cascade_obstruction.py`: independent Fourier support separation and
seed norms, geometric-series arithmetic for the actual infinite force impulse,
global energy exclusion of a doubled full-torus seed, WHY provenance, and
forged reset/transfer rejection pass. The projection and continuum energy
lemmas are written in `CASCADE_OBSTRUCTION.md`. The exclusion is specific
to that target and force/data; no general obstruction to blowup is asserted.

`verify_solution_segment.py`: independent full Fourier nonlinear production,
H3/H5 seed norms, rational contraction and displacement bounds for two
consecutive solution tubes, positive nonlinear and net enstrophy rates,
WHY provenance, and forged reset/gain rejection pass. The actual PDE
conclusion relies on the analytic proof in `SOLUTION_SEGMENT.md`, not a
RAD-formalized semigroup theorem. Both intervals have duration 2^-99;
their endpoint tubes enlarge. No repeatable frequency-transfer stage follows.

`verify_force_first.py`: 676 derivative-budget instances, actual schedule
descriptors, full dictated Fourier RHS, all 27 bounded-control probes,
local Picard arithmetic, forged cascade rejection, worker determinism and
replay pass. Global force smoothness and local PDE existence are proved by
the written analytic lemmas in `FORCE_FIRST_SCHEDULE.md`, not formalized in
the RAD kernel. The growth probe is not a reached solution state and its
positive enstrophy derivative is direct forcing, not nonlinear production.

`verify_force_cost.py`: actual force boundary data, 40 constant-germ costs,
three nonconstant-germ costs, Taylor kernels through order eight, and eight
causal accounting cases pass. Hidden background-cost rejection, deterministic
workers and replay also pass. The lower bound is independent of interior
shape but conditional on the stated force boundary data; it does not rule
out all smooth-forced constructions.

`verify_constraint_elimination.py`: three prescribed entrance-force targets
solved; initial germs and shared velocities preserved; four independent
polynomial endpoint identities and the exact triangular inverse verified.
Forged small-force rejection, deterministic workers and replay pass. The
solver closes this endpoint/join block, not the infinite-stage force budget.

`verify_construction_why.py`: six exact causal diagnoses, independently
computed entrance jets and midpoint force increment -31i/16; forged budget
rejection, deterministic workers and replay pass. The rational force-operator
extension passes twelve cases / 386 outputs, and the four-stage trajectory
regression remains green. Diagnoses apply to the specified grammar and
1/16 test budget, not the existence of smooth-forced singularities in general.

`verify_trajectory_stage.py`: four complete finite stages and 76 bivariate
Fourier force polynomials agree with independent algebra. The switching
derivative, physical-time endpoint germs, monotone fixture enstrophy,
causal selection, forged infinite-budget rejection, worker determinism and
world replay pass. All-order smoothness relies on the written fixed-step
analytic lemma. No infinite-stage force estimate or blowup is certified.

`verify_forced_fourier.py`: nine exact operator cases, 281 complete Fourier
outputs compared against independent complex algebra; four invalid inputs
rejected. `forced_fourier_check.rad` also passes its heat-flow and generated
force checks. This verifies instantaneous projected force computation only,
not that supplied time-derivative data form a trajectory or a repeatable stage.

`verify_outward_boundary.py`: passed. Direct RAD convolution and independent
symbolic algebra agree on two exact outward witnesses. At viscosity 1/30
the proposed region has K=15/8<3, B=1/10>0, Q=20/9 and Q'=16/135>0.
The candidate region is disproved, superseding its favorable point tests.

`verify_invariant_boundary.py`: five RAD rational boundary calculations
agree with a symbolic full Fourier jet, including the 113/4 contribution
to enstrophy acceleration from previously absent frequencies. The separate
18-shape production-boundary probe has 16 positive-viscosity states; two
negative-viscosity states are excluded. These checks do not establish
invariance, shellwise flux bounds, or control of arbitrary spectral tails.

The energy and continuation arguments are written analytic deductions.
The public Lean proof was not rebuilt. The force-first family has all-order
smooth-force bounds; no changing blowup profile, outer matching solution,
nonlinear stability argument, or infinite-stage viscous blowup construction
has been supplied.
