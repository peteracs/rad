# RADSHEET — a collaborative spreadsheet, as the D5 deliverable

The round-table's "wow" item: grid, formulas with dependencies via events,
per-cell `why()`, offline mode with a conflict picker, and a history
scrubber running off the recorded tape. RADTRACK proved the distributed
half on tickets; RADSHEET proves it on *derived state* — the hard part of
a spreadsheet is that most of what you see is computed from what was typed.

## The pieces

| file | what it is |
|---|---|
| `lib_sheet.rad` | grid, formula engine, evented recompute, rendering, wire helpers |
| `sheet.rad` | offline CLI client: set/grid/cell/why/pull/sync + conflict picker |
| `server.rad` | sync server with a *spreadsheet* merge policy (see below) |
| `smoke.rad` | engine test: SUM/AVG chains, cascades, cycles, #ERR, why() |
| `demo/run_sheet_demo.ps1` | the offline-conflict demo with convergence receipts |
| `demo/scrub_session.jsonl` | the history scrubber session (JSON-RPC over the tape) |

## Design decisions that did the work

- **The grid is pre-spawned; cell names are entity names.** Two offline
  clients editing `B1` produce a *field* conflict on `Cell.raw` —
  mechanically pickable — instead of a structural name claim. The grid is
  the namespace.
- **Source vs derived, enforced by merge policy.** `Cell.raw` is human
  truth: raw-vs-raw is a real conflict, shipped to the picker. `Cell.val`
  and `Cell.kind` are derived: the server auto-resolves them arbitrarily
  and then **reflows every cell from merged raw** before cutting the
  down-delta. One `derive(raw)` function is the only author of val/kind.
- **Dependencies are events, not a graph.** `CellSet` stores one cell and
  emits `Dirty`; the `Dirty` handler re-derives all cells (sorted order,
  deterministic) to fixpoint, 16-pass fuel so `=A1`/`=B1` cycles stabilize
  instead of spinning. Editing costs two frames: set, reflow. Because
  handlers are the only writers, `why()` on a formula cell is provenance,
  not a feature.
- **Formula grammar, honestly small**: `=SUM/AVG/MIN/MAX/COUNT(A1:B3)`,
  cell refs, numbers, `+ - * /` left-to-right. Parse failures and division
  by zero are `#ERR` values, never crashes.

## Receipts from the live demo (`run_sheet_demo.ps1`, real output)

Seeded budget: `B1=1200 rent, B2=450 food, B3==SUM(B1:B2)` → 1650.

- alice and bob pull the same 1705 B base, go offline.
- alice: `set B1 1300`, then `why B3` — **the PASS criterion, verbatim**:

  ```text
  Cell of B3 = { raw: "=SUM(B1:B2)", val: 1750.0, kind: "formula" }
    <- by `on Dirty` handler
    <- Dirty { depth: 0, by: "alice", ... }
    <- by `on CellSet` handler
    <- CellSet { cell: 12, raw: "1300", by: "alice", ... }
  ```

  The formula cell names the human who changed the *input*.
- alice adds `B4 ==B3*12` (annual: 21000), syncs clean; the server reflows
  `B3 → 1750` before answering. `converged: digests agree`.
- bob, from the *original* base: `set B1 1250` (collides) and `set B2 500`
  (clean). His sync surfaces exactly one human conflict —
  `cell B1: server 1300, yours 1250` — the picker keeps his; RESOLVE
  merges, the server reflows, and the grid shows `total: 1750` everywhere
  (1250 + 500). `Cell.val/kind` conflicts were auto-resolved as derived;
  `Sheet.passes` merged by max.
- `world_digest` agrees on **all three machines** after every sync — and
  bob's `swhy B3` answers from the *server* with cross-machine provenance:
  `Dirty { by: "bob" } … [via wire 592209ae, remote frame]`.

## What the dogfooding caught

The first demo run converged all three machines on a **wrong total**: bob
picked `raw="1250"` but the auto-resolved derived field kept `val=1300.0`,
and the reflow pass only recomputed *formula* cells — so number cells kept
stale values, B3 summed 1300+500=1800, and the digests agreed everywhere.
**Consistency is not correctness.** The fix made `derive(raw)` the single
normalization and the `Dirty` pass re-derive *every* cell from raw. That
bug class (derived state surviving a merge of its sources) is exactly what
D5 was designed to flush out.

## The history scrubber, off the tape

`run_sheet_demo.ps1 -Record` produces `sheet.radr` (8.7 KB, RADPACK'd).
It replays byte-verified — `59 io record(s) consumed, 0 leftover … world
digest matches` — and `rad replay --serve` turns it into a time-travel
session. `scrub_session.jsonl` scrubs `B3` across the timeline:

| frame | B3 | what happened |
|---|---|---|
| 2–10 | empty | seeding in progress |
| 12 | **1650** | seeded `=SUM(B1:B2)` |
| 14 | **1750** | alice's rent hike merged + reflowed |
| 16–18 | **1750** | bob's resolve kept 1250+500 |

…and `why` at frame 18 answers with bob's wire provenance, from the
referee's seat, months after the fact if need be. `goto_frame` returns the
world digest at any frame — every scrub position is itself verifiable.

## Run it

```powershell
# engine smoke (formulas, cascades, cycles, why)
./target/debug/rad.exe projects/dogfood/radsheet/smoke.rad

# the offline-conflict demo (add -Record for the tape)
powershell -File projects/dogfood/radsheet/demo/run_sheet_demo.ps1 -Record

# verify the tape, then scrub history through it
./target/debug/rad.exe replay projects/dogfood/radsheet/demo/sheet.radr
cmd /c ".\target\debug\rad.exe replay projects\dogfood\radsheet\demo\sheet.radr --serve < projects\dogfood\radsheet\demo\scrub_session.jsonl"

# poke at it interactively
./target/debug/rad.exe projects/dogfood/radsheet/server.rad -- mysheet.radw   # terminal 1
./target/debug/rad.exe projects/dogfood/radsheet/sheet.rad -- you ./mydir     # terminal 2
```
