# A prescribed smooth force, with a causal velocity derivative

This construction supplies an actual globally smooth periodic force for every
allowed sequence of pulse controls. It does not supply a blowup trajectory.
RAD checks the finite algebra and records why a local growth probe succeeds;
the infinite smoothness and local existence arguments below are written
analytic proofs, not theorems formalized in RAD's kernel.

## Fix the force before choosing a velocity trajectory

Work on the torus of side length 2 pi, with viscosity 1. For integers q >= 1 set

\[
t_q=1-2^{-(q-1)},\quad \delta_q=2^{-q},\quad
k_q=2^{q-1},\quad a_q=2^{5-q^2}.
\]

Let beta(s) = exp(4 - 1/s - 1/(1-s)) for 0 < s < 1, and zero otherwise.
For constant controls |c_(q,j)| <= 1, prescribe

\[
f_q(t,x)=a_q\beta(2^q(t-t_q))
  \sum_{j=0}^2 c_{q,j}\cos(k_qx_j)e_{(j+1)\bmod3},\qquad
f=\sum_{q\ge1}f_q.
\]

The concrete selected schedule has all c_(q,j) = 1. Each summand is
divergence free and has zero spatial mean. The pulse intervals have disjoint
interiors and accumulate at time 1. Extend the force by zero outside [0,1].
Controls may differ between pulses, but each is constant during its pulse.
Arbitrary time-dependent feedback is not covered by the estimates below.

### Proof of smoothness, including the accumulation time

The fixed bump is smooth and flat at both endpoints: its derivatives are
polynomials in reciprocal distances to the endpoints times a decaying
exponential. Thus C_m = sup |beta^(m)| is finite for every m.
For a spatial multi-index alpha, put d = |alpha| + m. Then

\[
\|\partial_x^\alpha\partial_t^m f_q\|_\infty
 \le 3C_m 2^{5-q^2+dq}.
\]

This bounds the actual prescribed force, without a residual assumption.
If q >= d+1, then -q^2+dq <= -q. Consequently, for Q >= d+1,

\[
\sum_{q\ge Q}\|\partial_x^\alpha\partial_t^m f_q\|_\infty
 \le 3C_m 2^{6-Q}.
\]

Every derivative series converges uniformly. Termwise differentiation gives
a globally smooth force whose derivatives all vanish at time 1. Every finite
seam is flat as well. Completing the square also gives

\[
5-q^2+dq\le5+\lfloor d^2/4\rfloor.
\]

Since only one pulse is nonzero at a time, a uniform bound is
3 C_m 2^(5 + floor(d^2/4)). Constants depend on the derivative order, as they
may, but not on the stage or control sequence. RAD checks the exponent
identities and 676 instances; the displayed argument proves the all-integer
statement. The executable descriptor's finite integer limits do not replace
that argument.

These forces meet the periodic smoothness and temporal decay requirements
of the forced target. For the unit torus, set L = 2 pi and rescale
u_unit(x,t) = L u(Lx,L^2 t), f_unit(x,t) = L^3 f(Lx,L^2 t).
Viscosity is unchanged. The finite rescaling preserves smoothness and compact
temporal support. This is not a construction on R^3 with spatial decay.

## Velocity is constrained by the PDE

Choose smooth divergence-free initial data
u_0 = 8(cos z, cos x, cos y). The initial-value problem is

\[
u_t=\Delta u-\mathbb P\operatorname{div}(u\otimes u)+f,
\qquad u(0)=u_0.
\]

`dictated_velocity_rate` computes this full instantaneous Fourier derivative,
including modes absent from its input velocity. It does not accept an
independently chosen velocity derivative. A proposed trajectory disagreeing
with this derivative must be rejected; its residual cannot be added to the
prescribed force to repair the disagreement.

### A quantitative local solution exists

This routine local existence check establishes that the force-first problem
really defines a PDE evolution. It is not evidence for singularity.
Use the normalized Fourier H^2 norm with squared weights (1+|k|^2)^2.
The initial norm squared is 384 < 400. At each time, the force obeys

\[
\|f(t)\|_{H^2}\le2a_q(1+k_q^2)
 \le4a_qk_q^2=2^{6-(q-1)^2}\le64.
\]

An explicit product constant follows from
S = sum_(k in Z^3) (1+|k|^2)^(-2) <= 53 < 64: the maximum-coordinate shell
n has 24n^2+2 points and contributes at most 26/n^2; sum_(n>=1) n^(-2) <= 2.
Fourier convolution, the inequality
1+|p+q|^2 <= 2(1+|p|^2)+2(1+|q|^2), and Young's inequality therefore give

\[
\|u\otimes v\|_{H^2}\le32\|u\|_{H^2}\|v\|_{H^2}.
\]

The heat/Leray/divergence multiplier has norm at most (t-s)^(-1/2).
The mild bilinear operator consequently has norm at most 64 sqrt(T).
Take T = 2^(-28); this constant is 1/256. The linear part has norm at most
20 + 64T <= 21. In the radius-42 ball of C([0,T]; H^2),

\[
21+42^2/256\le42,\qquad 2\cdot42/256=21/64<1/2.
\]

The contraction mapping theorem gives a unique mild solution in this ball.
RAD checks these numerical inequalities, not the functional analytic theorem.
This time interval is much shorter than a pulse, and supplies no continuation
to time 1 or to the growth probe at time 1/4.

## What WHY identifies

`force_first.rad` uses 27 isolated bounded-control choices with a checked
resolver. Its query is the vector field at u_* = 8(cos z, cos x, cos y),
at the first pulse's peak t = 1/4. This is a specified state query, not a
claim that the initial-value solution reaches that state at that time.

For normalized enstrophy E = ||curl u||_2^2 / 2, the exact decomposition is

\[
E(u_*)=48,\qquad
E'(u_*)=64(c_{q,0}+c_{q,1}+c_{q,2})-96.
\]

Nonlinear enstrophy production is zero at this query. All-one controls give
E' = 96, entirely because prescribed forcing exceeds viscous loss. Twelve
initially absent derivative modes are retained by the computation. The WHY
evidence labels the gain as direct forcing and rejects a forged claim of
repeatable amplification.

This changes the remaining question precisely: force smoothness is proved
for this family, but the dynamics must produce sustained nonlinear transfer
as the late pulses become small. The local control winner is not evidence
that this schedule does that. Neither reachability of the sampled state,
stage-to-stage velocity closure, nor blowup is established. A valid next
search must constrain actual solution segments and their inherited full
states, while keeping the prescribed force family fixed.

## Reproduction

- `smooth_force_schedule.rad`: pulse descriptors and exact exponent bounds.
- `forced_fourier.rad`: full projected Navier-Stokes velocity derivative.
- `force_first.rad`: causal control comparison and WHY evidence.
- `force_schedule_check.rad`: bounds, local existence arithmetic and RHS output.
- `verify_force_first.py`: independent symbolic Fourier checks, all 27 controls,
  forged-claim rejection, deterministic workers and record/replay.

Run with SymPy available:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_force_first.py
```

All listed checks pass. No kernel feature was needed or changed for this
application. No infinite velocity evolution or Millennium solution is certified.
