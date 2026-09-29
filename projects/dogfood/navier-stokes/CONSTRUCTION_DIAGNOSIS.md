# A WHY application that identifies missing construction constraints

Run `construction_why.rad` with `--experimental-laws --strict-types --deny-warnings`.
It evaluates six proposals in isolated forks, computes exact endpoint force
derivatives, checks adjacent-stage closure, and records an actionable diagnosis.
The resolver recomputes every proposal. WHY supplies the provenance of the
diagnosis; it does not invent its mathematics.

**Scope:** this explains obstructions in the implemented trajectory grammar
under an explicitly chosen diagnostic budget. It does not prove that no
smooth-forced blowup construction exists. The output explicitly records
`universal_impossibility_proved: false`.

## The structural finding

For u=A(s)+theta(s)B(s), theta and all its derivatives vanish at s=0.
Thus changing any smooth B leaves **every** entrance velocity derivative
unchanged. The same holds for every entrance force derivative, because the
force is a differential expression in u plus the spatial Leray projection.
This is a written consequence of the fixed-step flatness lemma; the three
tested B choices are examples, not its proof.

Consequently an interior-only repair search has identically zero influence
on an already incorrect entrance force derivative. It cannot repair such
an endpoint by trying more B coefficients.

Let N(u)=P[(u.grad)u], v=h A1, and a=2h^2 A2. At the entrance,

    f0 = v + N(A0) - nu Delta A0,
    f1 = a + N'(A0)v - nu Delta v,
    N'(u)v = P[(u.grad)v + (v.grad)u].

If the chosen background force has jets F0,F1 and the residual increment
has allowed jets g0,g1, the **joint constraints** are

    v = nu Delta A0 - N(A0) + F0 + g0,
    a = nu Delta v - N'(A0)v + F1 + g1.

These are actual full-equation compatibility requirements, not dimensional
estimates. Changing v or a also changes the preceding stage's required
terminal derivatives. An honest repair must propagate those changes upstream.
Higher orders impose further compatibility constraints; adding Taylor
coefficients alone would still not establish interval estimates or convergence.

## Exact diagnoses produced

The witness is the imaginary y-component Fourier coefficient at k=(1,0,1).
The diagnostic requests magnitude <=1/16 for each tested residual derivative.
**This chosen threshold is not a Clay admissibility requirement.**

| Proposal | Force-reference-relative entrance jets, orders 0..4 | Join | Diagnosis |
|---|---|---|---|
| Original interior blend | (1/2,0,0,0,0)i | closed | Entrance budget cannot be fixed by changing the blend |
| Zero interior blend | (1/2,0,0,0,0)i | closed | Same immutable entrance witness |
| Reversed interior blend | (1/2,0,0,0,0)i | closed | Same immutable entrance witness |
| Solve the full zeroth force-jet equation | (0,-2,1,0,0)i | broken | The new velocity derivative conflicts with the predecessor |
| Propagate that new derivative upstream | (0,-2,1,0,0)i | closed | The first force derivative now needs a compatible acceleration |
| Keep the original path, subtract its stationary background force | (0,0,0,0,0)i | closed | Endpoint witness passes, but an interior witness fails |

The repair generator obtains the new velocity derivative from the actual
full force operator. It verifies zero residual in **all** generated modes
before calling the zeroth force jet repaired. It then constructs a changed
upstream polynomial germ and checks the resulting smooth join. That changes
upstream data; it is not a repair that preserves a prescribed initial state.

For the last proposal, the reference is the actual smooth force maintaining
the constant base trajectory. This is a legitimate smooth background on the
finite interval; a later temporal cutoff can make it globally admissible.
Changing the reference is an accounting choice, not a reduction of the
physical force. Smooth forcing need not vanish at a stage entrance.

At the exact midpoint, theta(1/2)=1/2 and theta'(1/2)=2. The selected Fourier
coefficient of the **actual force increment** is -31i/16. Its magnitude is
31 times the requested 1/16 diagnostic budget. Since a normalized Fourier
coefficient is bounded by the spatial supremum norm, this is a genuine lower
bound obstruction to that budget, not a loose upper-bound artifact. The
endpoint success therefore cannot be promoted to a whole-interval estimate.

## What the application tells the next search to change

1. Specify the smooth background force explicitly; diagnose increments
   relative to it instead of silently requiring the entire force to vanish.
2. Solve inherited endpoint derivatives and neighboring stages jointly.
   Interior flat-switch parameters cannot change the endpoint constraints.
3. After each repair, recompute the complete force throughout the interval.
   Track both exact lower-bound witnesses and sufficient upper bounds.
4. Keep the infinite-stage uniform estimates as separate unresolved
   obligations. None of these finite diagnoses proves their existence.

These conclusions identify where the current grammar needs mathematical
input. They do not disclose a blowup mechanism or imply that a larger
coefficient search will necessarily find one.

## Dogfooding and validation

`actual_force_scaled` was added to `forced_fourier.rad` so exact rational
trajectory points can be evaluated without confusing amplitude rescaling
with the quadratic nonlinear term. Input Fourier coefficients are
(re+i im)/(2*scale), with checked integer scale 1..64. The original API
remains the scale-one wrapper. This is a research-library extension, not a
trusted-kernel change.

Validation passed:

- Twelve full-force operator cases, 386 Fourier outputs, including rational
  scales 2,8,64, compared with independent exact complex algebra.
- Six causal diagnoses independently checked, including the entrance jets
  and midpoint increment -31i/16.
- Forged whole-interval claims rejected; one/four-worker output identical;
  recorded-world replay verified.
- The four-stage, 76-polynomial trajectory regression still passes.

Run the diagnostic verifier with SymPy available:

```powershell
$env:PYTHONPATH='D:/Temp/rad-fluid-research/python'
python -X utf8 projects/dogfood/navier-stokes/verify_construction_why.py
```

The full structured diagnostic includes the witness numerators,
denominator, derivative order, reference force, join status, and suggested
next action. It is printed separately because WHY abbreviates long values.
The causal trace is

    ConstructionDiagnosis <- AssembleDiagnosis <- DiagnosisChecked <- SubmitDiagnosis.
