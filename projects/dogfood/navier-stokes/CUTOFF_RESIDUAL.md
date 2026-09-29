# A concrete pressure-independent localization residual

This calculation chooses an actual smooth compactly supported solenoidal
extension of an affine core and identifies a viscous residual that pressure
cannot absorb. It does not construct the compensating flow.

Take A=diag(1,2,-3), so tr A=0 and the affine field Ax is genuinely
three-dimensional without rotational symmetry. Put q=|y|^2 and Q=y dot Ay.
Choose a smooth radial cutoff chi(q) equal to one near q=0, equal to
(1-q)^2 in a neighborhood of q=1/2, and zero for q>=1. Smooth interpolation
between these disjoint closed regions is possible. Define

    Psi(y)=-(1/3) y cross Ay,
    V(y)=curl(chi(q) Psi(y)).

The vector identity curl(y cross Ay)=-3Ay uses tr A=0. Consequently V=Ay
in the core, V is compactly supported, and div V=0 everywhere.
Direct expansion gives

    V = [chi+(2q/3)chi'] Ay - (2/3)chi' Q y,

where primes differentiate q. On the specified collar chi=(1-q)^2, so

    V = a(q) Ay + b(q) Q y,
    a(q)=1-(10/3)q+(7/3)q^2, b(q)=4/3-(4/3)q.

Because A is symmetric, curl(Ay)=0 and grad Q=2Ay. With H=y cross Ay,

    curl V = 2(a'-b) H = (-28/3+12q) H.

Every component of H is a homogeneous harmonic polynomial of degree two.
The product identity in dimension three gives Delta(q H)=14H. Hence

    Delta curl V = 168 H

throughout this collar. At y=(1/2,1/2,0), H=(0,0,1/4), so

    (Delta curl V)(1/2,1/2,0)=(0,0,42).

For u(x,t)=tau^(beta-1)V(x/tau^beta), the viscous contribution to the curl
of the required force is therefore

    -nu tau^(-1-2beta) Delta curl V.

It is nonzero and divergent at the moving point x=tau^beta(1/2,1/2,0).
No pressure adjustment changes its curl. The time/nonlinear contributions
also have to be included. For beta=4/9 define

    B(V)=(1-beta)V+beta(y dot grad)V+(V dot grad)V.

Differentiating this explicit polynomial on the collar gives

    (curl B(V))(1/2,1/2,0)=(0,0,191/216).

Thus the third component of the full pressure-free force residual at this
moving point is exactly

    (191/216) tau^(-2) - 42 nu tau^(-17/9).

It diverges positively for every fixed nu>0. The two terms have different
exponents and cannot cancel as tau tends to zero. This explicitly rejects
the chosen cutoff profile as a smooth-force construction. The broader
fixed-profile energy argument also excludes different compact profiles in
the same beta range.

An independent SymPy differentiation verified div V=0, the full polynomial
identity Delta curl V=168(y cross Ay), and both pointwise coefficients.
`verify_cutoff.py` reproduces this finite symbolic check; SymPy is an
external algebra checker, not a RAD kernel or an analytic proof assistant.

What a correction must do is now explicit: cancel this pressure-independent
collar residual (and the larger leading inviscid residual), while matching
the affine core and maintaining the actual smooth-force bounds. Declaring
the core harmonic does not cancel the collar term. A different cutoff might
alter its shape, but a nonzero smooth compact solenoidal V cannot have
Delta curl V=0 globally: compact harmonic curl V is zero, and then the
divergence/curl identity makes V harmonic and zero as well.
