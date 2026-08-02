# The counterfactual finale (D7)

A RADSHEET build ships with a one-character formula bug: `SUM`'s range
loop reads `range(r0, r1)` instead of `range(r0, r1 + 1)` — **every total
silently drops the last row of its range**. Nothing crashes. Two people
spend a session entering Q1/Q2 numbers, and the books don't balance:

```text
    A         B         C         D
1   widgets   1200      1350
2   gadgets   880       940
3   gizmos    410       505
4   doodads   75        60        <- visible in the grid…
5   TOTALS    2490      2795      5285   <- …missing from every total
```

(True totals: 2565 / 2855 / 5420.)

The session was recorded (`--record incident.radr`, 17 io records — the
actual keystrokes, since edits arrive via `readline()`). Three commands,
all answered **from the tape**:

## 1. Reproduce the corruption, byte-for-byte

```text
$ rad replay incident.radr
…TOTALS 2490 2795 5285
Replay: 32 frame(s), 17 io record(s) consumed, 0 leftover
Replay verified: world digest matches the recorded run
```

## 2. The counterfactual: the same session, with the fix

`fixed.rad` differs by one character. `rad replay incident.radr --with
fixed.rad` replays the *recorded keystrokes* against the fixed build:

```text
…TOTALS 2565 2855 5420            <- the clean sheet
=== Retroactive replay: fixed.rad against the recorded session ===
Recorded io: 17 consumed, 0 repeated reads, 0 unused
The edit's blast radius (original vs edited final world):
  {Cell: 3}
  original digest: 966b71db…
  edited digest:   663aa015…
```

The diff is printed from the tapes themselves: the fix changes **exactly
three Cell rows** (B5, C5, D5 — the formula cells) and nothing else, with
both world digests as receipts. `0 unused` io means the fixed build
consumed the identical input stream — the counterfactual is honest, not a
re-enactment.

## 3. Forensics on the corrupt timeline

The tape is also a queryable witness (`rad replay --serve`):

```text
why(B5) @ frame 32:
Cell of B5 = { raw: "=SUM(B1:B4)", val: 2490.0 }   (set in frame 27)
  <- by `on CellSet` handler
  <- CellSet { raw: "=SUM(B1:B4)", by: "alice" } emitted in frame 26
```

Who typed the formula whose result was corrupted, at which frame, from a
file — months later if need be.

## Files

| file | what |
|---|---|
| `buggy.rad` | the shipped build (bug marked with a comment) |
| `fixed.rad` | the one-character fix |
| `edits.txt` | the editing session piped to the recording |
| `incident.radr` | the tape (committed: it IS the incident) |

Reproduce end-to-end:

```powershell
cmd /c ".\target\debug\rad.exe --record projects\dogfood\radsheet\incident\incident.radr projects\dogfood\radsheet\incident\buggy.rad < projects\dogfood\radsheet\incident\edits.txt"
.\target\debug\rad.exe replay projects\dogfood\radsheet\incident\incident.radr
.\target\debug\rad.exe replay projects\dogfood\radsheet\incident\incident.radr --with projects\dogfood\radsheet\incident\fixed.rad
```
