# Using WHY for checked repair proposals

`why()` walks RAD's recorded provenance ledger. It does not invert a
nonlinear PDE or synthesize a missing theorem. Its implementation is in
`core/vm/src/vm/builtins_impl/world_loading_and_sandbox.rs`, `bi_why`.
`model_check` additionally has trace shrinking for executable failing
histories; it is not a quantifier eliminator for infinite-dimensional states.

The useful arrangement is to record mathematical diagnostics as data, derive
repair proposals from them, check each proposal in an isolated fork, and
settle only independently recomputed evidence. WHY then explains the recorded
selection. The repair generator supplies the discovery step; WHY supplies
provenance. The full structured evidence is printed separately because the
human-readable WHY display abbreviates long values.

## Executed prototype

`residual_why.rad` calls the full Fourier force operator for a two-wave
state. In physical coordinates at viscosity one:

    u = (2 cos z, 2 cos x + 2 cos z, 2 cos x).

The generated (1,0,1) mode has force coefficient 2i in component y when
ut=0. The repair generator reads that computed coefficient and proposes
ut_y=4 sin(x+z), with its conjugate Fourier partner. A second full residual
evaluation verifies the cancellation. With p=4 sin x sin z, direct
physical-space differentiation gives

    ut + (u.grad)u + grad p - Delta u = u.

Thus the selected derivative removes the nonlinear force coefficient while
leaving four nonzero Fourier force modes. It does not eliminate viscosity,
establish a small force budget, or construct a time-dependent solution.

The two forks preserve the authoritative world. The resolver recomputes
each diagnosis and records the selected local repair. The trace is

    RepairDecision <- ChooseRepair <- RepairChecked <- SubmitRepair.

The structured decision explicitly includes `stage_certified: false`.
Changing a proposal to claim repeatability causes rejection. The independent
verifier checks physical-space algebra, both diagnoses, provenance, forged
proof rejection, deterministic worker-count output and recorded-world replay.

Run with SymPy available:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_residual_why.py
```

## What a more capable synthesis loop still needs

The demonstration's repair space contains one computable coefficient change.
It is not an unrestricted search for trajectories. A larger search needs
an explicit grammar of profile changes and executable or analytic checks
for their consequences. Diagnostics should retain the offending mode,
derivative order, stage scale, residual terms and the unmet bound.

A proposed cancellation must be rechecked against the full residual and
subsequent evolution. All-order estimates and stage-to-stage closure require
separate analytic certificates. WHY can record their provenance once supplied;
it cannot turn their absence into a proof. The prototype changes no kernel.
