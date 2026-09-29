# Collatz envelope ladder

This RAD workload certifies the complete family of sharp linear bounds on
first-contraction offsets. It covers every slope from
`s* = 756421/3145728 ≈ 0.24045976` upward. The limit slope below which no
linear bound exists is `1/(6 ln 2) ≈ 0.24044917`.

```text
For every s >= s* and every first-contraction word with q odd steps:
    C/3^q <= s q + max_{v in V} (C_v/3^v - s v),     equality attained.

V = 0, 1, 3, 15, 27, 68, 80, 121, 133, 174, 839, 1504, ..., 7489
gaps:  1, 2, 12, 12, 41, 12, 41, 12, 41, 665 (x11)
```

The gaps between optimal vertices are continued-fraction denominators of
`log_2 3`. The earlier bound `q/4 + 11/108` in [RESULTS.md](../RESULTS.md) is
the rung at slope 1/4.

A classical Denjoy–Koksma argument shows the method reaches every slope above
the limit. See [PROOF.md](PROOF.md) for the theorems, the complete proof, the
exploratory data out to `q = 10^9`, and scope. This is not a proof of Collatz.

From the repository root:

```powershell
py -O projects/dogfood/collatz-barrier/ladder/accept.py
```

To run the RAD program alone:

```powershell
.\target\release\rad.exe projects/dogfood/collatz-barrier/ladder/main.rad --strict-types --deny-warnings --experimental-laws
```

## What RAD does here

- **Isolated block lanes.** 16 isolated worlds split the 121,703,401
  candidate phases of the 15601-block certificate. Each lane uses integer
  interval arithmetic with outward rounding. `assert_only_changed` checks each
  lane's write footprint, and a snapshot round trip checks its evidence.
- **Causal assembly.** Lanes submit typed `LaneResult` intents. One resolver
  proves the lanes are an exact cover of `1..15601` and combines them. A
  constraint refuses any incomplete or non-contracting certificate.
  Settlements with forward and reversed proposal order must agree, and
  `why(forward, BlockCertificate)` prints the provenance of every lane.
- **Convergent probe.** Five more worlds test blocks 12, 41, 53, 306 and 665.
  None contracts at `s*`, which is why the certificate needs 15601.
- **Ladder certificate.** The program selects hull vertices, then re-proves
  every chord, concavity, tail-ray and bootstrap inequality from the
  rigorous enclosures. RAD traps integer overflow, so a wrapped product
  cannot enter a certificate.
- **Determinism and replay.** Runs with one and four workers must print
  identical rows. Each recorded trace replays with the other worker count.

## Independent checks

The verifier, `verify.py`, trusts no RAD number:

- It recomputes the block maxima from exact big-integer floors and ceilings,
  not from RAD's recurrence.
- It re-proves chords, concavity, tail and bootstrap with the exact rationals
  `3C_q/3^q`, using integer cross-multiplication with no rounding.
- It requires every RAD upper bound to be at least its own rigorous lower
  bound.
- It requires RAD's enclosures to contain the exact values.

`test_verify.py` checks that tampered enclosures, missing lanes, extra or
missing vertices, an early bootstrap and a wrong slope are all rejected.
