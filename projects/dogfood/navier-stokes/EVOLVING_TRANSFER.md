# A finite nonlinear transfer around an evolving profile

This is a concrete replacement for the impossible doubled full-torus seed:
retain the old modes and follow their generated modes using a first Picard
profile. For the actual solution from `SOLUTION_SEGMENT.md`, we prove a
positive net nonlinear energy transfer beyond its initial frequency band,
decreasing total kinetic energy, and an all-mode error bound around this
evolving profile. It is a very short finite transfer, not a repeatable cascade.

All notation, the five-wave datum v, viscosity 1, normalized periodic norms,
and the prescribed all-one smooth force are unchanged. Write

    N(u) = -P div(u tensor u),  H(t) = exp(t Delta),  T = 2^-98.

Here N is the nonlinear velocity derivative, not the scalar enstrophy
production denoted N in some earlier notes. The established actual solution
on [0,T] satisfies ||u||_(H^3) <= B=2048 and
||u-v||_(H^3) <= delta=2^-18. This controls arbitrary Fourier tails.

## The evolving profile and its full error

Define the explicit first Picard profile

    W(t) = H(t)v + integral_0^t H(t-s) f(s) ds
                  + integral_0^t H(t-s) N(v) ds.

The first and third terms are finite Fourier sums with exponential time
coefficients. The second is the convolution of the already prescribed smooth
force with the heat flow. W is not asserted to solve the nonlinear equation;
its discrepancy is explicitly bounded. No residual is added to the force.

The mild equation and the same bounds as in the segment proof give

    ||u(t)-v||_(H^3) <= 4352 t + 2^29 sqrt(t).

The H^3 product and heat multiplier estimates then imply

    ||u(t)-W(t)||_(H^3)
      <= 128 (B+||v||_(H^3)) sqrt(t) sup_(s<=t)||u(s)-v||_(H^3)
      <= 2^19 sqrt(t) [4352 t + 2^29 sqrt(t)]
      <= 2^49 t,                                  0 <= t <= T.

The last step uses 4352 sqrt(t) <= 2^29. In particular the endpoint error
is at most 2^-49. This estimate covers every mode, not a truncated ODE.
It does not establish a contraction after resetting the profile at a new scale.

## A mode-specific functional resolves a transfer the H^3 bound cannot see

Take k=(2,1,0), so |k|^2=5, beyond the initial support |k|^2<=2.
Exact convolution with global pressure projection gives

    N(v)_k = (-1024/5, 2048/5, -2048).

The k coefficient of the initial datum and the prescribed force is zero
throughout [0,T]. In particular

    W_k(t) = (1-exp(-5t))/5 * N(v)_k.

The coarse H^3 error above is larger than this tiny new coefficient and
cannot establish its sign. Use instead the linear functional
ell(u) = -Re(u_hat(k)_z). Incompressibility writes each nonlinear Fourier
coefficient as -i P_k sum_p (k dot u_hat(p)) u_hat(k-p). Cauchy-Schwarz gives
the all-mode estimate

    |N(u)_k-N(v)_k|
      <= |k| (||u||_2+||v||_2) ||u-v||_2
      <= 3*4096*2^-18 = 3/64 < 1.

This bound includes every generated mode and arbitrary tails. The relevant
source b(t)=-Re(N(u(t))_k,z) consequently lies between 2047 and 2049.
For a(t)=-Re(u_hat(k,t)_z), the actual PDE yields

    a'(t)+5a(t)=b(t),  a(0)=0.

Since 5T<1/4, the heat weight is at least 3/4. Therefore

    a(t) >= 2047*(3/4)*t > 1024 t,
    |a(t)| <= 2049 t,
    a'(t) >= 2047-5*2049*t > 1024.

These statements concern the actual solution, not merely W. Moreover the
target coefficient's profile error is at most (3/64)t, by integrating the
source discrepancy. This is why the targeted functional succeeds where the
coarse whole-profile norm could not resolve the signal.

## Genuine net transfer with decreasing total energy

Let K_>(t)=||P_(|k|^2>2) u(t)||_2^2/2. Conjugate symmetry and the single
pair k,-k give

    K_>(t) >= a(t)^2 >= 2^20 t^2,  for 0<t<=T,

whereas K_>(0)=0. The high-band force vanishes on this interval. Its exact
energy identity consequently reads

    integral_0^t <u_>(s), P_> N(u(s))> ds
      = K_>(t) + integral_0^t ||grad u_>(s)||_2^2 ds
      >= 2^20 t^2.

Thus the integrated nonlinear flux into the previously empty high band is
strictly positive. This is stronger than observing a nonzero initial
derivative, but it is not a claim of pointwise monotone total high-band energy.

There is no conflict with the global energy bound. Initially ||grad v||_2=192.
The H^3 distance is below 1, so ||grad u||_2>191. The force has H^3 norm
below 1 on this early interval, as proved in `SOLUTION_SEGMENT.md`. Hence

    K'(t) = -||grad u||_2^2 + <u,f>
           < -191^2+2048 < 0.

The solution redistributes energy into finer modes while dissipating more
energy overall. It does not increase the amplitude of a full-domain copy.

## Scope of the result and WHY

At T=2^-98 the lower bound on high-band energy is only 2^-176. We have
certified neither a fixed fraction of energy transfer nor a significant
amplification factor. The new frequency is sqrt(5), beyond the original
sqrt(2); there is no proved iteration to arbitrarily large frequencies.

`evolving_transfer.rad` computes the exact projected source and checks the
error and energy margins. Its resolver and WHY distinguish:

- all-mode profile control from the sharper mode-specific sign certificate;
- direct forcing from nonlinear high-band flux;
- a finite transfer from an indefinitely repeatable scale transition.

The proof that these arithmetic quantities bound the infinite-dimensional
solution is the written analytic argument above, not a RAD kernel theorem.
WHY reports provenance; it did not independently discover or verify the
functional analytic lemmas. Forged source coefficients and repeatability
claims are rejected.

The useful next question is whether a moving profile can transfer a controlled
fraction to another band while retaining a normalized error margin. This
certificate does not answer it. We have a specific viable finite replacement
with actual nonlinear transfer, not a viable blowup construction.

## Reproduce

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_evolving_transfer.py
python -X utf8 projects/dogfood/navier-stokes/verify_solution_segment.py
```

The independent verifier recomputes the full Fourier source and all rational
inequalities, checks the exact target heat profile, and tests WHY provenance
and rejection of both forged claims. The second command covers the shared
seed-module refactor and the actual-solution segment certificate.
