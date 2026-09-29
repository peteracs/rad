# Fixed compact profiles fail the full energy balance

This is an analytic deduction under explicit ansatz assumptions. RAD checks
the associated rational exponents, not the integration or PDE hypotheses.

Let tau=T-t, nu>0, and let V be a fixed nonzero smooth compactly supported
divergence-free field on R^3. Consider the exact global ansatz

    u(x,t) = tau^(beta-1) V(x/tau^beta),  2/5 <= beta < 1/2.

Assume it solves Navier–Stokes with a force uniformly bounded in space and
time up to T. This boundedness follows from Clay's smooth-force conditions.
The usual energy identity is applicable to these smooth compact fields.

Writing C_E=(1/2) integral |V|^2>0 and C_D=integral |grad V|^2>0 gives

    E(t) = C_E tau^(5 beta-2),
    E'(t) = -(5 beta-2) C_E tau^(5 beta-3),
    ||grad u||_2^2 = C_D tau^(3 beta-2),
    |integral f dot u| <= ||f||_infinity ||V||_1 tau^(4 beta-1).

For 2/5<beta<1/2, substitute into

    E' = -nu ||grad u||_2^2 + integral f dot u.

Divide by tau^(5 beta-3). The left side is the nonzero negative constant
-(5 beta-2) C_E. The dissipation term tends to zero like tau^(1-2 beta),
and the force term tends to zero in absolute value like tau^(2-beta).
Contradiction. At beta=2/5 the left side is zero, while dividing the right
side by tau^(3 beta-2) gives -nu C_D plus a force term tending to zero as
tau^(beta+1). Again a contradiction.

Thus **no nonzero field of this exact fixed compact-profile form** works
in the proposed range. This is a template exclusion, not general regularity
of Navier–Stokes and not an exclusion of arbitrary changing profiles.

The scale screen uses beta=r/s. Its strict window 2r<s<(5/2)r is precisely
2/5<beta<1/2. Consequently all 76 screen survivors fail if interpreted as
this exact global fixed compact-profile ansatz. The exponent conditions are
still relevant to a local core matched to a separate outer flow.

## A stronger limitation on straightforward profile corrections

If one proposes V_tau=V_0+O(tau^(1-2 beta)) on one fixed compact rescaled
support, with enough uniform derivative control to pass to the leading
profile equation, the leading inviscid equation is

    (1-beta)V_0 + beta(y dot grad)V_0 + (V_0 dot grad)V_0 + grad P_0 = 0.

Pairing with V_0 and integrating gives

    (1-(5/2)beta) ||V_0||_2^2 = 0.

Pressure and transport integrate to zero; the dilation term contributes
-(3/2) beta ||V_0||_2^2. For the strict window, V_0 must vanish.
Thus a regular viscous correction series about a nonzero fixed compact
leading Euler profile cannot repair these entries either. The asserted
convergence and derivative control are necessary assumptions of this
argument; arbitrary time-dependent profiles are not covered by it.

## Local cores can exchange energy with an outer flow

For a moving ball B_R(t), e=|u|^2/2, the local identity is

    d/dt integral_B e
      = -integral_boundary (e+p) u dot n
        + nu integral_boundary u dot partial_n u
        - nu integral_B |grad u|^2 + integral_B f dot u
        + R' integral_boundary e.

The boundary and pressure terms omitted by the global compact-profile
ansatz are exactly what a matched construction must supply. They are not
free parameters: pressure and exterior velocity must satisfy the global
incompressible equation. A useful next target is an explicit outer solution
and matching estimates for these fluxes, together with mixed force bounds.

## RAD workflow and trust boundary

The scale workload evaluates every candidate in a fork. Its causal resolver
checks each result against the declared finite arithmetic and verifies exact
portfolio coverage. Constraints admit only the complete aggregate. The
aggregate distinguishes scale-feasible entries from fixed-profile exclusions;
`why()` traces the aggregate to all contributing proposals. Reversing the
proposal order must leave the aggregate unchanged.

This tracks computational evidence for the analytic classification. It does
not transform the written energy proof into a kernel-checked PDE theorem.
