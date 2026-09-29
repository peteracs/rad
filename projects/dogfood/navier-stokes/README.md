# Navier–Stokes research: first viscosity audit

Target: a complete proof of alternative C or D in Clay's official problem.
Status: **no Navier–Stokes blowup construction or Millennium solution**.

[PRIMARY_COVARIANCE_CHAIN.md](PRIMARY_COVARIANCE_CHAIN.md) derives the
source's actual linear pulse lower bound, Gaussian covariance concentration,
explicit perturbation tolerance, flat-target column scaling, and all-order
inverse-jet majorant under stated coefficient hypotheses. RAD checks 32 cone
and 512 determinant certificates, with requested-case validation, isolated
forks, settlement rechecking, deterministic workers, replay, and mutation
rejection. The accompanying analytic proofs are not RAD kernel proofs of
the complete nonlinear construction.

[SIGNED_COVARIANCE_PROOF.md](SIGNED_COVARIANCE_PROOF.md) gives the signed
cross-cancellation identity, retained quadratic error, and half-weight jet
induction. RAD derivative expansions through order eight agree with an
independent symbolic calculation.

[MODAL_ENERGY_PROOF.md](MODAL_ENERGY_PROOF.md) proves the actual two-mode
operator's all-state, all-nonzero-harmonic energy estimate, sharpening its
entrywise error factor from 4 to 2. RAD checks 16 sum-of-squares certificates,
replay, and rejection of factor 1. The coefficient hypotheses were traced to
the base-phase constructor; full construction validation remains unfinished.

[UNIFORM_SCALE_BOUNDS.md](UNIFORM_SCALE_BOUNDS.md) derives the source's
uniform weighted-inverse exponential, clock cancellation, polynomial geometry
cost, and all-degree dyadic absorption. RAD checks symbolic identities and six
polynomial certificates with replay and forged-invariant rejection. The actual
primitive weighted estimates and full correction invariant remain unverified.

[PRINCIPAL_CANCELLATION.md](PRINCIPAL_CANCELLATION.md) ports the source's
pressure-projected particular correction into RAD polynomial identities in
16 independent variables. Principal source cancellation and tangency-defect
damping pass, including replay and omission rejection. Local derivative
propagation is derived analytically; scale-uniform localized remainder bounds
and full invariant preservation remain unverified.

[NONLINEAR_RESIDUAL_PROOF.md](NONLINEAR_RESIDUAL_PROOF.md) proves an all-order
nonlinear residual stability inequality and the flat-tail consequence. A new
RAD differential-polynomial application checks 105 mixed-jet identities with
viscosity and both cross terms, plus replay and mutation rejection. The actual
construction's native cancellation and invariant-preservation estimates remain
unverified; the general stability inequality does not assert those estimates.

[SOURCE_CONSTRUCTION_RAD.md](SOURCE_CONSTRUCTION_RAD.md) maps the supplied
NavierStokesAndEuler source and ports its heat-profile algebra and correction
ledger into RAD. Thirty-six forks, exact polynomial checks, deterministic
replay, and forged-claim rejection pass. This source includes physical
viscosity and a residual-order iteration; its full analytic proof is not
independently verified. Lean execution was stopped at the user's request.

[DESKTOP_EULER_AUDIT.md](DESKTOP_EULER_AUDIT.md) examines the different
45-page local manuscript claiming unforced Euler blowup. Its history and
frame-transfer construction supplies a concrete new candidate. An exact RAD
operator/scale audit identifies why its auxiliary viscosity does not provide
physical Navier–Stokes evolution, and why the unchanged scale bounds do not
make physical diffusion perturbative. The manuscript itself is not verified.

[CALIBRATION_RUN.md](CALIBRATION_RUN.md) records 193 adaptive RAD fork rounds
covering 9,248 polarization/radius/existence-method cases. Exact algebra
retires amplitude-only receiver tuning; 20 polarization shapes have positive
donor and receiver nonlinear production. The selected finite transfer is
certified to time 2^-44, with a verified full Fourier receiver and replay.
Its receiver remains too small to amplify autonomously; no return map is
proved. The finite grid is exhausted and no background job is running.

[RETURN_MAP_HOLES.md](RETURN_MAP_HOLES.md) solves an inverse error-metric
problem for the known coupled amplitude equations. A diagonal metric makes
the projected flow contractive on the certified rectangle and gives a finite
shadowing bound. WHY exposes the unresolved full-tail decay, residual,
coordinate-change and profile-matching bounds explicitly; none is assigned
a proved value. The full PDE return map remains open.

[REPETITION_CRITERION.md](REPETITION_CRITERION.md) states a precise conditional
solution-map criterion for indefinite repetition and eliminates an error-radius
search algebraically. Eighty exact scalar design cases pass independent checks;
WHY refuses to promote feasible numbers into a proved PDE return map. The
actual all-stage profile inclusion and stability estimates remain missing.

[SECOND_TRANSFER.md](SECOND_TRANSFER.md) proves that the coupled original and
first-receiver structure generates a second 32-mode profile beyond squared
frequency 6, reaching 14. The complete second Taylor derivative agrees with
the polarized source, and an H11 remainder bound gives relative H3 error at
most 2^-12 on the existing short interval. This is a second finite transfer,
with amplitude of order t^2, not a scale-uniform repetition theorem.

[COUPLED_EVOLUTION.md](COUPLED_EVOLUTION.md) derives exact donor/receiver
projection equations with all omitted modes retained in bounded remainders.
Full Fourier polarization identifies a positive feeding term and its energy
feedback. On the existing interval, the actual donor amplitude decreases and
receiver amplitude increases. An explicit outgoing mode excludes finite-mode
invariance; repeated finer-scale handoff remains unproved.

[RECEIVER_PROFILE.md](RECEIVER_PROFILE.md) improves the finite profile error
to O(t^2) using an H7 solution bound. The whole generated band enters an
explicit 16-mode profile with relative H3 error at most 2^-50. Exact full
Fourier arithmetic then shows this normalized receiver has standalone
nonlinear enstrophy production -7: it cannot be promoted directly to an
autonomous amplifying seed. The coupled donor/receiver iteration is unproved.

[TRANSFER_CLOSURE.md](TRANSFER_CLOSURE.md) bounds the actual new-band energy
fraction by 2^-56 throughout the certified interval and compares whole-profile
and single-coefficient relative errors. It excludes a significant-energy
handoff on this interval and explains why tracking one coefficient does not
certify a whole next-stage profile. A localized or later cascade is not ruled
out; the normalized handoff theorem remains missing.

[EVOLVING_TRANSFER.md](EVOLVING_TRANSFER.md) proves finite nonlinear transfer
to mode (2,1,0), beyond the initial frequency band, while total kinetic energy
decreases. An evolving first-Picard profile controls all H3 errors; a sharper
mode-specific functional certifies the tiny transfer that the coarse norm
cannot resolve. Independent Fourier/profile checks and forged repeatability
rejection pass. The analytic argument lasts to time 2^-98 and does not prove
a repeatable scale transition or significant transferred energy fraction.

[CASCADE_OBSTRUCTION.md](CASCADE_OBSTRUCTION.md) audits why the two short
segments do not yield a cascade. An exact Fourier separator excludes the
immediate doubled seed, and a global force-impulse/energy bound excludes
the doubled full-torus seed at every smooth time for this initial-value
problem. The same bounded H3 tube cannot yield blowup. WHY distinguishes
these exclusions from the still-unproved localized transfer and normalized
error contraction requirements.

[SOLUTION_SEGMENT.md](SOLUTION_SEGMENT.md) proves two consecutive actual
solution intervals with positive nonlinear production under the prescribed
force, starting from the earlier five-wave amplification datum. RAD checks
the exact Fourier and bound arithmetic; written analytic lemmas control all
H3 tails. Each interval lasts only 2^-99 and gains enstrophy at least 2^-82.
The endpoint enters an enlarged next tube, spending error margin; same-region
closure, finer-scale transfer and repeatable amplification remain unproved.

[FORCE_FIRST_SCHEDULE.md](FORCE_FIRST_SCHEDULE.md) prescribes an actual
infinite smooth force schedule with proved all-order derivative bounds,
uniform over bounded pulse controls. RAD derives the full velocity derivative
and WHY distinguishes direct forcing from nonlinear amplification. Independent
algebra checks all 27 control probes. A written local existence estimate
defines the initial evolution; repeated amplification remains unproved.

[FORCE_COST_GUARD.md](FORCE_COST_GUARD.md) adds an exact Taylor-remainder
lower bound on force derivative cost. It detects when endpoint data already
make a budget impossible, regardless of the interior blend, and prevents
hiding the same cost in a background-force label. It is a necessary
compatibility test, not a sufficient whole-interval force certificate.

[CONSTRAINT_ELIMINATION.md](CONSTRAINT_ELIMINATION.md) replaces the entrance
force/join search with an explicit triangular solve. The predecessor's flat
switch repairs its exit jet while preserving initial data and the shared
velocity. Three target-force solves and independent endpoint identities pass;
whole-interval small forcing remains unproved.

[CONSTRUCTION_DIAGNOSIS.md](CONSTRUCTION_DIAGNOSIS.md) adds a WHY application
that identifies immutable endpoint-force constraints, propagates a local
derivative repair upstream, distinguishes background force from increments,
and checks an exact interior obstruction. It diagnoses this grammar rather
than claiming no construction exists.

[TRAJECTORY_STAGE.md](TRAJECTORY_STAGE.md) extends the repair grammar to whole
smooth finite trajectories. RAD computes exact full force polynomials,
derives interval bounds and checks all-order stage joins through polynomial
endpoint germs. The causal two-stage selection and independent 76-mode
polynomial verification pass; infinite-stage closure remains unproved.

Active direction: [FORCED_CONSTRUCTION.md](FORCED_CONSTRUCTION.md), targeting
smooth-forced ordinary Navier–Stokes on the periodic torus (Clay D).
The new RAD force budget checks sufficient all-order summability arithmetic
and distinguishes carrier damping from envelope diffusion. Actual residual
estimates and a repeatable amplification stage remain missing.

[WHY_REPAIR.md](WHY_REPAIR.md) demonstrates a provenance-guided local repair:
the full residual supplies a generated-mode coefficient, two isolated proposals
are checked, and WHY traces the selected correction. Physical-space algebra,
forged-proof rejection and replay pass. This is not a stage construction.

[OUTWARD_COUNTEREXAMPLE.md](OUTWARD_COUNTEREXAMPLE.md) **disproves** the
proposed region S>=2nu D and KD/E^2<=20/9. Direct RAD Fourier convolution
and independent symbolic algebra agree on an outward boundary witness at
the original viscosity and energy cap. The region is retired. Favorable
point checks in `INVARIANT_REGION.md` do not establish its invariance.

[AMPLIFICATION_ATTACK.md](AMPLIFICATION_ATTACK.md) now targets the missing
lower bound directly using a separate 3D periodic family. It derives
E'>=E^(3/2)/64 on a continuous six-mode parameter region, extends it to
small arbitrary H2 perturbations, and identifies exact counterexamples to
unrestricted remainders and fixed-mode invariance. Run
`python projects/dogfood/navier-stokes/verify_amplification.py` with SymPy
available. The 225-case RAD search and direct Fourier checker include the
global pressure projection and viscosity; persistence to blowup is unproved.

[LOCAL_CONVERGENCE.md](LOCAL_CONVERGENCE.md) now gives a quantitative
short-time Picard convergence argument for the globally localized datum:
T=nu*2^(-90), using the conservative bound ||U_0||_H2<2^41. The arithmetic
is checked by `python projects/dogfood/navier-stokes/verify_local_convergence.py`.
It also explains why the smooth cutoff does not justify Taylor convergence
and why none of the current estimates establishes blowup. This is an
instance of classical local theory, not a Millennium breakthrough.

[GLOBAL_PRESSURE_MATCHING.md](GLOBAL_PRESSURE_MATCHING.md) specifies a
smooth global cutoff, constructs the decaying pressure through three
radial integral formulas, and fixes the collar's harmonic correction.
It also defines globally matched finite time coefficients. Run
`python projects/dogfood/navier-stokes/verify_global_pressure.py --quadrature`
for exact algebra checks, RAD provenance/replay, and non-certified numerical
matching constants (SymPy/mpmath required).

[COUPLED_EVOLUTION.md](COUPLED_EVOLUTION.md) continues beyond the single
angular cancellation. It shows that two radial modes do not close and
checks a full spatial time expansion, with pressure and viscosity, through
the first three momentum orders. Run
`python projects/dogfood/navier-stokes/verify_coupled_evolution.py` (SymPy
required). This polynomial expansion remains local: its coefficients differ
from the globally matched recurrence, and its order-t^3 residual is nonzero.

[ANGULAR_CORRECTION.md](ANGULAR_CORRECTION.md) now constructs an explicit
second angular strain mode that cancels the identified nonlinear term at
a reference time. It gives a global finite-energy inverse formula and
computes the new cross/self-interaction residual. Run
`python projects/dogfood/navier-stokes/verify_angular_correction.py` (SymPy
required) for the RAD certificates, independent derivatives, corruption
checks, parallel reproducibility, and replay.

See [VALIDATION.md](VALIDATION.md) for the executed checks, including parallel
reproducibility, causal corruption rejection, and recorded-world replay.

Latest iteration: [MOVING_RADIAL_OBSTRUCTION.md](MOVING_RADIAL_OBSTRUCTION.md)
derives a quantitative force-curl obstruction even when the radial cutoff,
amplitude, and support radius vary in time. `cutoff_polynomial.rad` moves
the finite collar calculations into exact bounded integer polynomial
arithmetic in RAD, with fork isolation, wire round trips, recomputing causal
settlement, and `why()` ancestry. This uses the existing language and the
local-mutation checker fix; it adds no PDE axiom to the kernel.

```powershell
target/debug/rad.exe projects/dogfood/navier-stokes/cutoff_polynomial.rad --experimental-laws --strict-types --deny-warnings
# The independent comparison scripts below require SymPy:
python projects/dogfood/navier-stokes/verify_cutoff_rad.py
python projects/dogfood/navier-stokes/verify_angular_identity.py
```

The [cross-paper attack](CROSS_PAPER_ATTACK.md) compares IPM, Boussinesq,
Euler, and the public `affinecore` formalization. It identifies affine cores
as a limitation of a purely Fourier damping argument, derives the unchanged
Boussinesq schedule's growing diffusion exposure, and identifies a nonempty
necessary scaling window for a new three-dimensional localized core.
Run `python projects/dogfood/navier-stokes/verify_core_scale.py` to check its
704-case RAD exponent portfolio against independent rational arithmetic.
This causal workload needs the checker fix in the current source: build with
`cargo build -p rad-cli -j 1` first. Its verifier defaults to `target/debug/rad.exe`;
pass `--rad PATH` to select another freshly built binary.
The missing nonlinear localization and force-cancellation lemma is explicit.

[FIXED_PROFILE_OBSTRUCTION.md](FIXED_PROFILE_OBSTRUCTION.md) proves an energy
obstruction for an exact fixed compact profile in the candidate scale window.
All 76 scale-feasible entries are excluded for that ansatz. The RAD workload
now assembles its 704 cases through checked causal proposals, compares reversed
proposal order, checks serialized forks, and emits `why()` ancestry.

[CUTOFF_RESIDUAL.md](CUTOFF_RESIDUAL.md) computes an explicit non-gradient
viscous collar residual for a compact solenoidal extension of the triaxial
affine core. [RAD_RESEARCH_MAP.md](RAD_RESEARCH_MAP.md) maps the open analytic
obligations to the language capabilities and records the checker issue
exposed by duplicate detection in the causal resolver.

This investigation starts from the public *Blowup for the Euler Equations
with Smooth Forcing*. It identifies a rigorous obstruction to retaining
that construction's bounded-velocity conclusion, derives the viscous
coordinate equations, and checks a frozen Fourier-mode model in RAD.

Read [VISCOSITY_AUDIT.md](VISCOSITY_AUDIT.md) for the derivations and precise
limits. `frozen_mode.rad` checks finite rational arithmetic and speculative
isolation; it is not a PDE solver or a formalization of the analytic argument.

From the repository root:

```powershell
target/debug/rad.exe projects/dogfood/navier-stokes/frozen_mode.rad --strict-types --deny-warnings
python projects/dogfood/navier-stokes/verify_frozen_mode.py
```

The Python checker independently evaluates the fixed rational examples and
checks coordinate identities on monomials. It does not validate RAD's VM,
prove continuation theory, or certify a nonlinear fluid solution.

The source PDF retrieved on 2026-09-08 contains 112 pages:

- URL: https://cims.nyu.edu/~tristanb/euler.pdf
- SHA-256: `97ef408bff09b4f6ed9f3867734d1eb2245f3f34e6334b28136c84c02d0ae8d8`
- Public statement: https://raw.githubusercontent.com/tristanbuckmaster/fluid_lean/refs/heads/main/euler-blowup/Challenge.lean
- Clay statement: https://www.claymath.org/wp-content/uploads/2022/06/navierstokes.pdf

The Euler proof has not been rebuilt here. The PDF analysis is targeted at
Theorem 1.1, Sections 2–3, the parameter selection in Section 12, and the
boundedness/extension arguments in Section 13; it is not a full proof audit.

Next required research result: a replacement amplification mechanism that
allows unbounded physical velocity and involves the axis or breaks axisymmetry,
while retaining finite energy, positive viscosity, and admissible smooth force.
A finite collection of favorable
amplitude calculations would still require nonlinear stability, localization,
all-order force estimates, and an infinite-stage construction.
