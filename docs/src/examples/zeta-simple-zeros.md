# Zeta simple-zero certificate

`projects/dogfood/zeta-simple-zeros/` is an end-to-end RAD verifier for a
finite six-gap certificate used to bound the proportion of simple zeros of the
Riemann zeta function. The refined certificate proves

```text
F6(g1,...,g6) >= 191/50000 = 0.00382
simple-zero proportion >= 67.30213627288%.
```

This is a finite-certificate result, not a sampled optimizer estimate. The
default run checks 4,096 initial boxes, 789,908 search nodes, 392,906 splits,
and every pressure, interval, and convex-tangent leaf. Its minimum rigorous
leaf lower is `382000131862 / 10^14`.

## What the workload exercises

The verifier deliberately crosses several RAD boundaries:

- `fork_with` and `simulate_many` execute independent exact proof lanes;
- a content-addressed native-extension handoff avoids repeatedly marshalling
  the 9.7 MB certificate artifact;
- Causal Laws and a Candidate Constraint merge lanes and reject incomplete or
  altered evidence;
- forward and reversed proposal orders must settle identically;
- a monolithic replay must match the sharded result;
- world digests, serialization round trips, `why()`, recording, and replay
  audit determinism and provenance.

The RAD proof stays in `zeta-simple-zeros/`. Its measured cover-tree hot path
is implemented by the project-owned `native-math-kernels` extension, outside
the generic VM.

## Trust boundary

The authoritative verification run uses RAD and the Rust extension. It replays
the complete integer tree and checks every branch, split, counter, and leaf;
Python is not the verification driver. Regenerating the transcendental lookup
artifact still requires `python-flint`/Arb for directed-rounding enclosures of
the kernel, derivatives, and tangents.

## Build and verify

From the repository root on Windows:

```powershell
& projects/dogfood/native-math-kernels/build.ps1 -Profile release

target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/verify.rad `
  --experimental-laws --deny-warnings -- `
  projects/dogfood/zeta-simple-zeros/certificate-382.json `
  16 `
  projects/dogfood/zeta-simple-zeros/out/verification-382.json
```

The [project README](../../../projects/dogfood/zeta-simple-zeros/README.md)
documents certificate regeneration, route-discovery experiments, recording,
and replay.
