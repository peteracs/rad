# Active target: a smooth-forced ordinary Navier–Stokes construction

We target Clay alternative D first (the unit periodic torus), at one fixed
positive viscosity. It avoids the separate spatial-decay obligation of C;
this is a scope choice, not evidence that D is easier to solve. The force
must be defined smoothly for all t>=0 and obey condition (9), not just be
smooth before a proposed singular time. Source: the
[official statement](https://www.claymath.org/wp-content/uploads/2022/06/navierstokes.pdf).
The earlier periodic moment region is disproved and retired.

No mechanism is currently known here that closes this target. We are not
reconstructing a disclosed proof or claiming a likely solution.

## The construction must produce the force, not assume its regularity

For a chosen divergence-free trajectory, write

    f = u_t + (u.grad)u + grad p - nu Delta u.

Pressure and every correction must be included before estimating this
residual. Defining f by this formula does not prove its admissibility.
Finite-order Taylor cancellations and a fixed number of small residual
derivatives are insufficient.

The required stage lemma should take a precisely defined structured input
configuration to a stronger next configuration using a chosen smooth force.
It must provide, together:

1. Exact incompressibility, the full equation and global periodic pressure.
2. A quantitative amplification/transfer lower bound for the complete step.
3. Bounds on outgoing modes and profile changes that allow the next step.
4. Mixed-derivative estimates on the actual force increment, including all
   switching, matching and cutoff terms.
5. Summable stage times, compatible initial data, convergence before the
   limiting time, and a lower bound implying genuine loss of smoothness.

These are obligations, not results. A test missing one of them cannot be
promoted to a repeatable stage theorem.

## New RAD budget: all orders, conditional on actual residual estimates

Suppose the **actual** smooth force increments satisfy, for every fixed
spatial multi-index alpha and time order m,

    ||d_x^alpha d_t^m f_q||_infinity
        <= C_(alpha,m) 2^(-q^2 + d*q),
    d = max(kappa,r)*|alpha| + s*m.

The constant is independent of q. All additional losses, including any
q-dependent constants, must already be accounted for in this bound.
The parameters describe carrier frequency <=2^(kappa*q), envelope inverse
radius <=2^(r*q), and inverse stage time <=2^(s*q). They do not identify
those two spatial scales. This derivative-cost model is an assumption to
be proved for the real increments, not a consequence of dimensional labels.

For q>=Q>=d+1,

    q^2-d*q-q = q*(q-d-1) >= 0,
    sum_(q>=Q) ||d_x^alpha d_t^m f_q||_infinity
        <= C_(alpha,m) 2^(1-Q).

Thus every fixed mixed derivative has a uniformly convergent tail. If
each f_q is a globally smooth periodic function supported in a common
compact time interval, the summed force is globally smooth and satisfies
Clay's temporal-decay condition. For stage supports accumulating at T and
vanishing after T, each increment must itself extend smoothly by zero;
the uniform derivative convergence then gives the smooth extension of
the sum. Overlapping supports do not invalidate the sufficient bound.

This is a written all-order comparison argument. `force_budget.rad` checks
845 bounded integer instances and detects two common inadequate estimates;
`verify_force_budget.py` independently checks its output and the symbolic
identity and geometric sum. RAD has not formalized the function-space
theorem. No actual residual satisfying the bound has been constructed.

The first diagnostic is finite-order decay: a bound 2^(-40q) combined with
carrier exponent 3 cannot certify spatial derivative order 14. An inadequate
upper bound does not prove the force itself is inadmissible; cancellations
could improve the bound. The second diagnostic uses r=1, kappa=3, s=4:

    core diffusion exposure ~ nu 2^(-2q),
    carrier diffusion exposure ~ nu 2^(2q).

These parameters are a diagnostic example, not a selected construction.
They show why short core-exposure time does not control carrier damping.

## Research selection rule

### Actual residual operator now available

`forced_fourier.rad` provides `actual_force(u, ut, viscosity)` for bounded
finite Fourier banks. It computes

    f = ut + P[(u.grad)u] - nu Delta u

with exact integer numerators and rational denominators, including every
frequency in the input, time derivative, and quadratic convolution. This
chooses a divergence-free force; the complementary gradient is absorbed
into the global periodic pressure. It does not assume a spectral truncation
of the nonlinear equation. Conjugate symmetry, incompressibility, duplicate
frequencies, positive viscosity and arithmetic input bounds are checked.
The input bounds keep all intermediate integers below 10^14.

`verify_forced_fourier.py` compares 281 output coefficients across nine
cases with independent exact complex algebra, including an ut-only mean
mode, and verifies rejection of four invalid inputs. The separate
`forced_fourier_check.rad` includes an exact zero-residual heat-flow check
and an interacting-wave check with nonzero generated force modes.

These are operator correctness tests, not nine candidate stage proofs.
In particular, the caller still must prove that the supplied ut is the
time derivative of its trajectory; one instantaneous call cannot check
that condition. No time interval, all-order residual bound, amplification
lower bound or stage-to-stage matching is inferred from the output. The
operator is a research library, not an extension of RAD's trusted kernel.

### Admission rule for further candidates

Do not pursue another fixed-support, fixed-profile or broad moment-region
repair already excluded by the recorded counterexamples. Before extending
a new ansatz to more orders, derive one actual forced-stage estimate that
links amplification to its force cost under refinement. Reject a proposed
leading-order mechanism if its unavoidable force derivatives diverge.
Keep bounded-velocity continuation and the earlier geometry-specific
obstructions in force; allowing smooth forcing does not erase them.

The unresolved central task is constructing a repeatable stage with the
stated estimates. The new budget exposes that task; it does not solve it.
