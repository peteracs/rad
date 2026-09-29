# Exact elimination of the transfer budget

No web search was used. This is a symbolic dependency check, not a proof
of a universal arithmetic source bound.

Let delta=s+epsilon with s,epsilon>=0 integers. The proposed joint target is

    exists B>=0: delta<=m+B and epsilon>=B.

Equivalently,

    max(0,delta-m)<=B<=epsilon.

This integer interval is nonempty if and only if s<=m. Necessity follows
by adding the source inequality m+B-s-epsilon>=0 and the correction
inequality epsilon-B>=0, obtaining m-s>=0. For sufficiency, when s<=m,
take B=max(0,delta-m). If delta-m>=0, B=s+epsilon-m<=epsilon; otherwise
B=0<=epsilon. All values are integers, so no real-to-integer feasibility
issue is hidden in the elimination.

Thus, at each eligible prime,

    exists B>=0 with both comparison bounds <=> s_p<=m.

Quantifying over eligible primes and all curves gives exactly X3, by the
previously proved Selmer growth equivalence. This does not show that a
two-part arithmetic proof is impossible. It shows that discovering a budget
numerically or introducing it as an auxiliary variable cannot itself
establish the universal existence theorem.

Two tempting automatic choices expose the dependency:

* B=max(0,delta-m) makes the source bound automatic, but the correction
  becomes exactly the condition s<=m.
* B=epsilon makes the correction automatic, but the source bound becomes
  exactly the condition s<=m.

The RAD program implements elimination by positive combinations of affine
inequalities in symbolic variables [m,s,epsilon,B]. Sixteen forks vary the
positive presentation scales, not ranks or elliptic-curve instances. Each
returns the same normalized inequalities m-s>=0 and epsilon>=0. WHY exposes
the surviving rank condition rather than accepting it as an input theorem.
The universal sufficiency proof is the interval argument above.

The earlier quotient-transfer implication remains correct. What has not
been supplied is an independent arithmetic proof of the surviving condition.
Further budget calibration alone would repeat an equivalent form of X3.
No universal X is filled by this elimination.
