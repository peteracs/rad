# A rigorous local amplification region and the failure of fixed-mode invariance

This iteration targets an actual lower bound for the full Navier–Stokes
derivative. It uses a new periodic initial-field family, not the earlier
compact cutoff silently reinterpreted. The region below has positive
enstrophy amplification with pressure and viscosity included. It has not
been proved invariant, and no blowup conclusion follows.

## What `rg.exe` found and how RAD is being used

| Available capability | Implementation or existing dogfood | Use in this attack |
|---|---|---|
| Isolated heterogeneous search | `core/vm/src/vm/builtins_impl/sandbox_and_rollouts.rs`, `bi_simulate_many`, `bi_fork_with` | 225 candidate states checked independently |
| Counterexample-guided refinement | `projects/dogfood/collatz-termination/local-rank/refine.rad` and `refine.py` | Pattern reused to distinguish valid cone states from remainder counterexamples |
| Explain accepted/rejected evidence | `core/vm/src/vm/builtins_impl/world_loading_and_sandbox.rs`, `bi_why` | Aggregate traces to the concrete candidate proposals |
| Generated-history shrinking | `core/vm/src/vm/builtins_impl/property_testing.rs`, `shrink_model_trace`, `bi_model_check` | Available for trajectory search; this finite scan instead finds the exact smallest integer witness |
| Solver proposals with independent checks | `projects/dogfood/collatz-termination/search.py` uses external Z3; `verify.rad` checks results | Establishes a usable proposal/check separation; no SMT PDE theorem is assumed |
| Arbitrary natural arithmetic | `projects/dogfood/collatz-barrier/natural.rad` | Available if coefficient growth exceeds bounded integers; current domain fits int64 |

`spectral_rate.rad` adds a direct complex Fourier-convolution and Leray
projection checker in pure RAD. `amplification_search.rad` performs the
bounded exact search and rejects forged acceptance of a counterexample.
No missing VM feature was encountered and no kernel axiom or bypass was
introduced. Model search and `why()` do not supply the missing PDE invariance
proof automatically.

## An exact three-dimensional periodic test family

Use the torus with coordinate periods 2pi, normalized spatial measure,
and viscosity nu>0. Let u be the sum of the following six real waves:

| Amplitude | Wave | Polarization |
|---|---|---|
| a | cos x | (0,1,1) |
| b | cos y | (1,0,1) |
| c | sin(x+y) | (1,-1,1) |
| d | cos z | (1,1,0) |
| e | sin(y+z) | (1,1,-1) |
| f | cos(y+2z) | (0,-2,1) |

Every polarization is perpendicular to its wavevector. The field is smooth,
mean zero, divergence-free, genuinely depends on all three coordinates
when the first five amplitudes are positive, and has finite energy on the
torus. Standard Navier–Stokes scaling changes the period to one if needed.

For every Fourier mode k, the full instantaneous equation is

    d_t u_hat(k) = -nu |k|^2 u_hat(k)
      - i P_k sum_{p+q=k} [u_hat(p) dot q] u_hat(q),
    P_k=I-kk^T/|k|^2.

The sum includes every pair of input modes; it is not a Galerkin assumption
that newly generated modes stay zero. The pressure contribution is exactly
the global Fourier projection P_k.

With E=(1/2) integral |curl u|^2, exact calculation gives

    E=(a^2+b^2+3c^2+d^2+3e^2)/2 + 25f^2/4,
    E'=(abc+bde)/2 - def/4
         - nu[a^2+b^2+6c^2+d^2+6e^2+125f^2/2].             (1)

The kinetic energy derivative simultaneously satisfies K'=-2nu E, as it
must for the full unforced equation. Both identities were independently
checked from the Fourier derivative.

## A continuous parameter region with a lower bound

Let lambda>=64nu and impose

    lambda <= a,b,c,d,e <= (5/4)lambda,  |f|<=lambda/4.     (2)

The cubic production in (1) is at least (231/256)lambda^3, and the
viscous loss is at most (875/32)nu lambda^2. Therefore

    E' >= (973/2048)lambda^3 > (3/8)lambda^3.

Also E <= (475/64)lambda^2 < 8lambda^2. Since 8^(3/2)<24,

    E' >= E^(3/2)/64,                                    (3)
    E'-E^(3/2)/64 >= (205/2048)lambda^3 > 0.              (4)

These are analytic bounds for every real parameter in (2), not just
statements about sampled corners. The exact RAD scan checks all 96 corner
and central-remainder cases at lambda=64, nu=1 as arithmetic evidence.
There is no claim that (2) describes the old compactly supported datum.

## Stability under small arbitrary angular perturbations

The bound is not confined to a precisely finite Fourier support. For
smooth divergence-free fields the functional E' can be written

    R(u)=-integral partial_k u_i partial_k u_j partial_j u_i
               -nu ||Delta u||_2^2.

Let C be a fixed torus Sobolev constant such that
||grad w||_L3^3<=C||w||_H2^3. For u=v+w, ||v||_H2<=B,
||w||_H2=delta<=B, trilinearity and Cauchy–Schwarz give

    |R(u)-R(v)| <= 7CB^2 delta + 3nu B delta,
    |E(u)^(3/2)-E(v)^(3/2)| <= 5B^2 delta.

For the six-mode fields in (2), direct Fourier norms give
||v||_H2^2 <= (1065/16)lambda^2 < (9lambda)^2. Set B=9lambda and
m_0=(205/2048)lambda^3. It follows that (3) still holds for every smooth
divergence-free perturbation satisfying

    delta <= min[B, m_0 / {2[(7C+5/64)B^2+3nu B]}].        (5)

Thus sufficiently small arbitrary generated angular modes do not instantly
destroy the lower bound. By H2 continuity of the smooth local solution,
it persists on some positive short interval from any such initial field.
This functional estimate is a written argument; C and the trajectory's
residence time in (5) have not been certified numerically by RAD.

## What the counterexample search found

Drop the restriction on f while fixing a=b=c=d=e=64 and nu=1.
The smallest nonnegative integer f in the scan 0..128 that violates (3)
is f=39. At f=128 even E' is negative. Increasing the sixth angular mode
can therefore defeat a lower bound that ignored its size.

RAD verifies the full derivative directly for f=0,16,128. It also detects
the new mode k=(1,0,1), which is outside the six-mode support:

    d_t u_hat(1,0,1) = -(i ad/2)(0,1,0).

At the tested amplitudes this is (0,-2048i,0). The six-mode subspace is
not invariant under the true equation. Recording (3) without this outgoing
mode would miss an essential part of the dynamics.

There is a second exact invariance failure. At a=b=c=d=e=lambda and f=0,
the kinetic energy is 3lambda^2 and its derivative is -9nu lambda^2<0.
Any field retaining all five orthogonal wave amplitudes at least lambda
has kinetic energy at least 3lambda^2; additional orthogonal modes only
increase that minimum. Hence the fixed-lambda region (2), even with added
orthogonal modes, cannot contain this solution for all sufficiently small
positive times. This does not contradict short persistence of (3): the
inequality can hold outside the sufficient region (2).

## The remaining prize-level obligation

If (3) held all the way along a solution, integration would force a
singularity no later than 128/sqrt(E(0)). We have not established that
persistence. A successful construction needs a region of states that the
full evolution provably stays within while energy transfers to higher
frequencies and the dissipation is controlled.

Any evolution confined to a fixed Fourier band |k|<=N has
E<=N^2 K<=N^2 K(0), so it cannot achieve unforced enstrophy blowup.
Adding finitely many favorable modes without proving transfer to unbounded
frequencies cannot close this gap.

The concrete next search target is therefore a quantitative transfer
estimate together with a bound on the outgoing-mode remainder, compatible
with decreasing kinetic energy. It must cover an infinite continuation of
the transfer. Neither a finite scan nor a solver timeout establishes that.

## Validation and scope

`verify_amplification.py` checks full Fourier incompressibility, conjugate
symmetry, the kinetic energy identity, and RAD's independent convolution
values. All 225 search cases agree; 96 cone cases pass and the canonical
remainder counterexample is 39. Forged acceptance of a counterexample is
rejected. Worker-count output and recorded-world replay agree.

This gives an actual local amplification lower bound with a perturbative
extension, and explicit failures of proposed invariant sets. It supplies
no persistent blowup lower bound or Millennium solution, and no claim of
novelty is made for instantaneous enstrophy growth.
