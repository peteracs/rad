# Particular correction: principal cancellation and derivative preservation

2026-09-09. RAD independently verifies the principal cancellation algebra
below. The complete localized correction and its scale-uniform derivative
estimates remain unverified. No Lean execution is used.

## What the supplied construction actually cancels

The relevant source is `TangentProjection.projectedRhs`, used by
`NativePrincipalEquations.complexCopyCoefficients_principal_at`, then by
`CorrectionStep.native_cancellation_of_principal` (line 5927).
The last theorem explicitly retains good and Gaussian remainder terms.
It does not assert that the full updated nonlinear residual vanishes.

Let n be the phase normal, v the correction amplitude, K the local shear
operator, f the preceding harmonic residual, and delta the physical damping.
The source's principal equation is

    v' = -Kv + [(n.Kv - n'.v)/|n|^2] n - delta*v - P_n f,
    P_n f = f - (n.f)n/|n|^2.

Set pi=(n.Kv-n'.v+n.f)/|n|^2. For n nonzero this gives exactly

    v' + Kv + delta*v - pi*n = -f.                       (1)

Consequently the old harmonic source plus the principal residual of this
correction is zero. The scalar pi is the real normal pressure coefficient;
conversion to the complex pressure amplitude also uses the nonzero carrier
frequency, as in the source. It is not the complete global pressure field.
Differentiating n.v along this equation gives

    (n.v)' = -delta*(n.v).                               (2)

Zero initial tangency defect therefore remains zero. These identities hold
for arbitrary shear and source, not just a chosen numerical configuration.

`principal_cancellation.rad` represents all 16 scalar components of
n,n',v,Kv,f,delta as independent indeterminates in a free polynomial ring.
After multiplying by |n|^2, it checks all three components of (1) and (2)
coefficient by coefficient. It rejects omission of physical damping.
The nonzero-normal condition is needed to interpret the rational equation;
clearing denominators does not prove that geometric condition.

## Why the inverse constructor can supply the derivative

Once the geometric coefficient fields are supplied, the projected equation
has the form v'=A(t,xi)v+g(t,xi), with g=-P_n f. On a finite interval where
n stays nonzero and the coefficients are smooth, let Phi(t,s,xi) be the
fundamental solution of A. Then the zero-initial-value construction

    v(t,xi) = integral_0^t Phi(t,s,xi) g(s,xi) ds          (3)

really solves the equation: differentiating the upper limit contributes g(t),
and differentiating Phi contributes A(t)v(t). This is the variation-of-constants
argument, not the assignment of an arbitrary velocity derivative.
The supplied source uses a finite-path version on a common cover. Its cover,
transported source, cutoff, and quantitative bounds still need an independent
port; this audit does not identify an arbitrary integral (3) with that whole
localized constructor without those obligations.

## All-order parameter-derivative estimate for that inverse

On a compact parameter region and interval [0,T], suppose the derivatives of
A,g exist through order m. Let A_r,F_r bound the norms of their r-th parameter
derivatives, and Q_r bound the corresponding derivatives of v uniformly in
time. These can also be maxima over all orders up to r. For zero initial data,

    Q_0 <= exp(A_0*T)*T*F_0,
    Q_m <= exp(A_0*T)*T*
           [F_m + sum_(r=1)^m binomial(m,r) A_r Q_(m-r)]. (4)

Proof: differentiate v'=Av+g in m parameter directions. The term with no
derivative on A is A D^m v. The other terms are the Leibniz splits, with
r>=1 derivatives on A and m-r on v. Apply variation of constants to this
inhomogeneous equation and ||Phi(t,s)||<=exp(A_0(t-s)). Taking norms and
integrating over an interval of length at most T proves (4), inductively in m.
It uses no derivative of g above m. For the source's nonzero initial data,
the corresponding initial-jet norm must also be added inside the exponential
factor. Time and mixed derivatives then follow by differentiating the ODE,
with their required coefficient and source derivatives explicitly included.

Thus local smoothness and finite derivative bounds propagate through a
smooth finite-path inverse. This analytic argument is stated and justified
here; RAD's polynomial run does not certify the integral or Gronwall theorem.

## The remaining scale-uniform issue

Finite constants in (4) do not prove the weighted estimates needed for
infinitely many construction stages. In particular exp(A_0*T), inverse powers
of |n|, and differentiated phase/coordinate maps may depend on the physical
scale. Replacing them by uniform constants without proof would reintroduce
the central gap.

The source's claimed localized cancellation has the form

    harmonicResidual(corrected_wave) + old_source*carrier
      = (localGood + localGaussian)*carrier.

The **full** nonlinear update also contains interactions with existing waves,
the square of the correction, and the subsequent signed and mean updates.
The previous `nonlinear_residual_audit.rad` verifies that both cross terms and
the self-interaction must be retained. The new principal identity cannot erase
these terms. Their weighted derivative estimates are supplied in the source
through `ActualParticularStageControls`, `NativeDynamics`, `WaveData`, and
`CorrectionAnalyticStep.step`; they have not been established in this RAD port.

Therefore the verified conclusion is principal cancellation, with the local
inverse's derivative propagation explained by (3)–(4). Neither the complete
weighted invariant preservation nor the smooth-force blowup theorem is
certified by these checks.

Run `python projects/dogfood/navier-stokes/verify_principal_cancellation.py`.
The verifier checks RAD execution, causal WHY provenance in the saved output,
record/replay, and rejection of a missing damping term. Receipts are
`principal_cancellation.json` and `principal_cancellation.why.txt`.
