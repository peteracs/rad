# Adaptive finite-layer decision for the X3 rank target

This work proves a terminating algebraic decision rule, including an optimal
worst-case cutoff. It does not prove the universal arithmetic assertion X3.
No web searches were used.

## Inputs and arithmetic scope

Let R=Q_p[[T]], and let X be a finite-length R-module. Assume its exact length
delta is known. Write c=dim(X/TX) and b_j=dim(T^j X/T^(j+1) X). An oracle
supplies exact b_j sequentially. Fix m>=2 and assume c is congruent to m mod 2.

For the intended arithmetic application, the identification c=s_p and the
parity statement are supplied premises. Neither is proved by this RAD program.
Nor does it construct X, compute delta for an arbitrary elliptic curve, or
provide an arithmetic layer oracle. The test oracle uses synthetic Smith blocks.
The decision function itself receives only delta, m, and the queried layers.

## Theorem: finite decision without assuming a positive answer

The inequality c<=m can be decided after at most

    J_* = max(0, delta-m-1)

sequential layer queries, using only these inputs.

Proof. Since c<=delta, if delta<=m+1 then parity already gives c<=m.
Otherwise J_*=delta-m-1>=1. Query through b_(J_*), unless earlier observations
already decide the answer. If b_(J_*)=0, all subsequent layers vanish, so

    c = delta - sum_{j=1}^{J_*} b_j.

This computes c exactly, allowing either a positive or a negative decision.
If b_(J_*)>0, monotonicity and integrality imply b_j>=1 for every j<=J_*.
Consequently

    c <= delta - sum_{j=1}^{J_*} b_j <= delta-J_* = m+1.

Parity gives c<=m. These alternatives exhaust the possibilities. QED.

The proof is independent of the finite domain bounds imposed by the executable.
RAD checks instances and provenance; the written argument supplies the universal
algebraic quantifiers.

## Optimal early stopping

With no observations and delta>0, the possible c are the integers from 1 to
delta; with delta=0 only c=0 occurs. After a nonempty realizable prefix with
last entry positive, the supplied optimal-information theorem gives exactly

    b_1 <= c <= delta - sum(prefix).

If the last entry is zero, only the right endpoint occurs. Restrict these
possibilities to c congruent to m mod 2. Stop positively if their maximum is
at most m, negatively if their minimum exceeds m; otherwise query again.
No decision from these observations alone can stop earlier on an ambiguous
prefix, because modules exist realizing both answers.

## The cutoff is sharp for the stated information

For m>=2 and J>=1 take

    X_good = (R/(T))^(m-1) direct_sum R/(T^(J+2)),
    X_bad  = (R/(T))^(m+1) direct_sum R/(T^J).

Both have delta=m+J+1, hence J_*=J. Their quotient dimensions are m and m+2,
respectively, so both satisfy parity but give opposite answers. Their layers
b_j agree and equal 1 for every 1<=j<J. At j=J the good module has layer 1
and the bad module has layer 0. Thus fewer than J sequential observations
cannot always decide the inequality using only delta, m, and parity.

These are abstract modules, not asserted to arise from elliptic curves.
Additional arithmetic constraints could exclude them; proving such exclusions
would require additional arithmetic input. In particular they are not BSD
counterexamples.

For m=2, delta=10, the executable uses Smith exponents [1,9] and [1,1,1,7].
Both require seven queries. The first returns c=2, the second c=4.

## Relation to X3 and the remaining quantifier

A positive arithmetic decision gives s_p<=m. Using the supplied decomposition

    C_E,p(N) = p^(N s_p) #F_p[p^N],

choose any positive integer N>log_p(#F_p). Then

    C_E,p(N) <= p^(Nm) #F_p < p^(N(m+1)),

which supplies the revised X3 certificate for this curve and prime. The
earlier consecutive-ratio certificate also follows once the finite group
F_p[p^k] has stabilized.

However the universal X3 statement has an existential arithmetic quantifier:
every relevant curve must admit an eligible prime with a positive decision.
Proving termination of each fixed-prime decision does not prove this. A negative
decision at one prime also does not refute that existential statement.

This attempt removes an algorithmic stopping uncertainty under exact oracle
access; it does not remove the missing arithmetic comparison with complex
analytic rank. No complete X placeholder is filled.

## Execution and independent validation

Run:

    python projects/dogfood/bsd-x/verify_adaptive_layers.py

The RAD program uses isolated forks, simulate_many, parent-state checks,
recomputing resolution, WHY(), and field-level provenance. The verifier:

- compares execution under one and four workers and checks recorded replay;
- checks 625 forked requests and two sharpness witnesses;
- independently enumerates every integer partition of each tested total length,
  including modules outside the RAD fork batch, to compute the exact feasible
  coranks at every prefix and verify earliest stopping;
- rejects a forged positive outcome and a forged universal-proof flag.

These checks validate this implementation's finite calculations. WHY() records
how the decision arose; it does not supply an unencoded arithmetic theorem.

Files: `adaptive_layer_decision.rad`, `verify_adaptive_layers.py`,
`adaptive_layer_decision.why.txt`, and `adaptive_layer_verification.json`.
