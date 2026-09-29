# Independent hypothesis: termination from level dynamics

This experiment was formulated from the existing Kummer-growth identities.
No web search or external proof attempt was used in this iteration. No X is
proved. The aim is to test a candidate global mechanism, not individual curves.

Let e_k=log_p C(k), g_k=e_(k+1)-e_k. The previous elementary quotient injection
proves that g_k is a nonnegative nonincreasing integer sequence. X3 requires
e_N<N(m+1) at some finite level N for some eligible prime.

## Hypothesis 1: monotonicity gives a well-founded progress measure

Set D_k=e_k-k(m+1). On an unsuccessful branch D_k>=0. If also g_k>=m+1,
write h_k=g_k-(m+1)>=0. Search potentials V=aD+bk+ch, required to be
nonnegative throughout this orthant and strictly decrease at each step.

The permitted constant plateau g_k=m+2 gives D_k=k, h_k=1, and
V_(k+1)-V_k=a+b. Universal nonnegativity requires a,b,c>=0; strict descent
requires a+b<=-1. These requirements contradict each other for ALL real
coefficients, not just the integer coefficients explored by RAD.

More generally, this infinite path excludes any nonnegative integer-valued
strict ranking function for the stated transition relation, regardless of
its formula: its value would decrease by at least one infinitely often.
The constant plateau respects g_k congruent to m modulo 2 as well.

This is not a putative elliptic-curve counterexample. It shows that a proof
must use information beyond this transition relation. The earlier global
bridge audit found the same missing freedom in corank notation; changing to
dynamics has not removed it.

## Hypothesis 2: a block-decrease rule forces a finite certificate

Proposed additional hypothesis, not proved for Selmer groups:

    There is L>=1 such that for every k>=0,
    g_(k+L) <= max(m, g_k-1).

Set H=max(g_0-m,0). Iteration of this rule gives

    max(g_k-m,0) <= max(H-floor(k/L),0).

Proof: write k=qL+r with 0<=r<L. Monotonicity gives g_k<=g_(qL), and
the block rule decreases each positive excess by at least one per block.
The sum of the right-hand side over all k>=0 is

    A = L H(H+1)/2.

Consequently e_N<=mN+A for every N, and choosing N=A+1 yields
e_N<N(m+1). This proves the conditional termination estimate for all H,L.
RAD checks its finite arithmetic on a diagnostic grid; the argument above
establishes the unbounded statement.

## Does the repair fill X3?

No. For nonincreasing integer growth sequences, existence of such an L is
equivalent to eventually having g_k<=m. If g_K<=m for some K>=1, take L=K;
then g_(k+L)<=m for every k. If already g_0<=m, take L=1. The converse
follows by iterating the block rule. Eventual g_k<=m is equivalent to the
single-level certificate, by summing the finite initial excess.

Thus a universal arithmetic proof of the block rule would prove X3, but
the rule must not be inserted as an automatic repair and then counted as
an established theorem. This iteration found a valid conditional estimate
and rejected a circular route to claiming unconditional termination.

## Execution

termination_synthesis.rad explores 729 coefficient proposals in isolated
resource-bound forks, checks requested-input agreement and parent preservation,
then evaluates the explicit block-decrease repair. WHY records the failed
strict-descent condition through a resolver that rejects forged acceptance.
No kernel extension is justified by this experiment: the missing object is
an arithmetic proof, not an unimplemented integer operation.
