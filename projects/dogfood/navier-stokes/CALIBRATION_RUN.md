# Adaptive RAD fork calibration: 193 rounds, 9,248 checked cases

This run changes actual profile polarizations and certified finite-time PDE
bounds. It does not search by assigning convenient values to unproved
return-map constants. The finite search completed; no return map or blowup
construction was found or certified.

Receipts are in `calibration_run/candidates.jsonl`, `summary.json`, `audit.json`
and the 193 `why_*.txt` files. The runner is resumable and stops when this
finite parameter box is exhausted. It is not running in the background.

## An exact obstruction redirected the search

For the original five-wave polarizations, vary all five real amplitudes
a,b,c,d,e of the normalized donor d and set r=2 P_(|k|^2>2)N(d).
Full symbolic Fourier convolution gives

    S(r) = -(19/10)a^2 c^2 e^2 -(51/10)c^2 d^2 e^2 <= 0.

Thus no amount of amplitude-only tuning makes that isolated receiver have
positive nonlinear enstrophy production. This is an all-parameter algebraic
identity, not an unsuccessful finite search. `build_receiver_calibration.py`
records its coefficients in `receiver_polynomials.*`.

The next model varies polarizations instead. With physical amplitude 64,

    v=64[cos x(0,1,1)+cos y(1,0,1)+sin(x+y)(1,-1,alpha)
         +cos z(1,1,0)+sin(y+z)(beta,1,-1)].

Every field is divergence free, smooth and has finite periodic energy.
The prescribed all-one force schedule stays fixed. This is a new initial
datum family; it is not a claim that the earlier trajectory reaches these
data. `build_polarization_calibration.py` derives exact polynomials for donor
and receiver production, receiver loss/norm, and donor Sobolev norms from
the full Fourier operator, including the pressure projection.

## Forks, iterations and selection

The search covers alpha,beta in {-2,-7/4,...,2}, 16 H^7-ball radius multipliers
from 9/8 through 24/8, and two proved existence estimates. This is
17*17*16*2=9,248 distinct cases.

Each round runs up to 48 isolated `fork_with` candidates through
`simulate_many`, checks fork isolation and allowed mutations, and submits
their evidence to a resolver that recomputes every certificate. WHY records
the accepted and rejected proposals. The Python coordinator uses the current
winner's limiting bound to prioritize nearby polarizations and radii, while
reserving part of each batch for unexplored cases. No case is repeated in
the search. Complete coverage took 193 rounds.

There are 20 polarization pairs with positive nonlinear production in both
donor and receiver, giving 640 successful finite-transfer bound cases across
the two methods and 16 radii. Positive production alone is not positive net
enstrophy derivative: viscosity is checked separately for the selected field.

The objective first maximizes the certified duration, then minimizes the
isolated receiver's activation amplitude D(r)/S(r). This is an explicit
finite-grid objective, not a claim of a globally optimal fluid construction.

## The calibrated finite-time estimates

Let M be the rounded-up exact initial H^7 norm, B the chosen radius,
H=||u||_(H^7), and F=32768 the established force bound.

Method 0 uses the earlier mild-solution estimate, with bilinear constant
2048 sqrt(T), checking both ball inclusion and contraction.

Method 1 improves the lifespan estimate using the transport cancellation.
For the Fourier weight w_s(k)=(1+|k|^2)^(s/2), the mean-value theorem gives

    |w_s(p+q)-w_s(q)|
       <=s*2^(s-2)|p|[w_(s-1)(p)+w_(s-1)(q)].

The commutator between w_s(D) and u dot grad is consequently bounded in L^2
by 16*s*2^(s-2)||u||_(H^s)^2, using Young's inequality and the Fourier
gradient l^1 bound by 8||u||_(H^s), for s>=3. The highest transport term
cancels in the energy identity by incompressibility. At s=7 this gives

    H' <=3584 H^2+F.

Viscosity only improves this estimate. Therefore

    2(3584 B^2+F)T <= B-M

keeps H strictly below B by a first-exit argument. Local existence and
continuation in H^7 then supply the actual solution on this interval.
This written analytic argument controls all Fourier tails; it is not a
RAD-formalized PDE theorem or a contraction of the full return map.

For either method, define

    X=B+256 B^2+32768,
    C=64(B+M)X+256M^2,
    Q=P_(|k|^2>2)N(v), r=Q/2048.

The H^7 bounds imply ||u(t)-v||_(H^4)<=Xt. Comparing the actual solution
with its forced first Picard profile and then with tQ in the receiving band
gives, exactly as in `RECEIVER_PROFILE.md`,

    ||P_>u(t)-tQ||_(H^3)<=C t^2.

The calibrated cases verify ||r||_2>=1, CT<=512 and XT<=1. Hence

    ||P_>u(t)/t-Q||_(H^3)/||Q||_(H^3)<=Ct/2048<=1/4.

No force residual is added to the prescribed force. The chosen dyadic time
is determined by these inequalities; it is not an unconstrained input.

## The selected field and what it actually proves

The winner has alpha=3/2, beta=-1/4, M=5464, B=6147, and

    T=2^-44, X=9673154819, C=7188167681573952.

Its relative receiving-profile error bound is about 0.199512 at T. This
interval is 2^54 times the old 2^-98 interval, but still only about
5.68e-14 time units. Its limiting bound is profile error, not existence.

An independent direct RAD Fourier evaluation of the selected receiver gives

    S(r)=193/160 >0, D(r)=3565/16,
    D(r)/S(r)=35650/193 approximately 184.715.

For the selected donor, S(v)=163840 and D(v)=64000. All donor shapes in this
box have H^3 norm below 1024 initially; XT<1 gives the conservative bound
||u||_(H^3)<=1025. The continuity and force-work estimates then give

    E'(u) >=99840-(24*1025^2+2*1025)Xt-1025 >65536

on the selected interval. Also ||grad v||_2>=sqrt(7*4096)>128 and
||u||_2<256 there, so the same early small-force estimate proves decreasing
total kinetic energy. The receiving band has positive energy and net
nonlinear influx, with all omitted modes included in the error bound.

## Why this is still not a return map

The leading receiver amplitude is A=2048T=2^-33. It is far below its isolated
activation threshold 35650/193. This comparison is about the leading shape;
we can also check the actual receiving band independently of its approximation.
The calibrated bounds imply

    ||P_>u(t)||_(H^3) <=t(128M^2+512)<=1/4.

For any such high-band field h, with viscosity 1 and no force in that band,

    S(h)-||Delta h||_2^2
      <=(8||h||_(H^3)-3)||grad h||_2^2 <0

when h is nonzero. Thus even the actual small receiver is not yet an
autonomously amplifying state. Its full evolution is fed by the retained
donor. Autonomous receiver amplification is not necessary for every possible
cascade, but cannot be asserted for this experiment.

There is a uniform limit to further tuning inside these certificates.
Since C>=64(B+M)X and CT<=512,

    XT<=8/(B+M)<2^-10

throughout every accepted case (M>4096). Because the initial high band is
empty and its H^3 weight is at least 64, K_>(t)<2^-27. The total kinetic
energy is greater than 2^13, so its receiving-band fraction is below 2^-40.
This excludes a large-energy handoff within these intervals, regardless of
which grid radius or existence estimate wins. It does not exclude later
transfer or a localized singularity involving little kinetic energy.

The quantities Gamma, kappa and an all-stage profile inclusion from
`RETURN_MAP_HOLES.md` have not been calibrated into proved return-map bounds.
The new finite error estimate supplies part of a residual/normalization
budget, but no uniform family of actual longer stages. More copies of the
same finite-box run would not change that fact. The next substantive model
would need evolving central trajectories over transfer times, not merely
another choice of constants around the first Taylor profile.

## RAD extension and verification

The selected rational receiver did not fit the old Fourier input grid. The
research library `forced_fourier.rad` now allows scales through 512 and
coefficient numerators through 8192. Its fixed 48-mode, coordinate-8 and
viscosity-16 limits imply a conservative bound below 6e17 on every intermediate
in the final divergence check, safely inside int64. No kernel axiom or
proof bypass was added. This domain extension concerns the Fourier operator;
other applications must still respect their own arithmetic bounds.

`verify_forced_fourier.py` passes 16 cases / 516 complete outputs, six invalid
input rejections, and the explicit overflow bound. `verify_calibration.py`
independently checks the complete symbolic polarization model, all 9,248
receipts, the selected full Fourier field, the PDE-bound arithmetic, worker
determinism, record/replay and forged-return-map rejection. The continuum
lemmas remain written analytic arguments outside RAD's kernel.

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/run_calibration.py --resume
python -X utf8 projects/dogfood/navier-stokes/verify_calibration.py
python -X utf8 projects/dogfood/navier-stokes/verify_forced_fourier.py
```

The resume command recognizes completed coverage and preserves the receipts.
The run is complete for this search model. The Millennium objective is not.
