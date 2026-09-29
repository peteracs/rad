# RAD research workflow and the remaining mathematical work

The target remains Clay's three-dimensional, positive-viscosity problem.
No current workload supplies a blowup solution.

| Mathematical task | RAD operation | Actual guarantee / missing evidence |
|---|---|---|
| Compare parameter regimes | `fork_with`, `simulate_many` | Isolated finite evaluations; no implicit quantification over all scales |
| Reproduce an interesting candidate | deterministic seed, serialized world | Reproducible inputs and result; not a proof of a continuum profile |
| Exclude incomplete evidence | typed `intent`, `resolver`, `constraint` | Full finite portfolio, no duplicate slots, recomputed arithmetic |
| Explain a classification | `why(entity, component)` | Causal path from proposals to aggregate; analytic justification is linked separately |
| Check speculative isolation | `assert_only_changed`, `world_digest` | Runtime write-footprint and unchanged authoritative state |
| Check order independence | forward/reversed settlements | Same complete aggregate in both tested orders; resolver's commutative counting gives the general argument |
| Preserve exact numbers | existing integer/arbitrary-natural libraries | Arithmetic correctness within declared domains; differential checker adds evidence |
| Audit an infinite-domain assertion | a proved analytic reduction plus finite certificate | Required reduction must be written and verified, not replaced by a finite scan |
| Improve throughput | native extension only after a measured bottleneck | Does not reduce analytic assumptions or establish mathematical soundness |

## Implementation map from `rg.exe`

Paths below are relative to the repository root. These are implemented
capabilities; no claim of exclusivity over other systems is established.

| Capability | Implementation |
|---|---|
| Heterogeneous speculative lanes and per-lane seeds | `core/vm/src/vm/builtins_impl/sandbox_and_rollouts.rs`, `bi_simulate_many` |
| Override a resource without committing live state | Same file, `bi_fork_with` |
| Serialize and restore candidate worlds | Same file, `bi_fork_to_bytes`, `bi_fork_from_bytes` |
| Explain component writes through the provenance ledger | `core/vm/src/vm/builtins_impl/world_loading_and_sandbox.rs`, `bi_why` |
| Enforce a permitted component write footprint | Same file, `bi_assert_only_changed` |
| Resolve proposals and record settlement ancestry | `core/vm/src/vm/settlement/commit.rs`, `resolve_and_commit_settlement`, `record_settlement_provenance` |
| Replay native calls made inside speculative lanes | `core/vm/src/vm/nested_native_replay.rs`, `prepare_nested_native_simulation` |
| Check local indexed mutation and effectful index expressions | `core/vm/src/checker/declarations/effect_analysis.rs` |

`why()` reports the execution's causal ancestry. It does not infer why a
mathematical proposition is true or validate the energy identity by itself.
`cutoff_polynomial.rad` now performs the finite polynomial residual
calculation in pure RAD using explicitly bounded integer coefficients,
degree checks, differentiation, curl, and exact half-point evaluation.
SymPy provides an independent comparison rather than supplying RAD's
classification. The analytic bridge to the continuum equation remains
separate. `verify_angular_identity.py` additionally checks the general
radial-function algebra externally, without formalizing the integration.

## Current result

The 704-case core-scale portfolio has 76 entries passing the strict local
scaling screen. All 76 fail the exact global fixed compact-profile ansatz by
the energy identity in FIXED_PROFILE_OBSTRUCTION.md. Both facts are retained
in separate aggregate fields and connected to their candidate proposals.

## Open mathematical obligations

`AMPLIFICATION_ATTACK.md` records a new `rg.exe` capability audit and the
direct lower-bound attack. `spectral_rate.rad` evaluates complex Fourier
convolution with the exact pressure projection; `amplification_search.rad`
checks 225 states and preserves both successful inequalities and the
canonical counterexample in causal evidence. The analytic bound holds in
a nonempty region, including sufficiently small H2 perturbations, but the
tested fixed-amplitude region is not invariant. The unresolved target is
a persistent frequency-transfer mechanism with controlled remainders,
compatible with kinetic-energy dissipation.

`LOCAL_CONVERGENCE.md` replaces an unsupported Taylor-convergence claim
with an explicit local Picard argument for the actual global datum.
`local_convergence.rad` checks derivative/norm-bound arithmetic and rational
contraction/error certificates with causal provenance. It does not execute
the spatial Picard iteration. The local interval T=nu*2^(-90) is established
by the written analytic argument. Blowup and long-time control remain
unproved, and no blowup lower bound has been found. Extending finite Taylor
tables alone cannot resolve these obligations.

`GLOBAL_PRESSURE_MATCHING.md` now specifies a global initial velocity and
its unique decaying pressure, reduces that pressure to three radial Green
integrals, and fixes the harmonic correction to the local pressure. Its
globally matched finite-order recurrence uses the Leray projection. The
existence of each finite smooth L2 coefficient is an analytic argument;
uniform bounds in the time order and convergence remain missing.
`global_pressure.rad` checks the local radial identities and homogeneous
modes. Numerical quadrature of the matching constants is explicitly
separate from exact RAD certificates.

`coupled_evolution.rad` now checks all spatial coefficients of three
momentum orders and four divergence constraints for a local polynomial
time expansion. `COUPLED_EVOLUTION.md` records why the two radial modes
fail to close, the full coupled recurrence, and the unresolved global
pressure choice. The generated sparse coefficients are independently
checked in RAD rather than accepted on the generator's authority.
`poly11.rad` expands the polynomial degree capacity and rejects excessive
products before multiplication. These are pure RAD library operations,
not new trusted kernel primitives.

`angular_correction.rad` now supplies a coefficient-verified inverse for
one missing angular mode, using B=A^2-(14/3)I and a radial ODE. Its resolver
checks the differential equation rather than merely rerunning the solver.
`ANGULAR_CORRECTION.md` gives the global finite-energy interpretation and
the nonzero next-order interactions. This closes the first targeted
instantaneous cancellation, not the coupled evolution obligations below.

1. Choose a changing three-dimensional profile or a core/outer-flow pair
   that evades the fixed-profile energy obstruction and the off-axis
   axisymmetry obstruction. Merely changing a radial cutoff around the
   fixed triaxial affine core also fails: see `MOVING_RADIAL_OBSTRUCTION.md`.
2. Construct the divergence-free outer velocity and pressure so the required
   local energy flux is produced by the actual Navier–Stokes dynamics.
3. Bound the non-gradient localization residual; a large gradient may enter
   pressure, but a large curl cannot be hidden there.
4. Produce a viscous correction scheme and bounds in every mixed derivative
   for its actual physical forcing.
5. Prove one infinite choice of parameters, finite singular time, solution
   existence before that time, and failure of global smooth continuation.

The kernel cannot supply these missing analytic lemmas merely by recording
an `accepted` field. The workload's declarations track finite classifications,
not a claimed Millennium proof.

## Concrete language issue exposed by this workload

The causal resolver needs a local `seen` list to reject duplicate portfolio
entries efficiently. The purity checker previously accepted reassignment of
a local variable but rejected `seen[slot] = true` as non-local mutation.
The source fix recognizes indexed targets rooted in local mutable collection
bindings, while still inspecting index expressions for forbidden effects.
Field-rooted mutation is outside the new allowance. Regression cases cover
local collection writes, global writes, effectful index expressions, and a
resolver using local marks.

This is a checker capability improvement. No numerical primitive, PDE axiom,
or mathematical trust bypass is added to the VM.
