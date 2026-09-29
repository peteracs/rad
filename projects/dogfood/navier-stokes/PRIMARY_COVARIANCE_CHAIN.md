# From the actual modal ODE to uniform covariance and signed jets

This is an analytic derivation with RAD-checked polynomial certificates. It
ports a connected part of the supplied construction: GrowingMode ->
PulseCovariance -> PrimaryCovarianceBounds -> SignedCopyBounds. Its hypotheses
are stated explicitly. Neither RAD replay nor reading Lean verifies the full
Navier–Stokes theorem. No Lean process is invoked.

## 1. An actual solution has a uniform growing-coordinate lower bound

On [0,ell], let (p,q) solve the continuous linear system

    p' = (lambda-d+e11)p + e12 q,
    q' = e21 p + (-lambda-d+e22)q.

Assume lambda>=g>0, |eij|<=C/S, C>=0, S>0, p(0)>0, q(0)=0.
Set Kc=4(C+1)/g, assume S>=2Kc, and set r=Kc/S<=1/2.
The barrier B=q^2-r^2 p^2 has, on q=sign*r*p,

    B' = 2r p^2 [sign*e21+r(e22-e11)-sign*r^2 e12-2lambda*r].

All scalar damping cancels exactly. For either sign, the bracket is at most

    (C/S)(1+r)^2-2g*r < 0.

In fact, if gap=2g*r-(C/S)(1+r)^2, then

    4S*gap = 23C+32 + C(1-2r)(5+2r) >= 23C+32 > 0.

RAD checks the boundary identity in free variables, all 16 coefficient-box
vertices on both boundaries, and the last identity. Affinity in each eij
extends the vertex bound to the whole box; these are not sampled states.

For completeness, continuous coefficients give a unique solution on this
compact interval. A nonzero solution cannot hit the zero vector: backwards
uniqueness would imply its initial value was zero. Initially B<0. At a first
zero of B, p cannot vanish, so the displayed strict negative derivative
contradicts a first crossing from B<0. Thus B<=0. If p vanished, B<=0 would
force q=0, also impossible. Continuity and p(0)>0 give p>0 throughout. Hence

    |q(t)| <= r p(t)  for every t in [0,ell].

Now let P>0 satisfy P'=(lambda-d_ref)P. If |d-d_ref|<=D/S and
ell<=Kslot*S, then

    |p'-(lambda-d_ref)p| <= [(D+2C)/S] p.

Since p,P>0, integrate the derivative of log(p/P). With theta=p(0)/P(0),

    exp(-(D+2C)Kslot) theta P(t) <= p(t)
        <= exp((D+2C)Kslot) theta P(t).

These constants precede S and the choice of continuous coefficient functions.
For the canonical normalization theta=1 and physical radial coordinate x=p+q,

    a P(t) <= x(t) <= A P(t),
    a=exp(-(D+2C)Kslot)/2,
    A=3 exp((D+2C)Kslot)/2.

Also, for y=h(p-q),

    |y/x-h| = |2h q/(p+q)| <= 4|h|r.

This supplies the actual solution's radial lower bound and a directional
error estimate. It does not assume positivity of the unknown solution.

Source: `GrowingMode.lean:100,128,365,430`;
`PrimaryCovarianceBounds.lean:554`.

## 2. Physical damping produces a Gaussian pulse

The source's explicit reference is

    s(t)=u/2+ut/ell, lambda_ref(t)=lambda0/sqrt(1+s(t)^2),
    d_ref(t)=lambda0(1+s(t)^2)/(1+u^2)^(3/2),
    rate(t)=lambda_ref(t)-d_ref(t),
    P(t)=exp(integral from ell/2 to t of rate(v) dv),

where lambda0,u,ell>0. This includes positive viscosity in the reference.
Differentiation with respect to s gives

    rate_s = -lambda0*s/(1+s^2)^(3/2)
             -2lambda0*s/(1+u^2)^(3/2).

On s in [u/2,3u/2], put

    c=lambda0*u/(1+u^2)^(3/2),
    Cg=3lambda0*u/2+3lambda0*u/(1+u^2)^(3/2).

Then -Cg<=rate_s<=-c<0. Because rate(ell/2)=0, integrating twice yields

    exp(-B(t-ell/2)^2/ell) <= P(t)
        <= exp(-b(t-ell/2)^2/ell),
    b=uc/2>0, B=uCg/2>0.

The same inequalities hold on either side of the midpoint, accounting for
the orientation of the integrals. On a fixed compact positive parameter
range, the constants can be chosen uniformly. In particular, the canonical
solution has the actual gain estimate

    x(ell/2)/x(0) >= (1/2) exp(b*ell/4-(D+2C)Kslot).

The pulse begins exponentially small and reaches a bounded peak. This is
not an assertion of growing total energy or a nonlinear cascade.

Source: `GaussianEnvelope.lean:134-283`, `ViscousPropagator.lean:419`.
The differentiation and integration here are analytic arguments, not
operations claimed to have been checked by the RAD polynomial programs.

## 3. The integrated covariance inherits quantitative cone margins

Write Rslot=sqrt(ell)>=1, m=ell/2, and choose the source cutoff psi with
|psi|<=1, supported in [ell/6,5ell/6], equal to 1 on [ell/3,2ell/3].
Set W=psi^2*x^2, mass=integral W, moment=integral |t-m|W. The previous
two-sided pulse bounds imply

    m0 Rslot <= mass <= M0 Rslot,
    moment <= A^2 Rslot^2/(2b),
    m0=a^2 exp(-B/18)/3 > 0,
    M0=A^2 sqrt(pi/(2b)).

For the lower bound, integrate only over [m-Rslot/6,m+Rslot/6]. This is in
the region where psi=1 because Rslot>=1, and the Gaussian squared is at
least exp(-B/18). For the upper bounds integrate the Gaussian majorant on
the whole real line, using integral |z|exp(-2b*z^2) dz=1/(2b).

If a component of the actual uncut direction v/x obeys

    |v(t)/x(t)-H0_ij| <= E/Rslot^2 + F |t-m|/Rslot^2,

its W-weighted mean obeys

    |Hbar_ij-H0_ij| <= (E+F Q)/Rslot,
    Q=A^2/(2b*m0).

This is a bound on the actual integral, not an assigned covariance margin.

Here is an explicit replacement for the source's compactness-only tolerance.
For a model matrix H0 and target T0, assume entries of both have magnitude
at most M>=1. Suppose, for an orientation eta in {-1,1},

    eta det(H0)>=d0>0,
    eta N_j(H0,T0)>=n0>0,

where N_j are Cramer's numerators. If every matrix and target entry changes
by at most rho, then each determinant or numerator changes by at most

    4M*rho+2rho^2.

RAD verifies both signs at all 256 vertices of the eight-variable box of
four original entries and four perturbations. The determinant difference
is multiaffine, so interpolation proves the bound throughout that box.
Cramer's numerators are determinants of column-replaced matrices and obey
the identical estimate. A fixed target is an allowed special case.

Choose

    0<rho<=min(1, min(d0,n0)/(4(2M+1))).

Then determinant and numerator retain their common orientation, with lower
bounds d0/2 and n0/2. Entries of the new matrix have magnitude <=M+1, so

    |det(Hbar)|>=d0/2,
    [Hbar^-1 T]_j >= n0/[4(M+1)^2] =: w0 >0.

The actual averaged matrix meets this tolerance as soon as
Rslot>=max(1,(E+F Q)/rho).

Finally the actual covariance is H=Hbar diag(c0,c1), with cj>0. Let
R=sqrt(S) and suppose lo<=R*cj<=hi. For target zeta*T with zeta>=0,

    |det(RH)| >= lo^2*d0/2,
    |R H_ij| <= hi*(M+1),
    [H^-1(zeta*T)]_j >= (w0/hi)*R*zeta.

RAD checks the determinant and Cramer numerator scaling identities. The
inequalities follow by positivity. There is no division by zeta.

For the source chart, ell=2r0/c_time, 1/Tg<=S*c_time<=1, and
cj=kappa*c_time*mass_j. Consequently

    lo=kappa*m0*sqrt(2r0/Tg),
    hi=kappa*M0*sqrt(2r0)

work for every admissible band. Rslot and R are different radii; the proof
uses their displayed relationship and does not identify them.

Source: `PulseCovariance.lean:130,224,233,290`;
`PrimaryCovarianceBounds.lean:99,181,321,385,639`.

## 4. All-order inverse jets retain the target weight

The lower determinant bound is used for the normalized matrix Hhat=R H,
not a determinant that degenerates as R grows. Let U=Hhat^-1. At a point,
in a submultiplicative matrix norm, take A0,B0,L>0 and assume for j<=m

    ||D^j Hhat|| <= A0 L^j j!,  ||U||<=B0.

Differentiating Hhat U=I gives, for every m>=1,

    U^(m)=-U sum_(j=1)^m binom(m,j) Hhat^(j) U^(m-j).

For mixed derivatives the terms are indexed by subsets of the ordered
derivative directions; taking norms gives the same binomial majorant.
Set x=A0 B0. If a0=1 and am=x sum_(k=0)^(m-1) ak, then am=x(1+x)^(m-1)
for m>=1: the partial sum through m is (1+x)^m, by induction. Thus

    ||D^m U|| <= m! B0 [(1+A0 B0)L]^m.

This is an all-order induction, not an inference from finitely many computed
derivatives. For a target V, with Cv,Lv>0 and w>=0, whose actual derivatives obey

    ||D^j V|| <= Cv*w*Lv^j*j!,

the product rule gives, with Lh=(1+A0 B0)L,

    ||D^m(UV)|| <= m! B0 Cv*w*(Lh+Lv)^m.

Indeed the intermediate sum is sum Lh^j Lv^(m-j), bounded by (Lh+Lv)^m.
All weights are evaluated at the point; w is never differentiated. The
original inverse solve is R U V; R is constant in the native differentiation
variables and is a polynomial band cost.

For each fixed m, if input jet constants and inverse bounds are uniform
in band and their losses are polynomial in the scale, the displayed output
is also polynomial in that scale. Constants may depend on m and on the
correction iteration; no uniformity in those indices is manufactured.

Combining this estimate with Section 3's actual positive inverse weights
supplies the premises of the signed half-weight quotient lemma in
SIGNED_COVARIANCE_PROOF.md whenever the actual matrix and request input jets
have been established. In particular, inversion itself cannot introduce an
inverse power of the flat target factor.

Source: `SignedCopyBounds.lean:426,471`. The validation runner independently
checks the noncommutative inverse differentiation formulas through order 2.

## Validation and theorem scope

Run `verify_primary_covariance_chain.py` with SymPy available. It checks RAD
execution with one and four workers, requested-case identity in every fork,
isolated writes, parent-world preservation, record/replay, independent symbolic
identities, and rejection of omitted modal coupling and a reduced determinant
error coefficient. WHY records settlements which recheck every certificate.

The derived theorem above concerns actual linear solution segments and their
integrated covariances under specified coefficient and directional bounds.
It does not assert that the complete nonlinear physical correction iteration
satisfies every premise. The supplied source connects these statements through
`ActualSignedControl.referenceBounds`, `copied_coefficients_jets`,
`CorrectionAnalyticStep.step`, and `ActualCyclePreservation.state_runInvariant`.
Those source connections must not be replaced by a boolean assertion of proof.
