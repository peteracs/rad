# The coupled pair generates a second finer band

The original flow and its first receiving profile generate an explicit
second profile on the same actual Navier-Stokes solution. This is a second
finite transfer, not a scale-invariant repetition theorem.

Retain v, the prescribed smooth force, viscosity 1 and T=2^-98. Write
N(u)=-P div(u tensor u) and B(a,b)=N(a+b)-N(a)-N(b). The first receiver is
r=P_(|k|^2>2)N(v)/2048; its Fourier support has |k|^2<=6.

## The full second coefficient, including the original flow's evolution

The force and its time derivatives vanish at time 0. The actual initial
velocity derivatives are therefore

    F1=u'(0)=Delta v+N(v),
    F2=u''(0)=Delta F1+B(v,F1).

Set Q2=P_(|k|^2>6)F2/2. We use the complete F1, not just the first receiver.
Its support has |k|^2<=6, so P_>6 Delta F1=0. Decompose
F1=L+2048r, where L has |k|^2<=2. The mixed term B(v,L) has no support
above squared frequency 6: sums of lattice waves of squared norm at most 2
can exceed 6 only at squared norm 8, which requires identical parallel
norm-sqrt(2) waves. Their interaction vanishes by incompressibility.
Consequently

    Q2=1024 P_>6 B(v,r)=65536 P_>6 B(v/64,r).

This proves that the actual second high-band coefficient is precisely the
polarized original/receiver interaction. Independent full Fourier algebra
also checks the identity without using the support shortcut.

Q2 has 32 nonzero modes with 6<|k|^2<=14. For example,

    Q2_hat(1,3,2)=65536 i (-1,1,-1).

This coefficient lies beyond both the original band (maximum squared
frequency 2) and the first receiving band (maximum 6).

## Actual evolution, with a third-order all-mode remainder

A quantitative H^11 bound supplies the required time derivatives. The
initial norm obeys

    ||v||_(H^11)^2=537585*4096<65536^2.

The prescribed force obeys ||f||_(H^11)<=2^31. Indeed, each pulse is
bounded by 128 a_q k_q^11=2^(1-q^2+11q)<=2^31, and their interiors are
disjoint. The H^11 product constant is 2^14. In the radius-2^17 mild-solution
ball on [0,T], the bilinear ball contribution is
2^15 sqrt(T)*(2^17)^2=1, the Lipschitz constant is 2^-16, and the linear
part is at most 65536+2^31 T. Thus the ball maps into itself contractively.
This solution agrees with the previously established one by mild uniqueness.

Here are explicit bounds for the force derivatives used below. For the
fixed beta bump, e<3 and max_(x>0) x^j exp(-x)<=j^j imply

    ||beta'||_infinity <=2*81*4<2^10,
    ||beta''||_infinity <=81*(4*256+4*27)<2^17.

The first estimate bounds beta times its two reciprocal-square factors.
The second bounds the square of their difference and the two reciprocal-cube
derivatives. Endpoint values follow by flatness. Including the pulse time
scales and completing the quadratic exponents gives

    ||f||_(H^7)<=2^15,
    ||f_t||_(H^5)<=2^23,
    ||f_tt||_(H^3)<=2^28.

Using ||u||_(H^11)<=2^17 and the full Sobolev product bounds gives

    ||u_t||_(H^7)
      <=2^17+2^11*(2^17)^2+2^15 <2^46,
    ||u_tt||_(H^5)
      <=2^46+2*2^9*2^17*2^46+2^23 <2^74,
    ||u_ttt||_(H^3)
      <=2^74+2*2^7*2^17*2^74+2*2^7*(2^46)^2+2^28 <2^102.

The third derivative includes both the mixed u,u_tt terms and the two
u_t,u_t terms, as well as force derivatives. The estimates cover the entire
Fourier series. Taylor's integral remainder in H^3 yields

    ||u(t)-v-tF1-(t^2/2)F2||_(H^3)<=2^102 t^3.

Since the projection above frequency 6 kills v and F1,

    ||P_>6 u(t)-t^2 Q2||_(H^3)<=2^102 t^3.

The displayed coefficient of Q2 implies ||Q2||_(H^3)>=2^16, so for 0<t<=T,

    ||P_>6 u(t)/t^2-Q2||_(H^3)/||Q2||_(H^3)
       <=2^86 t<=2^-12.

This is a normalized bound for the whole second profile, including errors
from arbitrary modes outside its finite leading support. It is not an
assertion that the full solution occupies only these profiles.

## A second positive transfer of energy

At k=(1,3,2), the signed imaginary first component is at least
2^16 t^2-2^102 t^3>=2^15 t^2. Its conjugate pair supplies

    K_>6(t)>=2^30 t^4>0.

The force has no component above frequency 6 on this interval. Therefore
the integrated nonlinear flux into this band equals its energy gain plus
its viscous dissipation, and is at least 2^30 t^4. The previously proved
decrease of total kinetic energy still holds for this same solution.
At T, the conservative new-band energy lower bound is 2^-362.

## What this establishes, and what it does not

The coupled original/first-receiver structure produces a rigorously controlled
second receiving profile at finer frequencies. This answers the finite
generation question. The second profile's leading amplitude is t^2, whereas
the first's is t. We have not shown that it is a suitable seed for indefinite
amplification, that a fixed fraction transfers at each scale, or that stage
durations and errors close in an infinite iteration.

Successively computing Taylor coefficients is not a substitute for such a
theorem. The present coefficients describe a smooth short-time solution;
infinite frequency support alone is entirely compatible with smoothness.
No repeatable cascade or finite-time singularity is claimed.

## RAD and independent verification

`second_transfer.rad` computes the polarized source on the exact rational
grid, exports all 32 coefficients, and checks the stronger regularity and
normalized-error arithmetic. WHY records the proof dependencies and rejects
forged repetition or an overstated error bound.

`verify_second_transfer.py` independently builds F1 and F2 from full Fourier
convolution, compares every exported coefficient with P_>6 F2/2, and checks
the H^11 and force-derivative bounds. The continuum existence and Taylor
lemmas are the written arguments above, not RAD kernel formalizations.
The shared `coupled_basis.rad` module also has the coupled-evolution regression.

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_second_transfer.py
python -X utf8 projects/dogfood/navier-stokes/verify_coupled_modes.py
```
