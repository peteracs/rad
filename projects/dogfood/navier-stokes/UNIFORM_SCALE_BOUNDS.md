# Uniform scale estimates: established reductions and outstanding premises

2026-09-09. No Lean execution. This is not a proof of the complete weighted
correction invariant. It proves the uniformity reductions below, with RAD
checking their polynomial identities and finite-degree positivity certificates.

## The exponential bound is uniform under the source's reference controls

`ActualParticularControl.referenceControl` defines the error rate

    mu_(l,n) = (E+4C)/S_(l,n)

and derives the integration-length bound

    L_(l,n) <= M S_(l,n).

Here S>0 and E,C,M are fixed reference-family constants, independent of l,n.
For E,C>=0 this immediately proves

    exp(mu_(l,n) L_(l,n)) <= exp((E+4C)M).                  (1)

Thus the generic exp(A_0*T) concern in the preceding audit is addressed by
this source's weighted reference energy estimate, **provided its primitive
energy estimate and reference-family constants are valid**. Its errorRate
is a weighted energy growth rate, not an assertion that the norm of the
entire unweighted coefficient matrix equals (E+4C)/S.

If a positive clock lambda rescales the rate and length together, then

    (lambda*mu)*(L/lambda)=mu*L.                            (2)

There is no extra clock-dependent exponential. `uniform_scale_audit.rad`
checks (2) with the denominator cleared and the positivity certificate
for (1): writing d=M-L/S>=0, the exponent slack is (E+4C)d>=0.
These are identities in indeterminates, not samples of large band indices.

## Coordinate losses stay polynomial if the primitive clock and gap bounds hold

`ScaledActualParticularControl.geometry_cost_uniform` uses

    0 < clock_lower <= lambda_(l,n) <= clock_upper,
    gap_(l,n) <= fixed_budget,
    argumentCost(reference_(l,n)) <= A S_n^a.

With U=1+coveringBound(fixed_budget)*(2+clock_upper+1/clock_lower),
the transported cost is at most

    4 U^2 A^2 S_n^(2a).                                    (3)

The underlying estimate is elementary. Both transported coordinate maps have
norm at most U times their reference map norm. Each reference norm is bounded
by the reference argumentCost. If X=U*argumentCost(reference)>=1, the new cost
is at most 1+X+X(1+X)=(1+X)^2<=4X^2. Substitution proves (3).
This constant is independent of l,n. Establishing the three displayed premises
for the actual chosen geometry is a separate obligation, not something an
exponent ledger can supply.

## Polynomial band factors can be absorbed with one fixed dyadic power

For each integer p>=1 and every integer n>=0,

    n^p 2^(-n) <= (2p)^p.                                 (4)

Proof: for n<=2p the bound is immediate. For n>=2p,

    (1+1/n)^p = sum_(r=0)^p binomial(p,r)n^(-r)
               <= sum_(r=0)^p (p/n)^r <= 2.

Therefore n^p/2^n is nonincreasing from n=2p onwards. This proves (4)
for all n. For p=0 use the bound 2^(-n)<=1.

Since the actual chart uses Q_n=2^(-n), S_n=n^2, for every fixed integer a>=1,

    S_n^a Q_n^gamma <= (4a)^(2a) Q_n^(gamma-1).            (5)

The loss of **one** in the Q exponent does not depend on a. The constant does.
Consequently even a polynomial band degree depending on the fixed correction
stage J and derivative order m can be absorbed with this same exponent loss;
the resulting constant is allowed to depend on J,m. This matches the quantifiers
in the finite-prefix estimate, which requires constants uniform in physical
scale n, not a single numerical constant for every J,m.

RAD checks coefficientwise certificates for (4) at p=1,...,6: substituting
n=2p+x makes 2n^p-(n+1)^p a polynomial with nonnegative coefficients for x>=0.
The proof for arbitrary p is the binomial argument above, not extrapolation
from those six checks. The all-order analytic statement is not encoded as a
foundational RAD theorem.

This absorption does not handle a factor exp(n^2) or a stage-dependent power
Q_n^(-J). One must first prove the residual costs have the asserted polynomial
form. The source's weighted energy bound is significant for exactly this reason.

## What would complete uniform preservation

These estimates explain how the source avoids scale-dependent exponential
growth and absorbs polynomial derivative costs into its fixed loss

    L_m=m(m+2)+5/2+2h+2hm.

They do not yet prove that every actual corrected field belongs to the required
weighted class. The remaining input chain includes:

* `selected_energy` and `selected_input_jets` for the constructed phase family,
  including uniform reference constants and the supplied current-source norm.
* Actual clock lower/upper bounds, bounded cover gap, phase nondegeneracy, and
  polynomial geometry cost on every active cell, with coherent overlap formulas.
* Localized good/Gaussian residual estimates, signed and mean updates, and their
  nonlinear cross terms, proving that all components of the analytic invariant
  improve from sigma to sigma+1/10.

The supplied source claims to discharge these through its constructor modules.
This RAD work has not independently verified that chain. Assuming those inputs
and then reporting the full invariant as proved would be circular for this task.

## Execution

`uniform_scale_audit.rad` runs six isolated forks, checks the symbolic exponent
identities and polynomial certificates, and recomputes the results in its causal
resolver. `verify_uniform_scale_audit.py` checks record/replay, saved WHY
provenance, and rejection of an injected claim of full invariant preservation.
The field `actual_weighted_invariant_proved` remains false.

```powershell
python projects/dogfood/navier-stokes/verify_uniform_scale_audit.py
```
