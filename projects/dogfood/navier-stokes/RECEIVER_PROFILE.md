# A certified receiving profile, and its failed autonomous restart

This improves the coarse relative-error audit in `TRANSFER_CLOSURE.md`.
The entire new frequency band, rather than one coefficient, now has a small
proved relative error around a specified profile. Its direct promotion to
an autonomous amplifying seed nevertheless fails an exact nonlinear test.
This is not a repeatable cascade or a blowup construction.

Use the existing five-wave datum v, prescribed smooth force, viscosity 1,
and T=2^-98. Let N(u)=-P div(u tensor u) be the nonlinear velocity derivative,
P_> the projection to |k|^2>2, and Q=P_>N(v). Q is a nonzero, explicit,
divergence-free 16-mode field. RAD computes it from full Fourier convolution;
no trajectory derivative or force correction is independently prescribed.

## Higher regularity improves the whole-profile error

For integer s>=2, the lattice l^1 estimate used in the previous proofs gives

    ||a tensor b||_(H^s) <= 2^(s+3) ||a||_(H^s)||b||_(H^s).

One uses (1+|p+q|^2)^(s/2) <= 2^(s-1) times the sum of the p and q
weights, Young's convolution inequality, and the Fourier l^1 bound by
8 times the H^s norm. These are full infinite-series estimates.

The seed satisfies ||v||_(H^7)^2=6945*4096<8192^2. The force has a global
H^7 bound of 32768: a pulse has norm at most
2 a_q (1+k_q^2)^(7/2) <=32 a_q k_q^7 =2^(3-q^2+7q)<=2^15,
and only one pulse is active at each time.

Take B=16384. The mild H^7 bilinear constant is 2048 sqrt(T), so its
radius-B quadratic contribution is 2^-10 and its Lipschitz constant is
2^-23. The linear part is bounded by 8192+32768T. These bounds give
a contraction in the radius-B ball of C([0,T];H^7). Uniqueness identifies
this solution with the H^3 solution already studied.

The PDE now gives a strong H^4 time-derivative estimate:

    ||u_t||_(H^4) <= ||u||_(H^6) + 256||u||_(H^5)^2 + ||f||_(H^4)
                  <= B + 256 B^2 + 32768 < 2^37.

Thus ||u(t)-v||_(H^4)<=2^37 t. This is the useful change from the earlier
square-root-in-time displacement bound. No differentiation of an uncontrolled
tail is assumed: the stronger H^7 solution bound supplies the derivatives.

For the first Picard profile W defined in `EVOLVING_TRANSFER.md`,

    ||N(u(s))-N(v)||_(H^3)
      <=128 (||u(s)||_(H^4)+||v||_(H^4)) ||u(s)-v||_(H^4)
      <=2^59 s.

Heat contraction and integration therefore give

    ||u(t)-W(t)||_(H^3) <=2^58 t^2.

This supersedes the weaker O(t) whole-profile bound on the same interval.
The old bound was correct; it was insufficient for normalized profile control.

## A specified next-band shape with small normalized error

The initial datum and the force have no high-band component on [0,T]. Hence

    P_> W(t) = integral_0^t exp((t-s)Delta) Q ds.

Also ||Q||_(H^5)<=512||v||_(H^6)^2<=2^35. Subtracting tQ and using
||(exp(r Delta)-1)Q||_(H^3)<=r||Q||_(H^5) yields

    ||P_>u(t)-tQ||_(H^3) <= (2^58+2^34)t^2 <=2^59 t^2.

The exact coefficient Q_(2,1,0),z=-2048 implies ||Q||_(H^3)>=2048.
For every 0<t<=T, the full normalized receiving-band estimate is therefore

    ||P_>u(t)/t-Q||_(H^3) / ||Q||_(H^3)
       <=2^48 t <=2^-50.

This controls all errors in the new band, including arbitrarily high modes
outside the 16-mode leading profile. It does not assert that the whole
velocity is close to tQ: the original low-band flow is still present.

## WHY the accurately generated profile does not restart autonomously

Define r=Q/2048. RAD independently applies the full Navier-Stokes Fourier
operator to r, including every generated output mode. In the normalized
periodic enstrophy identity it obtains

    nonlinear production S(r) = -7,
    ||Delta r||_2^2 = 443,
    E'(r) = -450                        (viscosity 1, zero force work).

The force work is zero at this hypothetical state during our interval because
the prescribed force occupies the original axis modes and r occupies only
the higher band. By homogeneity, for any positive amplitude A,

    E'(A r) = -7 A^3 -443 A^2 < 0.

Thus increasing the amplitude of this exact isolated receiving profile does
not restore the desired initial nonlinear amplification. In particular the
new high band, whose leading amplitude is A=2048t, is not an autonomous
copy of the original positive-production seed.

This does not assert that the full solution loses its previously proved
positive production. Its full velocity retains the original low-band flow.
Writing u=l+h, the equation contains N(l), the two mixed interactions, and
N(h). The leading source creating h here is N(v), not N(h). Discarding l
changes the dynamics. A short negative derivative of r also does not prove
that r can never grow later; it excludes the claimed immediate autonomous
restart with positive production.

## What is now resolved and what remains

The finite solution map really does enter a specified receiving-band profile
with small normalized error. The former whole-profile error gap has been
closed on this very short interval using higher regularity.

An indefinite construction still needs a controlled evolution of the coupled
low and high bands that drives a further transfer, with scale-uniform error
control and growth sufficient for singularity. Neither keeping the low flow
frozen indefinitely nor deleting it is justified. The negative exact restart
test tells us not to identify a receiver profile with a self-amplifying seed.
It does not reveal a successful coupled transfer mechanism.

## Verification boundary

`receiver_profile.rad` computes Q, converts r to an exact rational input grid,
computes its full self-interaction, and checks the H^7 and error arithmetic.
WHY records this checked evidence and rejects forged positive production or
repeatability. The higher-regularity PDE argument above is a written analytic
proof, not formalized in RAD's kernel.

`verify_receiver_profile.py` independently recomputes the complete receiver
and its production/dissipation, verifies the force exponent identity, and
checks the rational contraction and normalized-error bounds. Run:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_receiver_profile.py
```

No kernel changes or new proof axioms were needed.
