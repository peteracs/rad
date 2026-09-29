# Automatic comparison transfer with the exact quotient correction

No web searches were used. The arithmetic origin and control statements are
the user's supplied premises. The deductions here are finite-length module
algebra; the RAD inputs are synthetic, not verified data for an elliptic curve.

## Correction to the earlier wording

The statement that an analytic comparison does not automatically transfer
was too broad. An established UPPER BOUND on length(X) transfers immediately
to a quotient. An equality or normalized characteristic identity must account
for the kernel. Neither operation creates a previously unknown comparison
with complex analytic rank.

Let R=Q_p[[T]], Y=X/TX, and write

    delta=length_R X,   epsilon=length_R TX,   c=length_R Y=s_p.

The exact sequence 0 -> TX -> X -> Y -> 0 proves

    delta=epsilon+c,   hence c=delta-epsilon.

This works for every finite-length R-module, without requiring any Smith
block to have length one.

## What transfers automatically

1. If delta<=m is established, then c<=m because epsilon>=0.
2. More generally, if delta<=m+B and epsilon>=B, then c<=m.
3. If delta=m is established, then c=m-epsilon. Equality c=m additionally
   requires epsilon=0. If X1 also gives r>=m, the inequalities r<=c<=m
   force epsilon=0; it is then a consequence of the supplied assumptions.

The second transfer rule permits extra deformation length and therefore
does not require proving the unnecessarily strong assertion delta<=m.

## Transfer of a characteristic-element identity

Over this DVR, characteristic ideals multiply in the exact sequence:

    char_R(X)=char_R(TX) * char_R(Y).

Since char_R(TX)=(T^epsilon), if an element f(T) of R with f!=0 is already
proved to generate char_R(X), then

    g(T)=T^(-epsilon) f(T)

belongs to R and generates char_R(Y). Its order is c. This is an automatic
algebraic transfer, with equality of ideals (generators are determined only
up to a unit). It applies to a p-adic analytic element when its original
characteristic-ideal identity is established.

It does NOT identify g with the complex L-function, or prove ord_T g<=m.
In particular, the p-adic identity alone supplies no complex-order bound.
Units of R can have any p-adic valuation, so this ideal-level construction
also supplies no normalized leading-coefficient formula for X6.

## Finite higher-level data can certify enough correction

Define the canonical layer dimensions

    b_j=dim_(Q_p)(T^j X / T^(j+1) X),   j>=1.

If X~=direct_sum R/(T^a_i), then b_j=#{i:a_i>j}. Therefore

    epsilon=sum_(j>=1) b_j.

For any finite J, the computable partial sum

    B_J=sum_(j=1)^J b_j

is a lower bound on epsilon. Thus a sufficient finite transfer certificate is

    delta<=m+B,    B_J>=B.

Proof: c=delta-epsilon<=m+B-B_J<=m. All quantifiers in this implication
are proved by exact-sequence length additivity; no limit approximation is used.

The first derived kernel-to-cokernel map has nullity b_1 in Smith coordinates.
Thus its degeneracy already certifies part of the correction. Subsequent
layers can certify more. These are canonical filtration dimensions, not raw
Taylor coefficients that might miss the Schur-complement cancellations in
the earlier experiment.

If the full presentation is known, testing its special-value rank directly
is simpler. The transfer certificate is useful when a total order bound and
partial higher-level information are what the argument supplies.

## Diagnostic examples

Take X=R/(T) direct_sum R/(T^3). Then delta=4, c=2, epsilon=2, with
b_1=b_2=1. At target m=2, a source bound delta<=m+2 transfers after both
layers are certified. One layer provides only a lower bound 1 and does not
justify subtracting the full budget 2.

For X=(R/(T))^4, delta remains 4 but epsilon=0 and c=4. The same nominal
source bound delta<=m+2 cannot give c<=2. The difference is explained by
the actual quotient correction, not by the common determinant order.

The application separately tests a p-adic-identity-only input. It declines
to treat the synthetic numerical relation as a supplied complex comparison.
Every test also retains arithmetic_instantiation_verified=false: the program
has not verified any analytic source bound for an actual elliptic curve.

## Reproduce and status

    python projects/dogfood/bsd-x/verify_quotient_transfer.py

quotient_comparison_transfer.rad uses resource-bound forks and a recomputing
resolver, with WHY exposing the discarded length and supplied comparison.
The verifier independently constructs nilpotent Jordan matrices and their
powers to check the length and filtration data, verifies replay and worker
determinism, and rejects forged transfer claims.

Resolved: precise automatic transfer rules, including quotienting a known
characteristic identity and using certified higher-level correction terms.
Unresolved: a universal arithmetic proof supplying the complex comparison
and sufficient correction. No X1, X3, X5 or X6 is filled.
