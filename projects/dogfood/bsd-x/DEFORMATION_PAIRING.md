# Deriving a pairing and vanishing from a deformation

This is an independently worked algebraic construction, with no web input.
The matrix families tested are synthetic. No Selmer deformation or analytic
comparison theorem has been constructed. No universal X has been filled.

## A general vanishing lemma

Let K be a field and M(T) an n-by-n matrix over K[[T]], with nonzero
determinant. Put c=dim_K ker M(0). Then

    ord_T det M(T) >= c.

Proof: constant invertible row and column operations put M(0) in the form
diag(I_(n-c),0). Write the resulting matrix in blocks

    M(T) = [[A(T), B(T)], [C(T), D(T)]].

Here A(0)=I, B(0)=C(0)=D(0)=0. The block A is invertible over K[[T]], and

    det M = det A * det R,    R = D-C A^(-1) B.

Every entry of the c-by-c matrix R is divisible by T. Thus det R is
divisible by T^c. The row and column changes and det A are units, so they
do not change the vanishing order. The cases c=0 and c=n follow with the
same empty-block conventions. QED.

No nondegenerate pairing is needed for this inequality. That requirement
in the earlier proposed pairing route is unnecessary if one can instead
construct the appropriate determinant presentation and arithmetic comparison.

## The pairing is derived, not inserted

Write M=M_0+T M_1+T^2 M_2+... . There is an induced linear map

    beta: ker M_0 -> coker M_0,    x -> [M_1 x].

It can be computed by lifting x to x+T x_1+... and taking the coefficient
of T in M(T)(x+T x_1+...). That coefficient is M_1 x+M_0 x_1, whose class
in the cokernel is independent of the lift.

For invertible formal basis changes M'=U M V, the derivative is

    M'_1=U_0 M_1 V_0+U_1 M_0 V_0+U_0 M_0 V_1.

On ker(M'_0), the second summand vanishes and the third is zero in the
cokernel. Thus beta transforms compatibly under the induced kernel and
cokernel isomorphisms. It is not an arbitrary Gram matrix assigned after
the fact.

If an arithmetic construction further supplies an identification of the
cokernel with the dual of the kernel, beta defines a bilinear pairing.
Symmetry requires compatible additional structure; it is NOT automatic
for every matrix. Identifying the full rational Selmer space with this
kernel, and supplying the duality, are still unproved inputs in this project.

In the normalized block coordinates, beta has matrix D_1. The next
coefficient of R is

    R_2=D_2-C_1 B_1,

not D_2 alone. (For general invertible A_0, insert A_0^(-1) between C_1
and B_1.) Higher coefficients similarly depend on the full deformation.
If beta is invertible, det R has order exactly c. If beta is singular,
the determinant has higher order, or vanishes identically. This is a
vanishing theorem for the FORMAL DETERMINANT, not yet for L(E,s).

## RAD's cancellation witness

The program constructs

    M(T) = [[1, bT, 0], [cT, hT+kT^2+aT^3, 0], [0,0,T]].

It computes the determinant from polynomial multiplication and all six
permutations, obtaining

    det M(T)=hT^2+(k-bc)T^3+aT^4.

The constant corank is 2. The first induced pairing in the displayed
kernel/cokernel coordinates is diag(h,1). The corrected second coefficient
in its potentially degenerate direction is k-bc.

For h=0, k=a=b=c=1, the raw second coefficient is 1, but its correction is
-1. It cancels exactly, leaving det M=T^4. WHY records the raw coefficient,
correction, effective coefficient and actual vanishing order separately.
When a=0 as well, the entire determinant is zero; the program marks this
case explicitly, rather than mistaking it for a finite analytic rank.

Left and right formal shears, both identity at T=0, change parameters by

    b'=b+v, c'=c+u, k'=k+ub+cv+uv.

The raw coefficient k changes while k'-b'c'=k-bc. RAD checks the full
determinant under all nine shears u,v in {-1,0,1} for each of 243 families.
The symbolic identity above proves invariance for arbitrary u,v.

## The exact arithmetic theorem that would finish X3 along this route

For every E/Q with analytic rank m>=2, construct at some good p>=5:

1. A square matrix M_(E,p)(T) over Q_p[[T]] whose determinant is nonzero.
2. An injective Q_p-linear map from the FULL rational Selmer space S_p
   into ker M_(E,p)(0).
3. A proof that ord_T det M_(E,p)(T) <= m.

Then the proved general lemma gives

    r+t_p = dim S_p <= dim ker M_(E,p)(0)
          <= ord_T det M_(E,p)(T) <= m,

which proves X3. The pairing and its nondegeneracy are not necessary for
this inequality. They may describe leading coefficients if further
identifications are constructed, but that is a separate task.

Item 3 is the missing link to complex analytic rank. A p-adic determinant
must not silently be identified with the complex Taylor series of L(E,s).
Declaring the determinant's order to be m, or taking M=T I_m without
constructing the injection in item 2, simply assumes the desired result.
The algebraic experiment proves none of these three universal arithmetic
construction/existence assertions. It makes the proposed mechanism and
the exact missing comparison more explicit.

## Reproduce

    python projects/dogfood/bsd-x/verify_deformation.py

Verification includes exact evaluation of the reported determinant at ten
integer arguments against independent rational Gaussian elimination. Both
polynomials have degree at most nine, so these ten comparisons establish
their equality for each tested matrix. Additional tests check worker-count
determinism, replay, and rejection of fabricated arithmetic identification.
The arbitrary-size formal lemma is proved above, not in a foundational
formal proof kernel. All matrix data here are synthetic.
