# A precise candidate comparison using mixed determinant coefficients

No web search or external proof attempt was used. This is a proposed
arithmetic mechanism with its algebraic part proved; the arithmetic identity
needed to apply it has NOT been constructed or proved.

## Independently variable perturbations reveal the rank invariant

Let A=P(0) be an n-by-n matrix over a characteristic-zero field K, and let
Z=(z_ij) be a matrix of independent indeterminates. Set

    D_A(Z)=det(A+Z).

Its lowest nonzero TOTAL degree is c=n-rank A. Proof: invertible constant
row and column changes reduce A to diag(I_(n-c),0). These changes induce
an invertible linear substitution on the z_ij, preserving lowest degree.
Each determinant term then requires at least c variable entries because
of the zero rows; taking the c complementary diagonal variables supplies
a nonzero term of degree c. QED.

More precisely, choose m entries z_(i_1,j_1),...,z_(i_m,j_m) in distinct
rows and columns. The coefficient of their product in D_A is, up to the
sign of their placement, the complementary (n-m)-minor of A. This follows
by collecting the determinant permutations that use those variable entries.
Every complementary minor arises this way. For 0<=m<=n,

    a degree-m mixed coefficient is nonzero
        <=> rank A>=n-m
        <=> dim coker A<=m.

For the supplied arithmetic presentation and control identification, this
is exactly the Fitt_m(X)=R certificate for X3. It counts Selmer directions,
not the sum of their deformation lengths.

## Why a fixed perturbation may hide the coefficient

Take A to be the four-by-four nilpotent Jordan matrix with ones on its
superdiagonal. It has rank three and corank one, yet

    det(A+zI)=z^4.

Allowing independent matrix entries instead gives a nonzero linear term
in z_30 (indices start at zero). Thus a one-parameter restriction can hide
lower-degree nonvanishing by cancellation or by omitting the relevant
direction. This is an algebraic explanation, not a claim that arbitrary
formal perturbations are allowable arithmetic deformations.

The RAD program computes the whole mixed polynomial, returns the exact
nonzero coefficient and its entry positions, and extracts the complementary
minor. It also compares a fixed diagonal perturbation with independent
perturbations under 25 invertible row/column changes of a rank-two matrix.
WHY exposes actual coefficients and positions, not only a Boolean rank test.

## The arithmetic identity this suggests investigating

A sufficient mechanism for an eligible prime p would be:

1. Construct a scalar A_(E,p) in Q_p with a proved implication

       L^(m)(E,1)!=0 (at analytic rank m) => A_(E,p)!=0.

2. Construct explicit Q_p coefficients w_eta and prove a finite identity

       A_(E,p) = sum_eta w_eta * [z^eta] det(P(0)+Z),   |eta|=m.

Then a nonzero summand coefficient exists, giving the unit minor and X3.
The weights must be produced independently of knowing a nonzero minor;
choosing w=A/c after assuming some minor c!=0 would be circular.

These two assertions are NOT established. In particular:

* No p-adic scalar is obtained just by treating the complex derivative as
  a p-adic number. The nonvanishing comparison requires an arithmetic theorem.
* The independent formal variables are not automatically genuine arithmetic
  deformation parameters. If the identity is to come from deformations,
  those parameters and their compatibility must be constructed.
* The proposed identity is a precise sufficient target, not evidence that
  such an identity exists or is easier to prove than X3.

The contribution of this experiment is to specify a coefficient-level
certificate and show why some single-parameter approaches lose the relevant
information. It does not supply an exact arithmetic mechanism as a proved
theorem and does not fill any X.

## Follow-up: distinguish an exact certificate from its arithmetic source

The following observation sharpens the status of the proposed identity. Fix
0<=m<=n and any independently given nonzero scalar a in K=Q_p. Let d_eta
denote the degree-m coefficients of det(P(0)+Z). Then

    there exist weights w_eta in K with a=sum_eta w_eta*d_eta
        <=> some d_eta is nonzero
        <=> s_p<=m.

Proof. A nonzero sum has a nonzero summand. Conversely, given a nonzero
d_eta, choose its weight to be a/d_eta and all other weights zero. The last
equivalence is the complementary-minor criterion proved above. QED.

Thus an unrestricted search for weights is exactly an equivalent certificate
search, not an independent arithmetic source theorem. In particular, defining
the scalar to be whichever coefficient the search finds reverses the required
direction of explanation. Taking a=1 does not solve this problem: it simply
asks for a unit in the ideal of the relevant minors. If necessary a square
presentation can be enlarged by identity blocks to arrange n>=m without
changing the presented module.

The proposed research target must instead specify independently:

1. an arithmetic construction of a scalar a_(E,p) and its nonvanishing
   implication from the complex leading derivative;
2. a construction of weights w_(E,p,eta), together with a proved identity
   a_(E,p)=sum_eta w_(E,p,eta)*d_eta, for an eligible prime for each curve.

These constructions may depend on the curve and arithmetic presentation;
"independently" means their existence and validity cannot be justified by
first assuming s_p<=m or selecting a nonzero target minor. No such construction
is supplied here. Also, a complex derivative cannot simply be inserted into a
Q_p identity without constructing and proving the required comparison.

The existing RAD test was rerun: 25 matrix forks, three diagnostic witnesses,
independent rational elimination and finite differences, worker determinism,
record/replay, and rejection of a forged reciprocity claim all passed. These
results check the algebraic certificate and its provenance, not either of the
two arithmetic constructions. WHY() does not infer an absent mathematical
comparison from an unexplained scalar or a user-supplied truth flag.

## Reproduce the executable checks

    python projects/dogfood/bsd-x/verify_polarized.py

The verifier checks matrix ranks by rational elimination, reconstructs each
reported mixed coefficient by finite differences of exact determinants,
checks the complementary minor, verifies replay and worker determinism, and
rejects forged arithmetic reciprocity. All matrices remain synthetic.
