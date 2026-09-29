# Global finite-energy matching of the initial collar pressure

This constructs a specific globally localized initial velocity and its
unique decaying pressure. It fixes the collar's harmonic pressure terms
through the exterior data, rather than leaving them arbitrary. It does
not establish a blowup solution or convergence of the subsequent time jet.

## Specify the complete velocity

Let rho(s)=exp(-1/s) for s>0 and rho(s)=0 for s<=0. Define

    S_ab(q) = rho((q-a)/(b-a)) /
              [rho((q-a)/(b-a)) + rho(1-(q-a)/(b-a))],
    chi(q) = [1-q S_(1/4,3/8)(q)] [1-S_(5/8,1)(q)].

Then chi is smooth, equals 1 for q<=1/4, equals 1-q throughout
3/8<=q<=5/8, and equals 0 for q>=1. This specifies both transition
regions, not just the collar. With q=|x|^2 and A=diag(1,2,-3), set

    U_0(x) = curl[-chi(q)(x cross Ax)/3].

U_0 is smooth, divergence-free, compactly supported, and therefore has
finite energy and rapidly decaying derivatives. On the open collar
3/8<q<5/8 it agrees exactly with the previously checked linear polynomial
velocity. The compact support satisfies the initial-data decay requirement
in [Clay's statement, condition (4)](https://www.claymath.org/wp-content/uploads/2022/06/navierstokes.pdf).

## Determine the global pressure

Let N_0=(U_0 dot grad)U_0 and F=-div N_0. The pressure at the reference
time is fixed by

    Delta p_0 = F,  p_0(x) -> 0 as |x| -> infinity,
    p_0(x) = -(1/4pi) integral_R3 F(y)/|x-y| dy.            (1)

F is smooth and compactly supported. Thus (1) defines a globally smooth
pressure. Uniqueness follows because the difference of two such pressures
is an entire harmonic function tending to zero at infinity. No arbitrary
harmonic addition remains after imposing this exterior condition.

Here the three-dimensional integral reduces to three one-dimensional
integrals. Write

    Q=x dot Ax,  P=x dot (A^2-(14/3)I)x,
    H_4=Q^2-(4/7)qP-(28/15)q^2.

P and H_4 are homogeneous harmonic polynomials of degrees 2 and 4.
Put a=chi+(2q/3)chi', b=-(2/3)chi', and

    d_2=4a(a'+b)+8q a'b,
    d_4=11b^2+4a'b+4(a')^2+4ab'+12qbb'+8qa'b'+4q^2(b')^2.

Direct expansion of -tr[(grad U_0)^2] gives

    F=f_0(q)+f_2(q)P+f_4(q)H_4,
    f_0=-[14a^2+(14/3)q d_2+(28/15)q^2 d_4],
    f_2=-[d_2+(4/7)q d_4],  f_4=-d_4.

Consequently

    p_0=g_0(q)+g_2(q)P+g_4(q)H_4,
    4q g_l''+(4l+6)g_l'=f_l,  l=0,2,4.

The regular, decaying solution is explicit. With s_l=l+1/2,

    g_l(q)=-1/(4l+2) [q^(-s_l) integral_0^q t^(s_l)f_l(t)dt
                                + integral_q^infinity f_l(t)dt].   (2)

At q=0 use the regular limit. Differentiation verifies the radial ODE;
the upper support cutoff is 1, so the integrals have finite ranges.
All quantities in (2) are specified by the chosen smooth cutoff.

## Match to the local polynomial pressure

Within the collar, chi=1-q and the radial sources simplify to

    f_0=-14+(196/3)q-(2254/45)q^2,
    f_2=4-(92/21)q,  f_4=-104/9.

A convenient local particular pressure uses

    g_0^loc=-(7/3)q+(49/15)q^2-(1127/945)q^3,
    g_2^loc=(2/7)q-(23/189)q^2,
    g_4^loc=-(52/99)q.

On the whole connected collar the exact matching relation is

    g_l(q)=g_l^loc(q)+C_l+D_l q^(-s_l).                    (3)

The two constants for each mode are fixed, not adjustable. At q_*=1/2,
evaluate (2) and its derivative, then set

    D_l=-(q_*^(s_l+1)/s_l)[g_l'(q_*)-(g_l^loc)'(q_*)],
    C_l=g_l(q_*)-g_l^loc(q_*)-D_l q_*^(-s_l).              (4)

The homogeneous Wronskian -s_l q^(-s_l-1) is nonzero. Thus (4)
matches both value and radial derivative, and the ODE gives equality
throughout the collar. Global formula (2) already supplies smooth matching
through both transition regions; no artificial surface force is introduced.

The earlier x-antiderivative polynomial P_0^old is a different particular
solution. Its exact replacement on the collar is

    p_0=P_0^old+h_0,
    h_0=[g_0^loc+g_2^loc P+g_4^loc H_4-P_0^old]
                         + sum_{l=0,2,4}(C_l+D_l q^(-s_l))H_l,

where H_0=1 and H_2=P. Every term in this difference is harmonic there.
This explicitly matches the previously selected polynomial pressure; it
does not assume its harmonic part was already the globally correct one.

For orientation, non-certified 30/45-digit quadrature gives

| l | C_l, approximately | D_l, approximately |
|---|---:|---:|
| 0 | 0.21676497114309346 | numerically zero |
| 2 | -0.14285714285714286 | 0.0026346838184445384 |
| 4 | 0.71504324383556604 | 0.0022165947302637295 |

The exact definitions are the integrals (2)–(4), not these rounded values.
The quadrature comparison is a diagnostic, not a rigorous interval enclosure.
In particular, numerical zero is not used as proof of D_0=0.

## Energy and the first globally matched time coefficient

The source also equals -partial_i partial_j(U_0i U_0j). Integration by
parts shows its monopole and dipole moments vanish. Equivalently, the
l=0 exterior moment in (2) vanishes. The pressure therefore has

    p_0=O(|x|^(-3)),  grad p_0=O(|x|^(-4))

outside the support; the l=4 part decays still faster. The first actual
globally compatible time coefficient is

    U_1^global=nu Delta U_0-N_0-grad p_0.                  (5)

It is smooth and divergence-free, has an O(|x|^(-4)) exterior tail, and
lies in L^2. The initial energy identity follows directly:

    integral U_0 dot U_1^global = -nu integral |grad U_0|^2.

On the collar, (5) differs from the old polynomial coefficient by

    U_1^global=U_1^old-grad h_0.

The correction has zero curl in the collar, but it changes subsequent
advection. Accordingly the old U_2,U_3 must not be relabeled as globally
matched coefficients.

## Continue globally instead of reusing the old local pressures

For the next step form N_1 from U_0 and U_1^global, solve
Delta p_1=-div N_1 with decay at infinity, and set
2U_2^global=nu Delta U_1^global-N_1-grad p_1. At this stage N_1 is still
compactly supported because each product contains U_0 or a derivative of
U_0. Outside the support U_1^global=-grad p_0 is harmonic, so U_2^global
again has an O(|x|^(-4)) tail and finite energy.

At the following step U_1^global interacts with itself, producing a
noncompact but decaying source. One must retain that exterior source in
the Poisson solve. Repeatedly imposing the old polynomial antiderivative
pressure would undo the global matching just constructed.

There is also an exact definition of every finite globally matched jet.
Let the Leray projection have Fourier multiplier
P(xi)=I-xi xi^T/|xi|^2, with the value at xi=0 immaterial for L^2 functions.
Recursively set

    N_n=sum_{i+j=n}(U_i^global dot grad)U_j^global,
    p_n=R_i R_j sum_{a+b=n}(U_ai^global U_bj^global),
    U_(n+1)^global=[nu Delta U_n^global - P N_n]/(n+1),    (6)

where R_i denotes the Riesz transform with multiplier -i xi_i/|xi|.
Thus grad p_n=-(I-P)N_n, and (6) satisfies the pressure and momentum
coefficient equations globally. Products at each order use the globally
corrected coefficients, not their old polynomial approximations.

For any fixed finite n, these formulas give smooth finite-energy velocity
coefficients: start with U_0 in every H^s, use the Sobolev algebra estimate
for s>3/2 and the boundedness of the projection multipliers on H^s, and
induct using two additional derivatives at each step. This works because
the induction hypothesis provides every finite Sobolev order. It supplies
no uniform control as n grows. Riesz transforms likewise give p_n in every
H^s from the smooth L^2 tensor products. This is a written functional
analytic argument, not part of the finite RAD certificate.

This settles the initial pressure matching and defines globally matched
coefficients at each finite order. It does not prove that an infinite jet converges or
that any resulting evolution blows up. A truncated jet with a residual
also does not automatically produce a force meeting Clay's all-order
spatial decay conditions.

## What was checked

`verify_global_pressure.py` checks the general harmonic source decomposition
using symbolic derivatives of an arbitrary radial cutoff. `global_pressure.rad`
checks the exact local radial ODE coefficients and both homogeneous
exponents for l=0,2,4, using forks and causal `why()` records. One/four-worker
output and recorded-world replay agree. The optional `--quadrature` computes
the matching constants for the specified smooth cutoff at two precisions.

The Newton integral, uniqueness, smoothness, matching, and energy arguments
above are written analytic deductions, not RAD-kernel-verified PDE proofs.
