# Global X3 bridge audit

Scope: a symbolic audit of a proposed proof route for all analytic ranks,
not a proof of BSD, a claim about realizable elliptic curves, or a survey of
all possible proof routes. No X has been filled by this audit.

## Source input and proposed deductions

Under its residual-representation, Manin-constant and nonvanishing hypotheses,
[Kim, Theorem 1.8](https://arxiv.org/html/2203.12159v6)
identifies Selmer corank s with the least auxiliary order d at which a Kurihara
number is nonzero. Its application needs a separate argument giving d<=m,
where m is the complex analytic rank. Section 1.9.2 also distinguishes the
p-adic L-function order from complex analytic rank.

For a fixed eligible prime, the revised X3 is equivalent to s<=m. Indeed,
with the usual cofinitely generated primary decomposition,

    log_p C(k) = k*s + log_p #F[p^k]

for a finite p-group F. A single-level bound implies s<m+1, hence s<=m.
Conversely, if s<=m, any k>log_p #F gives log_p C(k)<k(m+1).
Thus replacing the finite certificate by d<=m under Kim's hypotheses does
not by itself prove the remaining comparison.

The audit tests whether this comparison follows from the following NUMERICAL
consequences, even granting extra unproved inputs in the attempted proof:

    m>=2, r>=m, t>=0, s=r+t, d=s, s congruent to m modulo 2,
    q>=s (where q stands for a p-adic order).

It additionally tests t=0 (primary finiteness) and q=s (the strongest order
equality one might hope to import along this route). Granting these extra
conditions does NOT assert their unconditional arithmetic validity.

## Universal symbolic obstruction to this inference

For arbitrary integers u,v>=0 set

    m=2+u, j=1+v, s=d=m+2*j.

Choose either (r,t)=(m,2*j) or (r,t)=(s,0). Choose q=s or q=s+2*j.
All listed premises hold in every combination. In the strongest combination,
primary finiteness holds and the p-adic order equals corank. Nevertheless

    d-m=2+2*v>0

for EVERY u,v>=0. Thus those numerical premises do not imply d<=m, even
when supplemented by primary finiteness and p-adic order equality.

This is a proof about the listed implication only. These symbolic assignments
are NOT asserted to arise from elliptic curves or genuine L-functions.
Arithmetic information absent from the list could rule them out; that is
precisely what a successful proof must supply.

## RAD verification and stopping condition

global_bridge_audit.rad represents a+b*u+c*v by the integer coefficient list
[a,b,c]. Such an expression is nonnegative for every u,v>=0 if and only if
all three coefficients are nonnegative: evaluate at (0,0), then let either
variable grow for necessity; sufficiency follows by addition. A positive
constant and nonnegative other coefficients certify strict positivity.
Identically zero expressions have all coefficients zero. These elementary
facts justify the all-parameter interpretation of the coefficient checks.

Four isolated forks audit the four assumption combinations. No bound on u
or v is sampled. WHY explains the settled symbolic deficit [2,0,2], and the
resolver rejects fabricated evidence. The integer coefficient calculations
are executable checks; the universal reasoning is stated above, not encoded
in a foundational proof kernel.

This retires attempts to derive X3 solely by recombining these inequalities.
It does not retire approaches proving a new complex-to-arithmetic comparison.
Further iterations on this route require an actual candidate for that
comparison, not more parameter values or an assumption that d<=m.
