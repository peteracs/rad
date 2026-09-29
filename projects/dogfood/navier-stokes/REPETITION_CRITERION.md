# What would establish suitability for indefinite repetition

This is a sufficient theorem and a backward scalar-design calculation.
It is not an actual Navier-Stokes return map, a discovered blowup profile,
or a proof that our existing profiles meet the theorem's hypotheses.

The relevant object is a profile family together with a solution-map
inclusion theorem. A profile's instantaneous derivative, one accurate
Taylor approximation, or one successful transfer is insufficient.

## A precise sufficient criterion

Fix the actual prescribed globally smooth force and a smooth initial datum
on the periodic torus. Suppose there are entrance families C_q and times
t_q, with the following properties:

1. The initial state is in C_0, and each state reached in C_q generates a
   strong solution through the next stage under that same prescribed force.
   Its actual endpoint belongs to C_(q+1). The families include the retained
   original flow, pressure-compatible divergence-free velocity, generated
   modes, and a norm bound for arbitrary tails. No projection or deletion
   of modes replaces the endpoint. An inclusion for every state in C_q is
   sufficient; controlling precisely the reachable subfamily can also suffice.
2. At stage q the endpoint has energy K_q in a specified band with
   |k|>=k_0 lambda^q, where K_0>0. The evolution proves
   K_(q+1)>=theta K_q with theta>0. These are actual band-energy bounds,
   not chosen normalization constants.
3. In normalized profile coordinates, the endpoint error satisfies
   e_(q+1)<=rho e_q+d, with 0<=rho<1 and rho e_max+d<=e_max.
   The coordinate maps and constants are defined and valid at every stage.
   Initial membership includes e_0<=e_max.
4. Stage durations obey 0<t_(q+1)-t_q<=tau_0 beta^q, with 0<beta<1.
   The fixed force satisfies its full smoothness requirements through the
   accumulation time; stage rescaling is not permission to replace the force.

If lambda^2 theta>1, these hypotheses imply blowup. Induction gives

    e_q<=e_max, K_q>=K_0 theta^q,
    E(u(t_q)) >= k_0^2 lambda^(2q) K_q
               >= k_0^2 K_0 (lambda^2 theta)^q -> infinity.

Meanwhile the times increase to a finite T_*<=t_0+tau_0/(1-beta).
Thus the actual solution cannot extend smoothly through T_*. This is a
written conditional proof, not a formalized theorem in RAD's kernel.

Energy retention below one can suffice: high-frequency enstrophy weights
energy by frequency squared. Small kinetic energy alone does not disqualify
a concentrated profile. Conversely, lambda^2 theta<=1 only makes this
particular lower-bound argument inconclusive; it is not a no-blowup theorem.
This criterion is sufficient, not necessary for every possible blowup mechanism.

## Eliminate an error-radius search algebraically

Suppose a proposed analytic return-map estimate would give

    theta(e)>=theta_0-L e,       e_next<=rho e+d,

with L>0, d>0 and 0<=rho<1. A usable invariant radius must satisfy

    d/(1-rho) <= e_max < (theta_0-lambda^-2)/L.

Such a radius exists exactly when

    L d < (1-rho)(theta_0-lambda^-2).

This eliminates the scalar radius search. If the strict gap is positive,
choose the midpoint of the two bounds; both growth and error closure then
have margin. If it is nonpositive, no radius can make these particular
estimates support the desired conclusion. That does not say better PDE
estimates or another construction are impossible.

## A worked numerical target, explicitly not a proved fluid map

For illustrative design values

    lambda=2, theta_0=1/2, L=1, rho=1/2, d=1/16, beta=1/4,

the allowed interval is 1/8<=e_max<1/4. The midpoint e_max=3/16 gives

    e_next<=5/32<3/16,
    theta>=5/16,
    lambda^2 theta>=5/4>1,
    sum stage durations <=4 tau_0/3.

Thus those estimates WOULD certify error closure, decaying band-energy
allowances, increasing enstrophy lower bounds and finite-time accumulation,
IF they were proved for the actual solution map with initial membership and
all-stage applicability. We have not derived theta_0, L, rho or d for any
Navier-Stokes profile family. These numbers are a design example, not
measured performance of our 16-mode or 32-mode profiles.

For the same example with d=1/8, the error lower bound reaches the growth
upper bound and the strict interval disappears. This exposes exactly which
estimated defect budget would need improvement, without searching radii.

## Application to our existing results

The finite two-transfer calculation provides explicit receiving profiles,
accurate short-time approximations and positive energy transfer. It does
not provide a return map between a fixed normalized family at every scale.
In particular, its t and t^2 amplitudes are Taylor coefficients of one smooth
short-time trajectory; they are not evidence for uniform theta, rho or beta.

The condition above tells us what evidence would be decisive. It does not
discover the family or the missing nonlinear stability estimate. Calling a
scalar-feasible example a proved fluid return map would erase precisely the
hard hypothesis. The RAD application explicitly rejects that promotion.

## RAD evidence and limits

`repetition_gate.rad` eliminates the error radius for a dyadic parameter
family and checks 80 cases, including the exact defect threshold. It retains
the actual-solution-map status as unproved even for feasible scalar designs.
WHY records that separation and rejects a forged map/blowup claim.

`verify_repetition_gate.py` independently checks the rational intervals,
selected radii, energy/enstrophy margins, time summation and provenance.
No kernel extension, inferred PDE axiom or external proof placeholder is
accepted as a solution-map certificate.

Run `python -X utf8 projects/dogfood/navier-stokes/verify_repetition_gate.py`.
