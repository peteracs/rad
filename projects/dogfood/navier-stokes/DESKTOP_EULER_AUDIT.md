# Local Euler manuscript: physical-viscosity transfer audit

2026-09-09. No Navier–Stokes blowup proof has been found.

## Source and scope

Read `C:/Users/pxp12/Desktop/euler.pdf`, 45 pages, title *Finite Time
Blowup for the Euler Equation*, author line `OPENAI`.
SHA-256: `714dd538ed6cdb5c4d7e80e436d2a3347e6246437fc9dc7da017c44b0caa6703`.
Its provenance and theorem have **not** been independently verified.
It is different from our earlier 112-page smooth-forced Euler source
(`97ef408bff09b4f6ed9f3867734d1eb2245f3f34e6334b28136c84c02d0ae8d8`).
Page-numbered extraction is at
`D:/Temp/rad-fluid-research/desktop-euler.txt`.

The local manuscript claims unforced Euler breakdown from smooth compactly
supported velocity (Theorem 1.1, p. 1). This is not the positive-viscosity
problem in Clay's alternatives C/D. The useful new candidate mechanism is:

* Transverse oscillations in material coordinates, equations (2.3)–(2.4), pp. 2–3.
* History boundary-value problems with a one-sided upper pressure-Hessian
  bound; mean pressure included, Sections 3.3–3.4, pp. 10–15.
* Correction to an exact Euler evolution, Lemma 3.3, pp. 19–24.
* Claimed amplification **and transfer of the next frame**, Proposition 4.1
  and Section 4.6, pp. 27–35.
* Explicit scales and an induction maintaining those hypotheses,
  Sections 5.3–5.7, pp. 37–43.

These are manuscript claims to audit, not newly certified PDE lemmas.
The present audit checks the attempted viscosity transfer, not every step
of the Euler proof.

## Physical viscosity is absent from the claimed exact equation

The viscosity in (3.50), p. 20, is an auxiliary term
`nu * Delta_(y,theta) e` on the **correction only**, removed by taking
nu to zero. It does not diffuse the approximate packet or the parent flow.
Keeping this parameter positive would not turn (3.46) into Navier–Stokes.
The lifted Laplacian also differs from the physical, material-coordinate
Laplacian.

For comparison, for actual Navier–Stokes particle trajectories, differentiation
of the momentum equation gives

    F_tt = [-H + nu * grad(Delta u) + grad f](t,X) F.

Thus the Euler identity `F_tt=-HF` in (2.2), and the history coercivity built
from it, cannot simply be reused. The new matrix terms require estimates;
they are not supplied by the one-sided bound on H.

## Exact principal-symbol diagnostic

Let lambda=k/ell and let m be the transported phase normal. Applying physical
diffusion to a packet's leading oscillatory factor produces

    nu Delta(A exp(i lambda phi))
      = -nu lambda^2 |grad phi|^2 A exp(i lambda phi)
        + terms with fewer phase derivatives.

With the paper's normalized m, the first nonzero phase harmonic therefore
has transverse principal equation

    m' = -M^T m,
    v' = -M v + 2m(m . Mv)/|m|^2 - d(t)v,
    d(t) = nu (k/ell)^2 |m(t)|^2.

For any given M,m, its solution is exactly the Euler principal solution
multiplied by `exp(-integral d)`. Higher harmonics have the harmonic number
squared multiplying d. This is an exact identity **for this principal ODE**;
the full localized viscous packet has further terms, not controlled here.

Transversality implies

    (log |v|)' <= ||M|| - d,
    (log (|m||v|))' <= 2||M|| - d.

This identifies a necessary change in the leading model rather than a
force residual that can be discarded.

## The supplied scale bounds do not make that diffusion perturbative

Write x=x_(j-1). Equations (5.6) and (5.11), pp. 37–39, give, for j>J,

    log k_j       = x/j^2,
    log k_(j-1)   = x/(j-1)^4,
    log h_(j-1)   = x/(j-1)^7,
    ell_j <= 1.

The manuscript's polynomial parent-map bounds can be expressed, after
enlarging a fixed exponent C and absorbing fixed constants at large scales,
as `||F|| <= k_(j-1)^C`. Since m=F^(-T)m0 and |m0|=1, this implies
`|m| >= k_(j-1)^(-C)`. Also `||M|| <= C_M h_(j-1)` is (5.3).
Consequently

    log[d/(nu h_(j-1))]
      >= x [2/j^2 - 2C/(j-1)^4 - 1/(j-1)^7].

Omitting ell^(-2) makes this lower bound weaker, hence safe. For every
C>=0 and j>=4(C+1), the bracket is **greater than 1/j^2**. Multiplying
the difference by the positive denominator j^2(j-1)^7 gives

    (j-1)^7 - 2C j^2 (j-1)^3 - j^2 > 0.

The independent symbolic verifier substitutes j=4(C+1)+n. All 36 nonzero
coefficients of the resulting polynomial in C,n are positive, including
the constant. This proves positivity for every C,n>=0; the conclusion is
not extrapolated from finite RAD samples.

Since x/j^2 tends to infinity under x_j=j^2 x_(j-1), for each fixed physical
nu>0 the lower bound on d eventually exceeds `2 C_M h_(j-1)`. The same
principal packet then decays in gradient amplitude. This excludes a
small-viscosity treatment using these unchanged uniform scale/map bounds.
It does **not** prove decay of the full localized Navier–Stokes solution,
disprove Euler blowup, or exclude changed scales and profiles. A construction
where diffusion participates in the leading balance needs a different analysis.

## RAD evidence and limits

`desktop_euler_audit.rad` performs:

* Full Fourier residual checks on eight transverse shears. A stationary
  Euler shear cos(kx)e_y requires force k^2 cos(kx)e_y at nu=1; choosing
  u_t=-k^2 u instead cancels the force exactly. Pressure cannot cancel its
  transverse Fourier coefficient. These periodic shears check an operator
  identity; they are not finite-energy whole-space examples.
* Sixteen isolated `fork_with` / `simulate_many` scale-bound cases.
* Resolver recomputation, isolation checks, and `why()` provenance.
* Rejection of a deliberately forged `navier_stokes_proved=true` claim.

`verify_desktop_euler_audit.py` independently checks the universal polynomial,
scalar damping conjugation, every RAD scale result, and forged-proof rejection.
It saves `desktop_euler_audit.json` and `desktop_euler_audit.why.txt`.
RAD verifies these finite identities; it does not certify the continuum
estimates above or the source manuscript. No kernel change was required.

Reproduce in PowerShell:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python projects/dogfood/navier-stokes/verify_desktop_euler_audit.py
```

## Concrete remaining proof obligation

The promising object supplied by this PDF is its **history problem plus
frame-transfer map**, rather than another finite Fourier moment region.
An adaptation must construct the corresponding physical viscous solution
segment, prove gain after its accumulated damping, and restore the next
history and frame hypotheses. If forcing is used to change that evolution,
its actual residual must satisfy all derivative bounds through the limiting
time. Neither the auxiliary-viscosity lemma nor the calculations here supply
these missing results. Returning `true` for them would conceal the problem,
not solve it.
