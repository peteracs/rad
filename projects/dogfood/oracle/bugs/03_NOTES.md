# BUG 03 — memory corruption from `peek()` of `simulate_par()` result forks

Exit codes seen: `-1073741819` = `0xC0000005` **STATUS_ACCESS_VIOLATION**, and
`-1073740791` = `0xC0000409` **STATUS_STACK_BUFFER_OVERRUN**. Both are memory
corruption. No Rad-level error, no panic message, no partial output — the
process just dies.

> **Correction.** An earlier version of this file (and mailbox seq 124) claimed
> the trigger was peeking a component containing a **`str` field**, and offered
> deleting that field as a one-token fix. **That conclusion was wrong** and is
> retracted in mailbox seq 143. A `str` field is a strong *probability
> modulator* in some configurations, but it is not necessary: `main.rad` with no
> `str` field anywhere still crashes 11/30. The sections below are the corrected
> account.

## Repro

```
$env:RAYON_NUM_THREADS=4
target\debug\rad.exe projects\dogfood\oracle\bugs\03_simulate_par_str_peek_crash.rad
```

~20% of runs die. Expected: `acc <n>` then `SURVIVED`, every time. (The filename
overstates the `str` angle; read it as "peek off simulate_par results".)

`projects/dogfood/oracle/main.rad` at the commit that scored candidates with
three distinct peeks was an equally good repro at 11/30, with no strings
involved at all.

## How it was found

`main.rad` (beam-search planner) produced **different stdout at different
`RAYON_NUM_THREADS`**. The differing runs were truncated, not reordered — the
process was dying. Crash rate on that version of `main.rad`:

| RAYON_NUM_THREADS | runs | access violations |
|---|---|---|
| 1  | 12 | 0 |
| 4  | 12 | 1 |
| 8  | 30 | 3 |
| 32 | 30 | 8 |

An instrumented copy (`flush_stdout()` after every step) localized the fault:
the last marker printed is always `post-fork`, and `post-simpar` is never
reached. **The fault is inside `simulate_par()`.** It always occurred at ply 2 —
the first ply whose input fork descends from earlier `simulate_par` results.

## What is established

Each of these is required; removing any one gives 0 crashes.

1. **`simulate_par()`.** Substituting plain `simulate()` in the same shape: 0/30.
2. **`commit()` of a fork inside the loop.** Removing it: 0/30.
3. **`peek()`/`peek_resource()` of result forks.** Removing all peeks and scoring
   with plain arithmetic: 0/30. **This is the only reliable mitigation found.**

Two things are *not* required:

- **Multiple threads.** The compact repro crashes at `RAYON_NUM_THREADS=1`
  (4/20), `=4` (3/20), `=32` (1/20). This is **not** a data race. Thread count
  perturbs timing, not existence. (`main.rad` happened to be clean at 1 thread,
  which sent the first hour of investigation down a race-hunting dead end.)
- **Reading a `str` field.** A variant that peeks a str-bearing component and
  touches only its int field still crashes 6/30.

## Crash rate vs. what the scoring function peeks

All else identical, 30 runs at `RAYON_NUM_THREADS=32`.

| variant | peeks per result fork | crashes |
|---|---|---|
| M1 | none (arithmetic score) | 0/30 |
| M4 | `peek_resource(Ledger)` | 0/30 |
| M5 | `peek(Stock)` | 0/30 |
| M6 | `peek_resource` + `peek(Stock)` | 0/30 |
| M8 | `peek(Stock)` + `peek(Sector)`, no str | 0/**60** |
| M7 | `peek(Stock)` + `peek(Sector)`, Sector has `name: str` | **17/30** |
| main | `peek_resource` + `peek(Stock)` + `peek(Sector)`, no str | **11/30** |
| main | same, Sector has `name: str` | **8/30** |

And on the compact side, built up from a clean program:

| test | change | crashes |
|---|---|---|
| repro03 | three int-only reads per fork | 0/**60** |
| repro03b | identical + one `str` field on a peeked component | crashes on run 1 |
| T1 | `simulate()` instead of `simulate_par()` | 0/30 |
| T2 | no `commit()` in the loop | 0/30 |
| T3 | never reads the str field, only the int field | 6/30 |

The clean rows were re-run to 60 runs to rule out luck.

**There is no tidy predicate here.** Adding a third distinct read re-arms it with
no strings involved (main, 11/30), yet `repro03` does three distinct reads with
no strings and is clean at 0/60. Peek count is not the rule; the `str` field is
not the rule. What separates `main.rad` from `repro03` is incidental: 8 systems
vs 5, three components per entity vs two, more total world state. Probability
that tracks allocation shape and component layout rather than any semantic
property of the program is what memory corruption looks like from outside the
VM. I would not keep hunting for a Rad-level rule.

## What this cost the app

`main.rad` scores candidate futures off result forks — that is the entire point
of a beam-search planner, and `peek()` is the builtin the docs prescribe for it.
The working version had to be de-tuned to the M6 shape (two reads per fork:
`Ledger` and `Stock`) to stay alive. The `workers` term was dropped from the
objective and is now only captured indirectly through harvest yield. That is a
language bug dictating an application's objective function.

## Why it matters beyond the crash

`docs/src/reference/builtins.md` states `simulate_par` results are
"bit-identical for the same inputs at any thread count". Observably, identical
inputs produced **different stdout** at different thread counts, because some
runs died partway.

Once the scoring function is kept to two reads, that guarantee does hold: both
`main.rad` and `determinism.rad` now produce byte-identical output at
`RAYON_NUM_THREADS` of 1, 2, 3, 4, 8, 16 and 32, and across repeated fresh
processes. The determinism machinery is sound; the memory safety underneath it
is not.
