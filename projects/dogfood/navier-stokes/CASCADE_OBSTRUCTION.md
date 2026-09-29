# Why the certified segments do not repeat as a blowup cascade

This audit concerns the five-wave datum v and the prescribed all-one force
in `SOLUTION_SEGMENT.md`. It does not rule out smooth-forced Navier-Stokes
blowup. It rules out specific proposed conclusions from our short segments.
The calculations are exact; the projection and energy arguments below are
written analytic lemmas, not formalized PDE theorems in RAD.

## A fixed bounded tube cannot prove blowup

Our segment certificate bounds ||u||_(H^3) by B=2048. While this bound holds,
E(u) <= B^2/2. Thus remaining in this same tube through an accumulation time
cannot yield unbounded enstrophy; it also supplies the regularity bound for
local continuation. A positive constant lower bound on E' over a finite time
only gives a finite increase. It does not imply an infinite increase.

Moreover, the demonstrated entrance/exit estimate is

    epsilon_next <= epsilon + eta,  eta = 2^-19.

It provides no upper bound epsilon_next <= epsilon. This is a failure of
the estimate to close, not a proof that the actual distance always grows.
Shrinking time steps can make the cumulative errors summable, but if this
keeps the entire solution in the same bounded H^3 tube it still does not
produce a singularity. Simply partitioning a short smooth interval into
infinitely many pieces creates no cascade.

## A Fourier projection blocks an immediate scale reset

A tempting next target is the Navier-Stokes-scaled periodic seed
w(x)=2 v(2x). Let S be the complete signed Fourier support of v. Its five
positive wavevectors are (1,0,0), (0,1,0), (1,1,0), (0,0,1), (0,1,1).
The sets S and 2S are disjoint. Consequently P_S w=0, even if w is
translated or its Fourier phases are changed.

Our actual segment endpoint satisfies ||u-v||_(H^3) <= delta=2^-18.
Orthogonal Fourier projection is a contraction in H^3, so

    ||u-w||_(H^3) >= ||P_S u||_(H^3)
                   >= ||v||_(H^3)-delta > 640-delta.

This endpoint cannot enter even a radius-one H^3 neighborhood of the doubled
seed. The same support argument works for integer dilations >=2. It is not
an exclusion of next profiles that retain the old modes or of localized
multiscale configurations.

## Global energy blocks a doubled full-torus seed at any time

There is a stronger obstruction than the short residence time. With normalized
torus measure, ||v||_2^2=24576, so ||v||_2<157. The prescribed force has

    integral_0^infinity ||f(t)||_2 dt
      <= 2 sum_(q>=1) a_q delta_q
       = 2 sum_(q>=1) 2^(5-q^2-q)
      <= 16 + 64/63 < 18.

Here beta<=1, the three cosine polarizations have norm sqrt(3/2)<2,
the first pulse contributes at most 16, and the second contributes at most
1. Ratios of consecutive contributions from q>=2 are at most 1/64.
The estimate covers the actual infinite force schedule, not just its first
two pulses.

The smooth energy identity implies

    ||u(t)||_2 <= ||v||_2 + integral_0^t ||f(s)||_2 ds < 175

at every time the smooth solution exists. One obtains this inequality at
zero norm by regularization, or directly from the standard energy estimate.
Viscosity only improves the bound.

Periodic dilation preserves the spatial average of |v|^2, so
||2v(2x)||_2^2=4*24576>312^2. Any state within H^3 distance 1 of this
doubled seed has L^2 norm greater than 311, contradicting the bound 175.
Translations and phases do not alter this obstruction. It excludes the
doubled full-domain copy at every smooth time, not just our tiny segment.
It does not exclude high-frequency growth with bounded kinetic energy.

## What a viable repair would have to change

The full-domain doubling target is retired for this prescribed force and
initial datum. More searching for that endpoint cannot succeed. Merely
rescaling the force would also change the initial-value problem and lose
the established force schedule unless checked anew.

Two concrete requirements remain for a different construction:

1. Transfer must respect the kinetic-energy bound. For a localized template
   of comparable shape, energy scales as amplitude squared times volume.
   Doubling amplitude therefore requires an appropriate decrease in occupied
   volume if the energy of that component is not to increase. The usual
   three-dimensional spatial dilation by two reduces localized volume by
   eight and scales its energy by one half, whereas a repeated periodic
   full-domain pattern increases energy by four. This is a scaling identity,
   not a localized PDE construction or a guarantee about cross terms.
2. An error bound in the new normalized profile coordinates must close. One
   sufficient estimate is epsilon_next <= rho epsilon + d, with rho<1 and
   d <= (1-rho) epsilon_max. Our current estimate has rho=1 and d>0.
   RAD checks that rho=1/2, d=1, epsilon_max=2 would close arithmetically;
   no such contraction for the fluid has been proved. This sufficient
   condition is not claimed necessary for every possible blowup proof.

A localized transfer has to be generated by the actual equation with the
fixed admissible force, including pressure, diffusion and arbitrary tails.
Nothing in this audit establishes that transfer. The useful new result is
an exact exclusion of a tempting false repair, together with explicit
energy and error inequalities any replacement must address. WHY cannot
convert these requirements into an existence proof.

## RAD and independent checks

`cascade_obstruction.rad` checks the Fourier separator, seed norm thresholds,
infinite-force impulse bound's geometric-series arithmetic, and reset
inequalities. WHY records the checked diagnosis. The independent verifier
recomputes the norms from the five waves and checks the geometric exponent
identity and the provenance. Forged transfer and reset claims are rejected.

```powershell
python -X utf8 projects/dogfood/navier-stokes/verify_cascade_obstruction.py
```

No extra packages are needed for this verifier. No kernel changes were made.
