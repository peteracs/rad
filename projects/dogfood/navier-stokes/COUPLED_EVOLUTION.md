# Coupled donor and receiver evolution with controlled remainders

This supplies a quantitative short-time evolution of the original and
receiving profiles together. It explains the positive transfer even though
the isolated receiver's nonlinear enstrophy production is negative. It does
not supply an indefinitely repeatable cascade.

Use the same actual solution u, initial datum v, prescribed smooth force,
viscosity 1 and interval [0,T], T=2^-98, as in `RECEIVER_PROFILE.md`.
Write N(u)=-P div(u tensor u), Q=P_(|k|^2>2)N(v), and r=Q/2048.
All inner products below use normalized periodic measure and real fields.

## Exact coordinates, not a finite-mode replacement for the PDE

The Fourier supports of v and r are disjoint. Define

    V=||v||_2^2=24576, R=||r||_2^2=117/5,
    a(t)=<u(t),v>/V, b(t)=<u(t),r>/R,
    z(t)=a(t)v+b(t)r, w(t)=u(t)-z(t).

Then w is L^2-orthogonal to both profiles. It contains all their shape
changes and all other modes. It is never set to zero. Initially a=1,b=0,w=0.
The H^7 proof gives ||u||_(H^7)<=16384 and
||u(t)-v||_(H^4)<=2^37 t. Since the coordinate projection is L^2-orthogonal,

    ||w(t)||_2<=2^37 t,
    |a(t)-1|<=2^37 t, |b(t)|<=2^37 t.

In particular 3/4<=a<=5/4 and |b|<=1/16 on the whole interval.

## Polarization exposes the nonlinear transfer and its feedback

Full Fourier convolution gives

    C=<r,N(v)>=239616/5,
    D=<v,N(r)>=32/3,
    ||grad v||_2^2=36864, ||grad r||_2^2=97.

Let B(v,r)=N(v+r)-N(v)-N(r) be the mixed interaction. The exact identities

    <r,B(v,r)>=-D, <v,B(v,r)>=-C

also follow by comparing coefficients in <av+br,N(av+br)>=0. RAD verifies
both identities directly from the polarized full Fourier operator, rather
than assuming a finite-mode evolution conserves energy.

Projecting the actual PDE yields the exact equations

    a' = -(3/2)a -(39/20)ab + b^2/2304 + F_a + R_a,
    b' = 2048a^2 -(485/117)b -(160/351)ab       + R_b,

where F_a=<v,f>/V, and for phi=v or r,

    R_phi = <phi, Delta w + N(u)-N(z)> / ||phi||_2^2.

The force projection onto r vanishes on this early interval. The bounded
force projection onto v is retained. These are exact projection identities
for the full PDE, not autonomous equations in a and b.

For the two-profile part, nonlinear energy flow into the receiving coordinate
is

    <br,N(z)> = ab(Ca-Db).

Its opposite appears in the donor coordinate, so the exchange respects
kinetic energy. It is positive for a,b>0 and b/a<C/D=22464/5. The term
2048a^2 in b' is the leading source. Removing the original flow removes it.

## Every omitted mode is covered by the remainder bounds

Both profiles have Fourier support |k|^2<=6 and L^2 norm greater than 1.
For either phi, ||phi||_(H^3)<=27||phi||_2, and
||grad phi||_infinity<=8||phi||_(H^3). Integration by parts and Cauchy-Schwarz
therefore give

    |R_phi| <= [8+216(||u||_2+||z||_2)] ||w||_2
             <= 2^24 ||w||_2 <= 2^61 t.

Here ||z||_2<=||u||_2<=16384. No derivative of w is required for this
estimate; the derivatives are moved onto the fixed smooth test profile.
This is the useful functional trick: the projected equations admit an
all-mode error bound without pretending the rest of the flow is absent.
At T, both remainder bounds are at most 2^-37.

The established force estimate ||f||_2<1 also gives |F_a|<1. Consequently,
on the entire interval,

    b' >= 2048*(3/4)^2 -(485/117)/16 -(160/351)*(5/4)/16 -2^-37
        > 1024.

Thus b(t)>=1024t>0 for t>0. Using that sign in the donor equation,

    a' <= -9/8 + 1 + 1/(256*2304) + 2^-37 < 0.

The actual donor coordinate decreases while the actual receiver coordinate
increases. Their kinetic coordinate energies V a^2/2 and R b^2/2 therefore
respectively decrease and increase. The earlier total-energy decrease proof
continues to apply. These are actual solution inequalities with quantified
remainders, not sampled vector-field values.

## Why this does not close an infinite iteration

There is an explicit nonzero omitted mode. At k=(1,0,1), both v and r have
zero coefficient, but u'_k(0)=-2048 i e_y. Therefore w'_k(0) is nonzero;
the span of v and r is not invariant. Deleting w changes the initial PDE
derivative, even before contemplating long-time dynamics.

The exact identities explain the earlier receiver-only test: r can receive
energy through N(v) and mixed interactions even though its standalone
nonlinear enstrophy production is negative. They do not prove that the
resulting new collection of modes generates a further suitable donor/receiver
pair at a finer scale. The present remainder estimate grows with time and
is not a scale-uniform reset condition. Nor does the positive b' bound alone
establish accelerating transfer through unbounded frequencies.

What is now supplied is a proved finite coupled evolution and the precise
interaction responsible for it. The remaining problem is an actual repeated
handoff of this coupled structure, with inherited phases, pressure, tails,
viscosity and admissible forcing controlled. No such handoff is claimed.

## RAD verification

`coupled_modes.rad` builds d=v/64 and r on an exact rational grid, evaluates
N(d), N(r), and N(d+r), and checks both directions of the polarized energy
exchange. The small normalization keeps coefficients inside the existing
operator's domain; it does not alter the physical viscosity or equation.
It then checks the bounds for the actual v,r coordinates. WHY records the
accepted evidence and rejects forged finite-mode invariance or repetition.

`verify_coupled_modes.py` independently computes the pairings, polynomial
projection identities, the nonzero omitted-mode witness, and the rational
error/growth margins. The continuum remainder and existence lemmas are
written above and in `RECEIVER_PROFILE.md`, not formalized in RAD's kernel.

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_coupled_modes.py
```
