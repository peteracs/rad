# Affine cores: cross-paper clue and the unresolved localization problem

Status: analytic research notes and necessary scale tests; no PDE construction.

## Sources examined

The PDFs were downloaded on 2026-09-08. The local repository checkout is
`d0124689230b58b4f86e7b90ac59de06404b3b6b`.

| Source | SHA-256 | Sections used |
|---|---|---|
| [IPM](https://cims.nyu.edu/~tristanb/ipm.pdf) | `b3ebdbb8d9a93dcca5f3b3f8796e63f7f28b48e0b0e258b909110a4b69c72a12` | Introduction and Section 2 overview |
| [Boussinesq](https://cims.nyu.edu/~tristanb/boussinesq.pdf) | `895a628d1783bcb039374686f50b895b5f450f53b8ef8aa173523487a7a4a21b` | Sections 1, 3.1–3.3, 4, 8 |
| [Euler](https://cims.nyu.edu/~tristanb/euler.pdf) | See README | Sections listed in VISCOSITY_AUDIT |

The `affinecore` subtree formalizes an inviscid Boussinesq construction.
Its existence in the repository is not evidence of a viscous theorem.
The checkout was inspected, not rebuilt or independently kernel-replayed.

## The reusable structure

IPM supplies transported phases, nested Fourier smoothing, retained cutoff
commutators, and increasing control of mixed derivatives. Boussinesq supplies
an exactly affine core, a two-field growth mechanism, and a rotation that
returns vorticity amplitude to zero while preserving the scalar gradient.
Euler supplies the variable radial coefficients, nonlinear material maps,
and compact recovery of the physical force. None supplies the missing
positive-viscosity localization estimate.

In Boussinesq, F(s)=s near the origin and the cutoff is one there. The
temperature and velocity are exactly affine on a central region, and the
vorticity is constant. Thus their ordinary Laplacians vanish in that region.
This is an important limitation on the earlier frozen-Fourier calculation:
one cannot infer nonzero pointwise diffusion in an affine core from the
carrier frequency alone. The paper's actual wave is not one Fourier mode.

Diffusion still acts in the nonaffine regions. Exact compact localization
and its influence on the core require control. The natural heat timescale
at distance R is R^2/nu. A short-time shielding strategy would seek
nu * duration / R^2 << 1. This is a scale test for that strategy, not a
necessary-and-sufficient nonlinear regularity criterion.

## The published Boussinesq schedule fails this shielding test

Let L=lambda_(q-1), for q>=2. Equations (3.7), (3.10), the growth-scale
bound in Theorem 3.2, and Lemma 4.3 give

    ell_q = L^(-3), K_q <= B L^(1/16),
    support_radius_q <= B L^(-47/16),
    Gamma_q <= sigma_(q-1) <= sqrt(C_sigma) L^(1/16).

There is an explicit transition interval of length Gamma_q^(-1). Therefore,
using even the optimistic full-support radius rather than the smaller core,

    nu * transition_duration / support_radius_q^2
        >= nu / (B^2 sqrt(C_sigma)) * L^(93/16).

Every fixed positive nu yields an unbounded lower bound as L grows. The
affine plateau does not make this unchanged schedule a small-diffusion
perturbation. This does not rule out a mechanism with strong diffusion or
new localization; it rules out justifying this schedule by the stated
short-time shielding estimate. The constants are fixed before L.

## A nonempty scale window for a genuinely three-dimensional core

Try a localized three-dimensional affine core with strain S=L^s and radius
R=L^(-r), duration comparable to 1/S, and velocity size comparable to SR.
Assume a fixed-shape localization with gradients of size S on volume R^3.
This is an ansatz for dimension counting, not an existing divergence-free
matched solution. It gives

    diffusion exposure: nu/(S R^2) = nu L^(2r-s),
    local kinetic energy: S^2 R^5 = L^(2s-5r),
    local integrated viscous loss per stage: nu S R^3 = nu L^(s-3r).

Consequently

    2r < s < (5/2)r

allows vanishing exposure, vanishing local energy, and vanishing per-stage
viscous loss while SR grows. Geometrically increasing L gives summable
durations and summable local losses for these strict exponents. These
facts concern the ansatz terms; they do not control global interactions.

For example, r=2 and s=9/2 give exponents -1/2, -1, and -3/2 respectively.
Writing S approximately (T-t)^(-1) instead, R approximately (T-t)^(4/9)
gives local velocity growth (T-t)^(-5/9). This escapes the bounded-velocity
obstruction at the level of scaling only. The old off-axis axisymmetric
geometry must also be abandoned, as established in VISCOSITY_AUDIT.

`core_scale_screen.rad` checks the strict exponent inequalities using exact
integers, evaluates a finite portfolio in isolated worlds, and checks the
published 93/16 exponent. A feasible entry is explicitly NOT a blowup
certificate. The infinite power-limit claims are the elementary analytic
argument written above, not inferred from sampling large frequencies.

## The missing lemma is global matching, not exponent arithmetic

A global affine field u=A(t)x has Delta u=0, but nonzero A gives infinite
energy on R^3. It therefore does not solve the prize problem. Cutting it
off at radius R requires a solenoidal correction and creates residuals.
The generic sizes are S^2 R for time/nonlinear terms and nu S/R for
viscosity; for r=2,s=9/2 these grow as L^7 and nu L^(13/2).
Such terms cannot be dismissed as an admissible uniformly smooth force.

A successful matching lemma would construct the surrounding flow and
pressure so that these large terms cancel in the full vector equation,
with actual residual bounds in every mixed derivative. Pressure is nonlocal;
boundary errors cannot be treated as only heat leakage. Incompressibility,
all-order activation, matching to earlier stages, and the infinite-stage
limit remain open obligations. No matching lemma is proved here.

The concrete next attempt is to compute the full solenoidal cutoff residual
for a chosen trace-free affine matrix family, including its pressure-free
curl. This can expose which terms are gradients and which require a real
dynamical correction, before launching any parameter optimization.
