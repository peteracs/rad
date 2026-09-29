# Two actual solution intervals with nonlinear gain

The force is the fixed all-one schedule in `FORCE_FIRST_SCHEDULE.md`.
Viscosity is 1, and space is the torus of side length 2 pi with normalized
measure. This experiment changes the initial velocity from the cyclic probe
to the five-wave datum below. It does not claim the previous solution reaches
this datum. No force is reconstructed from a chosen velocity trajectory.

We prove two consecutive very short solution intervals with positive
nonlinear enstrophy production and net enstrophy growth. Their endpoint
conditions match by spending a finite error allowance. They are not
frequency-transfer stages or an invariant amplification region.

## Initial state and full nonlinear production

Set

\[
v=64\big[\cos x(0,1,1)+\cos y(1,0,1)
 +\sin(x+y)(1,-1,1)+\cos z(1,1,0)
 +\sin(y+z)(1,1,-1)\big].
\]

This is smooth, mean zero, divergence free and has finite periodic energy.
Let E(u) = ||curl u||_2^2/2, D(u) = ||Delta u||_2^2, and let N(u) denote
nonlinear enstrophy production. Exact full Fourier convolution and Leray
projection give

\[
N(v)=262144,\quad D(v)=61440,\quad N(v)-D(v)=200704.
\]

No finite-mode invariance is assumed. The convolution has nonzero output
outside the initial support. With the normalized Fourier H^s norm,

\[
\|v\|_{H^3}^2=105\cdot4096<1024^2,\qquad
\|v\|_{H^5}^2=825\cdot4096<4096^2.
\]

## An all-mode solution tube, not a sampled trajectory

Put tau = 2^(-99), eta = 2^(-19), and B = 2048. At time j tau define
the entrance set A_j by ||u-v||_(H^3) <= j eta, for j=0,1,2.
All fields in these sets are required to be divergence free. There is no
restriction to finite Fourier support, and arbitrary H^3 tails are allowed
within the stated norm budget.

For j=0,1, every entrance state in A_j has a unique local mild solution
on [j tau,(j+1) tau], with endpoint in A_(j+1). The actual trajectory starts
at v in A_0. Uniqueness identifies the second segment as the continuation
of the first, with no reset or replacement of its generated modes.

Here is an explicit analytic certificate. The lattice estimate
sum_(k in Z^3) (1+|k|^2)^(-2) <= 53 < 64 implies the H^3 product bound

\[
\|u\otimes w\|_{H^3}\le64\|u\|_{H^3}\|w\|_{H^3}.
\]

Indeed, the weight (1+|p+q|^2)^(3/2) is at most four times the sum of
the corresponding p and q weights. Apply Young's convolution inequality
and the Fourier l^1 bound by 8 times the H^3 norm. These estimates apply
to the complete infinite Fourier series.

The heat/Leray/divergence operator has H^3 multiplier bound
(t-s)^(-1/2). Hence the mild bilinear operator has norm at most
128 sqrt(tau) <= 128 * 2^(-49). The prescribed force has uniform H^3 norm
at most 256: each pulse has norm at most
2 a_q (1+k_q^2)^(3/2) <= 8 a_q k_q^3 <= 128,
and only one pulse is active at a time.

For an entrance state w in A_j, j <= 1, the linear mild part has norm at
most 1024+j eta+256 tau. Consequently

\[
1024+j\eta+256\tau+128\,2^{-49}B^2 < B,\qquad
2\cdot128\,2^{-49}B < 1/2.
\]

Contraction in the radius-B ball of C([0,tau];H^3) supplies the actual
solution. To bound its distance from v, use heat contraction on w-v and
||(exp(t Delta)-1)v||_(H^3) <= t ||v||_(H^5). For 0 <= t <= tau,

\[
\|u(j\tau+t)-v\|_{H^3}
 \le j\eta+4352\tau+128\,2^{-49}B^2
 <(j+1)\eta.
\]

This proves the endpoint implication A_0 -> A_1 -> A_2 and controls
every generated mode along both intervals. The standard mild-solution
framework is described in [Tao's local well-posedness notes](https://terrytao.wordpress.com/2018/09/16/254a-notes-1-local-well-posedness-of-the-navier-stokes-equations/).
The explicit constants and the forcing estimates here are our written
derivation. With this smooth datum and force, parabolic regularity yields
the smooth solution and its enstrophy identity. The identity also holds
in integrated form at the H^3 regularity used for the tube.

## Positive nonlinear production throughout the actual intervals

For all H^3 fields, ||grad u||_infinity <= 8 ||u||_(H^3). This follows
by Cauchy-Schwarz from
sum |k|^2/(1+|k|^2)^3 <= 53.
Write nonlinear production as the cubic integral

\[
N(u)=-\int\partial_k u_i\,\partial_k u_j\,\partial_j u_i.
\]

The trilinear form has absolute value at most the product of two H^3
norms and eight times the third. Telescoping its three factors, whenever
||u||_(H^3),||v||_(H^3) <= B and ||u-v||_(H^3) <= delta, gives

\[
|N(u)-N(v)|\le24 B^2\delta,\qquad
|D(u)-D(v)|\le2B\delta.
\]

Throughout both intervals delta <= 2 eta = 2^(-18). Thus the production
error is at most 384 and the dissipation error is less than 1.

The force does not cause this gain. On 0 <= t <= 2 tau < 1/128, the first
bump has s=2t <= 1/64, so beta(s) <= exp(-60) <= 2^(-60).
Its H^3 norm is therefore at most 128 * 2^(-60) < 1.
Force work in the enstrophy equation has absolute value at most B=2048.
It follows on the entire actual solution segment that

\[
N(u)\ge261760,\qquad
E'(u)\ge262144-384-61440-1-2048>131072.
\]

Each of the two intervals therefore increases enstrophy by at least
131072 tau = 2^(-82). This is a deliberately conservative, extremely
small quantitative gain, not a macroscopic amplification factor.

## What WHY can and cannot certify

`solution_segment.rad` checks exact full Fourier seed production, H^3/H^5
norms, the contraction and displacement arithmetic, and the production,
dissipation and force-work margins. Its checked resolver records

    actual solution: A_0 -> A_1 -> A_2
    radius budget: 0 -> eta -> 2 eta
    nonlinear production: >= 261760
    net enstrophy rate: >= 131072
    same-region closure: false

WHY supplies provenance for that evidence. It does not discover the
analytic semigroup estimates or independently formalize them. The bridge
to the actual infinite-dimensional PDE is the proof above, not an extra
trusted boolean or a claim that finite-mode simulation is exact evolution.

The two endpoint hypotheses match because the next tube is larger. There
is no proved reset of the error allowance, contraction toward a scaled
profile, or transfer to a finer characteristic frequency. Reusing this
bound indefinitely accumulates error and eventually exhausts the positive
production margin. This failure is a limitation of this certificate; it
does not assert the actual solution must lose production at that point.

The application rejects forged claims that the same region closes or that
the net gain equals the seed's nonlinear production. No kernel extension
is needed. We now have a finite actual-solution gain argument, but still
no repeatable cascade stage and no Millennium proof.

## Reproduce

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_solution_segment.py
```

The verifier independently computes full Fourier production and Sobolev
norms, checks both tube steps with rational arithmetic, and tests WHY
provenance and both forged-claim rejections. The analytic lemmas remain
outside the RAD kernel.
