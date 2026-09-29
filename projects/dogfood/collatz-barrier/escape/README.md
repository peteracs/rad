# Collatz coefficient escape

This RAD workload supports a written, unconditional bound on the spread of
the Collatz multiplicative coefficient. For the shortcut map, set
`P_j = 3^(odd steps before j) / 2^j` and `R_N = max(P_0..P_N)/min(P_0..P_N)`.
If the first `N+1` states are distinct and `N >= max(3*n,16)`, then

```text
R_N > N^(1/28) / 4.
```

The certified exponent is actually greater than `0.03585657`. Repeated-state
orbits have exponentially growing spread, so every positive integer orbit
eventually escapes each fixed coefficient band. This is a restriction on
possible behavior, not a proof of Collatz. The qualitative exclusion already
follows from Garcia--Tal's orbit-sparsity estimates; priority for the explicit
finite bound is unresolved. It does not improve the known asymptotic theory.
Read [PROOF.md](PROOF.md) for the complete argument and its relationship to
existing work; [evidence.json](evidence.json) records the executed campaign.

From the repository root:

```powershell
py -O projects/dogfood/collatz-barrier/escape/accept.py
```

For the RAD program alone:

```powershell
.\target\release\rad.exe projects/dogfood/collatz-barrier/escape/main.rad --strict-types --deny-warnings --experimental-laws
```

The workload uses RAD's advanced features for concrete verification tasks:

- 21 isolated worlds evaluate rational weights with certified logarithm bounds.
- Typed intents submit the resulting evidence to a resolver; a constraint
  requires the complete portfolio and an exponent strictly above `1/28`.
- Reversing proposal order must leave the selected certificate unchanged;
  `why` records the causal provenance of the selection.
- Eight additional worlds count admissible words in coefficient bands, using
  arbitrary-natural arithmetic; the largest modulus is `2^119`.
- Write-footprint checks, snapshot roundtrips, and live-world isolation check
  that proof jobs preserve their declared boundaries.
- Mutable byte buffers check the parity/residue bijection through depth 16,
  over 131070 residue cases and 1966082 literal transitions in total.
- Runs recorded with one and four workers must agree on every mathematical
  row, and each trace is replayed with the opposite worker count.

`exact.rad` adds ordinary-language arbitrary-natural Euclidean division,
ceiling division, and rational logarithm intervals to the arithmetic available
in the parent workload. There is no Collatz-specific VM primitive. All
coefficient counts, interval endpoints, and certificate margins are integers;
floating-point searches do not certify any reported conclusion.

`verify.py` independently uses Python integers and exact `Fraction` series.
It checks every selected margin and deadline, enumerates parity words in a
different representation, and checks finite height and residue-capacity
inequalities on literal trajectories. The all-size proof does not follow from
these finite checks. Explicit exceptions preserve acceptance checks under `-O`.
Mutation tests reject corrupted counts and rounded deadlines and guard the
distinctions between spread and maximum, and prefix and endpoint conditions.

Ignored `out/` holds raw output, stderr, and replay traces. The receipt hashes
their actual bytes, the executed binary, local source and proof files, and the
two imported arithmetic modules. It is an execution receipt, not a formal
proof object or a historical-priority certificate.
