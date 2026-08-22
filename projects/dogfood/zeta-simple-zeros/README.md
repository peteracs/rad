# Zeta simple-zero certificate in RAD

This dogfood independently consumes the finite certificate behind
[`ainta/zeta-simple-zeros`](https://github.com/ainta/zeta-simple-zeros), pinned
to commit `040c5e899e658aed7b56a2a87f501798fe10761d`.

The original `67.30085%` claim verifies. A refined certificate also proves

```text
F6(g1,...,g6) >= 191/50000 = 0.00382

simple-zero proportion >= 0.6730213627288
                       = 67.30213627288%.
```

This improves the advertised lower bound by about `0.00128347` percentage
points. It is a stronger finite-certificate result, not a sampled optimizer
estimate.

## The 70% boundary (not yet closed)

The fourth-trace route has the right long-shift exponent. With `Y=X^2`,

```text
H = X^2/T = Y^((2*lambda-1)/(2*lambda)).
```

For every fixed `lambda>3/4`, this is in the classical long-shift range
`H>Y^(1/3+epsilon)`. This makes a doubled Heath--Brown/Type-I/II attack
plausible, but the theorem stated by Matomaki--Radziwill--Tao does not directly
include the balanced truncated coefficient
`(Lambda 1_[1,X])*(Lambda 1_[1,X])`. A complete minor- and major-arc extension
for that coefficient is still required. A quantitative derivation of the
fourth trace, including end effects and weighted shift tails, is also still
required.

The resulting normalized moments at `lambda -> 1-` are

```text
1, 1, 4/3, 2, 13/4.
```

If those analytic obligations are closed, the exact degree-two Christoffel
value is `5/36`, and the unconditional signature conversion would give

```text
simple-zero proportion >= 1 - 2*(5/36)
                       = 13/18
                       = 72.222222...%.
```

The proof audit is in `ANALYTIC_PROOF.md`; `analytic_theorem_prover.rad` checks
the exponent transfer, exact moment algebra, fork isolation, wire round trips,
order-independent settlement, and causal ancestry. It deliberately reports
three open analytic obligations. The authoritative scheduler continues to
promote the rigorous `67.30213627288%` finite-certificate bound.

Run it with:

```powershell
target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/analytic_theorem_prover.rad `
  --experimental-laws --deny-warnings

target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/attack_scheduler.rad `
  --experimental-laws --deny-warnings
```

## Route-discovery history

Before the long-shift reduction was isolated, the scheduler promoted a
positive fourth-trace/Fejer factor-slicing route:

```text
promoted route          Fejer factor-sliced fourth trace / Type-II convolution
sufficient fourth cap  tr(G^4)/N <= 59/18 = 3.277777...
predicted fourth moment 13/4 = 3.25
relative slack          1/117 = 0.8547008...%
then-conditional conclusion simple proportion >= 70%
then-predicted conclusion             13/18 = 72.2222...%
```

`frontier_barrage.rad` runs the original ten-way architecture portfolio. The
refined `fourth_moment_transfer.rad` now runs 24 native arithmetic forks over
the truncated von Mangoldt convolution. Prefix sums evaluate the pooled
correlation over every `1 <= h <= H` in `O(Y)` rather than sampling the hot
path. At `Y=2,000,000`, `H=floor(sqrt(Y))`, factor cap 256, it reports:

```text
Y = 2,000,000, H = 1,414
all-shift pooled ratio              0.999864605
positive Fejer ratio                0.999156680
balanced-factor pooled ratio        0.998843486
small/balanced cross pooled ratio   1.000848162
```

All 24 worlds are inside the `1/117` numerical gate. Complete residue blocks
modulo 30 remove a sampling artefact caused by the large parity split; at the
largest cell their worst error is `0.5811023%`, still inside the gate. Fixed
small-factor cutoffs remain invalid: the tail L2 mass falls below the gate only
at cutoff 1408, essentially `sqrt(Y)`. In contrast, forks at the actual trace
scale `H=Y^((2*lambda-1)/(2*lambda))`, splitting factors at `H`, show tail L2
fractions falling from `14.40%` at `lambda=0.9` to `0.0003112%` at
`lambda=0.999`.

`fejer-type-ii-attack.rad` forks ten exact budget envelopes. Positivity turns
the fourth trace into a seminorm, so factor slices combine without separate
mixed-correlation estimates. At `lambda=0.999`, a balanced-tail sieve constant
of 9 gives a conservative ratio `1.004008016`, reserving roughly half of the
full `1/117` slack for localisation and moment-continuity errors. This
decomposition is now superseded: treating the full truncated convolution in
the long-shift range removes both the fixed-factor transfer and the separate
balanced-tip sieve obligation.

Two other rapid barrages were demoted. A 25-way two-Fourier-mode window search
improves only to `67.3195648%` (asymptotic ceiling about `67.31993%`). The
Bui--Heath-Brown mollifier reaches `70.3110320%` at `theta=0.499` before an
unconditional signature conversion, but tolerates only `0.4423658%` loss.

Run the historical RAD barrages with:

```powershell
target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/frontier_barrage.rad `
  --experimental-laws --deny-warnings

target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/fourth_moment_transfer.rad `
  --experimental-laws --deny-warnings

target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/fejer-type-ii-attack.rad `
  --experimental-laws --deny-warnings

target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/attack_scheduler.rad `
  --experimental-laws --deny-warnings
```

`strategy_lab.rad` was the earlier architecture search. It did not by itself
claim that 70% had been proved. Its exact fixed-point gate found:

```text
current F6 route                    67.3021% (best m=267)
optimistic m=267 cap/no-tax ceiling 67.5028%
least replacement baseline H       0.699378599
least short-block local C           0.056847381 (m=23, 14.881x current C)
```

The apparent `m=23` attack is then rejected rigorously. For unit-spaced
points, the normalized kernel has the exact integer-value identity

```text
|k(r)| = 1 / (2*pi^2*r^2 - 1) < 1 / (17*r^2).
```

Since `Delta(G) <= E`, this gives `E < 4m/289`. Elementary alternating
Taylor bounds also give `H0 < 17/25`, whereas a 70% deduction needs a
zero-span local term greater than `m/35`. Because `4/289 < 1/35` and the
unit-gap span slope is also smaller than the required 70% slope, **no
fixed-size shifted-block inequality of the form**

```text
Delta(G) + q*span >= A,  m >= 2, q >= 0
```

can reach 70% from this baseline. This rules out the whole affine-span block
family, not only the current `m=267` choice.

The leading surviving attack is wider bandwidth. Forking the scale-free
Montgomery--Taylor variational formula

```text
H(lambda) = 2 - (cot(lambda/sqrt(2)) + lambda/sqrt(2)) / sqrt(2)
```

on a `0.00025` grid finds its first numerical 70% crossing at
`lambda=1.04275`, with `H(lambda) >= 0.700057882` in the widened binary64
scout. Thus the quantitative target is a roughly **4.275% extension beyond
the currently proved bandwidth-one prime-side estimate**. This is a research
target, not yet a rigorous zeta bound.

The attack order recorded in the RAD receipt is:

1. prove the new prime-side terms for `1 < lambda <= 1.043`;
2. if that barrier is rigid, derive a higher-moment stability inequality
   (for example using `tr(M^3)` or a certified spectral polynomial) that
   defeats the unit-lattice obstruction;
3. test a multi-window direct-sum certificate whose overlap kernels do not
   share the same integer-spacing near-zeros;
4. consider nonlinear density-coupled or multi-scale settlements; an affine
   span multiplier has already been ruled out.

Run the attack laboratory with:

```powershell
target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/strategy_lab.rad `
  --experimental-laws --deny-warnings
```

The laboratory forks 294 block gates, 141 kernel witnesses, and 321 bandwidth
candidates. It settles each candidate family in forward and reversed order,
checks complete partitions with Candidate Constraints, round-trips every fork
through the wire format, audits speculative isolation, and stores `why()`
ancestry in `out/seventy-percent-gate.json`.

## What RAD verifies

The default `certificate-382.json` run visits exactly:

| Counter | Value |
|---|---:|
| Initial six-gap boxes | 4,096 |
| Search nodes | 789,908 |
| Splits | 392,906 |
| Pressure leaves | 6,268 |
| Interval leaves | 269,922 |
| Convex-tangent leaves | 120,812 |
| Maximum depth | 43 |

The minimum rigorous leaf lower is
`382000131862 / 10^14 = 0.00382000131862`, attained by a tangent leaf.

The final deduction no longer imports an Arb decimal for
`H0 = 3/2 - x cot(x)`, `x^2=1/2`. RAD sums positive rational terms of the
cotangent/Bernoulli expansion, obtaining

```text
H0 >= 6725007035877 / 10^13.
```

For a certified local lower `C`, RAD chooses the largest block size satisfying
`C(m-6) < 1`. The refined certificate selects `m=267` and checks

```text
B = (m H0 - (m-1)/500) / (m - (m-6) C)
  >= 0.6730213627288
```

with overflow-safe signed-integer arithmetic. The final comparison has a
positive integer margin of 170 after conservative digitwise ceilings.

## Advanced RAD features exercised

This is an end-to-end RAD workload rather than a wrapper around a Python run:

- `fork_with` creates independent cover-lane worlds;
- `simulate_many` runs exact lane kernels concurrently;
- a content-addressed native-extension handoff validates the 9.7 MB artifact
  once, then sends only its 64-byte digest through worker worlds;
- `assert_only_changed`, `world_digest`, `fork_to_bytes`, and
  `fork_from_bytes` audit isolation and wire stability;
- typed `intent`/`law`/`resolver` declarations merge all lane evidence;
- a Candidate Constraint rejects missing or duplicated lanes, altered search
  counters, weak leaves, and incomplete prune partitions;
- forward and reversed proposal orders must settle to the same component;
- an independent monolithic replay must match the sharded result exactly;
- events retain settlement ancestry for `why()` and `why_resource()`;
- `--record` and `rad replay` reproduce the same authoritative world digest.

The 16-lane proof plus monolithic differential replay takes about 0.45 seconds
on the development host. The earlier pure-RAD list-heavy prototype took many
minutes; moving the range-minimum/tree loop across RAD's generic native ABI and
eliminating repeated certificate marshalling are the relevant hot-path wins.

## Arithmetic and trust boundary

The authoritative verification command below runs only RAD and the
project-owned Rust extension. Python is not an optimization or verification
driver.

Arb is still needed to *regenerate* the transcendental artifact. RAD currently
has no arbitrary-precision directed-rounding implementation of `pi`, `sqrt`,
`sin`, or interval derivatives. Consequently the trust base still includes
the upstream formulas and `python-flint`/Arb for:

- normalized-kernel cell lower bounds;
- second-derivative enclosures and convexity checks;
- tangent value/gradient enclosures.

The Python producer's branch decisions and counters are not trusted: RAD
replays the complete integer tree and checks every leaf and split. The pinned
table hashes are:

```text
kernel:  a9992300d2bf71665aa2b6bd2727e798624cd297103bb200c7f0ca2baea55a2c
hessian: 7913c5511a572c32dd573cd53123d8cf3ddf73d3ec63b1aa823faae2ae83570a
```

## Build and verify

From the RAD repository root:

```powershell
& projects/dogfood/native-math-kernels/build.ps1 -Profile release

target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/verify.rad `
  --experimental-laws --deny-warnings -- `
  projects/dogfood/zeta-simple-zeros/certificate-382.json `
  16 `
  projects/dogfood/zeta-simple-zeros/out/verification-382.json
```

Record and replay the same proof:

```powershell
target/debug/rad.exe `
  projects/dogfood/zeta-simple-zeros/verify.rad `
  --experimental-laws --deny-warnings `
  --record projects/dogfood/zeta-simple-zeros/out/verification-382.radr -- `
  projects/dogfood/zeta-simple-zeros/certificate-382.json 16 `
  projects/dogfood/zeta-simple-zeros/out/verification-recorded.json

target/debug/rad.exe replay `
  projects/dogfood/zeta-simple-zeros/out/verification-382.radr
```

The checked replay consumes all six recorded I/O operations and reports an
identical world digest.

## Optional Arb regeneration

Regeneration is a certificate-production step, not the authoritative proof
run. With the upstream repository checked out at the pinned commit and its
`python-flint` environment active:

```powershell
python generate_certificate.py `
  D:\path\to\zeta-simple-zeros `
  certificate-382.json `
  --target-numerator 191 `
  --target-denominator 50000 `
  --progress-every 100000
```

`certificate.json` retains the original `19/5000` cover for differential and
historical verification. `inspect_certificate.rad` performs a fast RAD
single-lane inspection of either artifact.
