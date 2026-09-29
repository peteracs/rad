# First-order reduction of the proposed arithmetic deformation source

The user's Galois-deformation proposal specifies a possible arithmetic origin
for matrix jets. This note proves a refinement of its determinant step. It does
not construct Galois cocycles for an elliptic curve, verify Selmer local
conditions, or compare a determinant with the complex L-function. No web
searches were used.

## Theorem: higher lifts cannot repair the leading determinant jet

Let K be a characteristic-zero field, P(t) a square formal matrix over
K[[t_1,...,t_d]], A=P(0), and c=corank A. Write

    P(t)=A+sum_i t_i B_i+O(t^2).

Here O(t^2) means total degree at least two. Define the intrinsic maps

    beta_i : ker A -> coker A,   x |-> [B_i x].

Every determinant term of total degree less than c vanishes. After choosing
bases of ker A and coker A, there is a nonzero constant kappa such that the
homogeneous degree-c part is

    (det P)_[c] = kappa det(sum_i t_i beta_i).                 (1)

In particular this homogeneous part is independent of EVERY higher jet of P.

Proof. Constant invertible row and column operations reduce A to diag(I,0).
Write the resulting formal matrix in blocks as

    [ H(t)  C(t) ]
    [ D(t)  E(t) ],

where H(0)=I and C(0)=D(0)=E(0)=0. Since H is invertible over the formal
power-series ring, its determinant is a unit and

    det P = kappa det H * det(E-D H^{-1} C).

The c-by-c Schur complement has linear term sum t_i beta_i; the product
D H^{-1} C has total degree at least two. Its determinant has no terms below
degree c, and its degree-c part is the determinant of that linear term.
Since det H has constant coefficient 1, equation (1) follows. The cases of
empty blocks have their usual determinant-one convention. QED.

Consequently, for the proposed coefficient [t_1...t_m] det P:

- if c>m, it is zero for every deformation;
- if c=m, it depends only on the induced first-order maps beta_i;
- if c<m, higher jets can contribute and must in general be retained.

Thus the user's warning about nonlinear corrections is correct in general.
But if a separate argument has already established s_p=c>=m, the corrections
cannot change the target degree-m coefficient. Using the manuscript's X1 for
this lower bound is conditional on proving X1; this note does not assume it
as an established universal arithmetic theorem.

## An exact test for a proposed space of tangent maps

At c=m, let W be the K-linear span of the induced maps beta_i. Then the
homogeneous polynomial det(sum t_i beta_i) is nonzero if and only if W contains
an isomorphism ker A -> coker A.

Indeed, if every combination is singular, the polynomial vanishes at every
K-point. Since K is infinite, the polynomial is zero. Conversely an invertible
combination gives a point at which it is nonzero. This assertion concerns the
entire homogeneous polynomial, not necessarily a particular mixed coefficient
in fixed coordinates.

If an invertible combination beta exists along a genuine formal one-parameter
deformation, pull back that family by u=t_1+...+t_m. Equation (1) then gives

    [t_1...t_m] det P(t_1+...+t_m) = kappa*m!*det(beta) != 0.

Hence several named deformation parameters are unnecessary when one admissible
direction already has an invertible induced map. Conversely, adding higher
corrections to directions whose entire span is singular cannot fix the leading
coefficient. Existence of the formal deformation is essential: a first-order
direction is not automatically liftable through all orders or compatible with
the local Selmer conditions.

For a c=m space, this reduces the search to the following specific question:
does the image of the arithmetic, locally compatible, liftable tangent
directions in Hom(ker A,coker A) contain an invertible map? Neither its image
nor such a map is constructed here. This criterion does not establish c=m
from the analytic rank, so it does not complete X3.

## Quotient out coordinate changes before searching

Under P'(t)=U(t)P(t)V(t) with U,V invertible and U(0)=V(0)=I,

    B'_i = B_i + U_i A + A V_i.

The additional terms induce zero from ker A to coker A. Thus beta_i is
unchanged. In particular, pure changes of basis of a constant presentation
produce beta_i=0, even if their displayed matrix derivatives are nonzero.
Such changes lift to every order but cannot create leading nonvanishing.

This also applies to representation deformations which are mere isomorphisms
of the entire Selmer datum, when the induced finite model is related by these
compatible equivalences. A nonzero matrix derivative alone is not evidence
of a nontrivial arithmetic tangent direction.

## Executed RAD test

`selmer_jet_source.rad` computes in K[s,t]/(s^2,t^2) with integer coefficients.
For A+sB+tC+stD it evaluates [st]det by exact jet multiplication. It tests
243 forked cases: constant matrices of corank three, two, and one, each with
81 choices of first-order entries and nonzero higher corrections.

At corank two the direct coefficient agrees with

    B_22 C_33 + C_22 B_33 - B_23 C_32 - C_23 B_32

(zero-based indices) independently of D and of the other entries. At corank
three it vanishes. Corank-one cases exhibit genuine dependence on D, checking
that the theorem's scope is not silently extended.

The independent verifier evaluates ordinary determinants by rational Gaussian
elimination on a five-by-five parameter grid. Exact polynomial interpolation
extracts the st coefficient, using a different method from RAD's jet algebra.
It also checks isolated forks, one/four worker agreement, recorded replay, and
rejection of a forged arithmetic-provenance flag. WHY() records the settled
coefficients and correction contributions.

Run:

    python projects/dogfood/bsd-x/verify_selmer_jet.py

The universal statements above are proved in this note. The finite executable
checks are not a foundational formal proof of them or of the Galois inputs.

## Remaining arithmetic work

The supplied mixed cocycle lifting equation and local compatibility conditions
remain obligations for actual arithmetic data. This experiment has not solved
them. Nor does first-order reduction prove that the complex leading derivative
forces any particular deformation coefficient to be nonzero. All claims of
arithmetic provenance and universal X3 remain false in the emitted reports.

What is established is that, in the c>=m regime, solving more higher-order
lifting equations cannot by itself repair a zero target coefficient. One must
change the induced first-order maps, prove their arithmetic admissibility, and
establish the analytic comparison. The algebra now separates those tasks.
