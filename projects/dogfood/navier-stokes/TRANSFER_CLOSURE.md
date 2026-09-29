# A quantitative audit of the next-scale implication

Update: `RECEIVER_PROFILE.md` closes the coarse whole-profile relative-error
gap below using a stronger H7 argument on the same short interval. It does
not close repetition: the computed receiver's standalone nonlinear production
is negative. The small transferred-energy upper bound below still applies.

This audit applies to the actual finite transfer proved in
`EVOLVING_TRANSFER.md`. It distinguishes a genuine upper bound on transferred
energy from an inadequate upper bound on approximation error. Neither is a
general impossibility theorem for Navier-Stokes blowup.

## The actual new-band energy fraction is small

Let P_> select |k|^2>2, the band absent from the initial datum v. The actual
solution satisfies ||u-v||_(H^3)<=delta=2^-18 on [0,2^-98]. For integer
wavevectors in this band, (1+|k|^2)^3>=64. Therefore

    K_> = ||P_> u||_2^2/2 <= delta^2/128 = 2^-43.

Also ||v||_2^2=24576>(313/2)^2, and delta<1/2, so ||u||_2>156.
The total kinetic energy K is greater than 156^2/2>2^13. Thus

    K_>/K < 2^-56

throughout the certified interval. This is an upper bound on the actual
transfer, not just a weak positive lower bound. It excludes, for example,
an endpoint target requiring half the kinetic energy above the old band.
Any fixed target fraction at least 2^-56 is excluded on this interval.

A large fraction of total energy is not necessary for every conceivable
blowup mechanism. Highly localized singular behavior may involve little
kinetic energy. This calculation rules out that particular type of handoff;
it does not rule out a localized cascade or a later transfer.

## A tiny absolute error is not necessarily a useful normalized error

At T=2^-98, the demonstrated target amplitude has lower bound
a(T)>=1024 T=2^-88. The all-mode H^3 profile-error bound is 2^49 T=2^-49.
Dividing the available error bound by the proved signal bound yields

    ||u-W||_(H^3)/a <= 2^39.

This upper bound does not certify a small relative error around a new
profile of that signal size. It also does not prove the actual relative
error is large. Improving the estimate could change this conclusion.

The single observed coefficient has a much sharper bound:

    |u_hat(k)_z-W_hat(k)_z|/a <= (3/64)/1024 = 3/65536.

Thus the earlier mode-specific functional genuinely resolves a positive
transfer. It does not, by itself, control the phases, polarizations and all
other modes needed as input to another amplification argument. Accuracy of
one coefficient is not accuracy of the entire next profile.

## What would close a specified construction

For an evolving multiscale family V_q, define the exact solution map over
the proposed stage with the prescribed force. A usable handoff theorem
would map its specified entrance set into the next entrance set, while
bounding error in the normalization of V_(q+1), not merely in fixed physical
units. It must also prove the growth criterion and stage-time summability
that imply loss of regularity. These objects are not supplied by the current
single-mode certificate.

For example, if a chosen proof uses an invariant normalized error ball,
it could seek an estimate r_(q+1)<=rho_q r_q+d_q and verify
rho_q r_max+d_q<=r_max, together with a rigorously realized transfer profile.
This is a conditional proof interface, not a fluid estimate we have found.
One must not mark the missing estimate true merely because its scalar
inequalities would imply the desired result.

The new audit retires two invalid deductions: that the present short interval
has transferred a significant fraction of total energy, and that accurate
tracking of one new coefficient establishes a whole next-stage profile.
Neither a repeatable cascade nor blowup follows. No successful replacement
for the missing handoff theorem is claimed here.

## RAD evidence

`transfer_closure.rad` checks the weighted Fourier projection arithmetic,
energy fraction separation and both relative-error scales. Its checked
resolver rejects forged next-profile and repeatability claims. WHY records
the provenance of the diagnosis. The continuum projection argument is the
written lemma above; RAD is not a foundational PDE proof kernel.

Run `python -X utf8 projects/dogfood/navier-stokes/verify_transfer_closure.py`.
No extra Python packages are required.
