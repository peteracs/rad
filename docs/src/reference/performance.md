# Performance

Measured numbers for the world-state machinery — fork, merge, wire codec,
persistence, events, causality. Reproduce with:

```bash
cargo test -p rad-vm --release bench_everything -- --ignored --nocapture
```

**External baselines** (vs Lua, QuickJS, Node, Yjs, Automerge — including
the legs rad loses) live in [External Baselines](external-baselines.md), regenerated
by `py benches/baselines/run_baselines.py`. Summary: rad's interpreter
is 6–12x slower than Lua on scalar code; naive string concat is ~6x
behind Lua (was 10x — the concat path now builds one exact-capacity
buffer, and f-strings/string `+` chains fuse into a single n-ary
`ConcatN`), while the O(n) accumulation idiom `join(map(range(n), fn))`
beats Lua's naive loop ~9x. On the same 10k-edit peer-sync script rad
ships a 2.2x smaller delta than Yjs (27x smaller than Automerge) and
ingests it 6–40x faster. The claims on this page are about world-state
operations, and the baselines file is what keeps that distinction honest.

**String accumulation guidance:** `s = s + x` in a loop is O(n²) — every
language in the baseline suite pays this on its naive loop, rad included.
For O(n) building, produce parts with one `map()` and `join` them. Note:
the `parts << x` loop is *not* the builder idiom today — value-semantics
lists deep-copy per push (a measured 8.5 s vs the naive loop's 2.6 s at
60k parts; tracked as an open gap).

Workload: 10,000 entities, 2 components each (one `int`-heavy, one mixed
`float`/`str`), measured in release mode on commodity hardware. Numbers are
wall-clock medians of warm runs; treat them as orders of magnitude, not
contractual limits.

## The headline numbers

| operation @10k entities | cost | scaling |
|---|---|---|
| `fork()` | **~8 µs** | O(archetypes) — CoW snapshot, independent of entity count |
| `commit()` | ~120 µs | O(archetypes) |
| `merge_forks()` (200 edits, 50 spawns, two-sided divergence) | **~2 ms** | O(divergence + rows of touched columns) — see below |
| `diff()` (shared lineage) | ~140 µs | O(touched columns) |
| `fork_to_bytes()` (state + provenance) | ~51 ms, 1.55 MB | O(world + ledger closure) |
| `fork_from_bytes()` | ~66 ms | O(world) |
| `fork_delta()` (200 touched of 10k) | **~2.4 ms, ~29 KB** | payload is O(divergence); time also pays touched-column rows |
| `fork_apply()` (same delta) | ~1.3 ms | O(divergence + touched-column rows) |
| `save_world()` | ~14 ms | O(world) |
| `emit` + handler dispatch | ~5 µs/event | O(handlers) |
| `why()` over a 20k-write ledger | ~60 µs | O(chain length) |
| ledger at retention cap (100k writes) | ~18 MB | bounded by the cap |

## Measured again at 1,000,000 entities

Same divergence (a few hundred touched rows), world 100x bigger — the
receipt that the structural claims aren't a constant-factor illusion:

| operation @1M entities | cost | vs @10k | verdict |
|---|---|---|---|
| `fork()` | **~11 µs** | ~1x | flat — speculation stays free at any size |
| `commit()` | **~7 µs** | ~1x | flat |
| `merge_forks()` (200 edits) | ~68 ms | ~30x | grows with *touched-column rows*, not entity count |
| `diff()` (shared lineage) | ~13 ms | ~90x | same — touched columns now hold 1M rows |
| `fork_delta()` (150 touched) | ~14 ms | ~6x | time tracks touched columns... |
| `fork_delta()` **payload** | **14 KB** | **smaller** | ...but the *wire* is pure divergence |
| `fork_apply()` | ~24 ms | ~18x | CoW restore + surgical edits |
| `save_world()` | ~2.3 s, 49 MB | ~170x | O(world), honestly |
| `fork_to_bytes()` | ~6.3 s, 61 MB | ~120x | O(world + capped ledger closure) |
| `fork_from_bytes()` | ~6.6 s | ~100x | O(world) |

Two honest findings from running this at scale:

- **"O(divergence)" was too generous** for merge/diff/delta *time*: CoW
  pointer-skips work at column granularity, so once a column is touched at
  all, its full row count is in play. The right claim is
  **O(divergence + rows of touched columns)** — still independent of how
  many *other* columns and archetypes exist, and the delta *payload* really
  is pure divergence (14 KB at 1M entities).
- The first attempt at this benchmark didn't finish at all: ledger eviction
  was `Vec::drain` from the front under a 100k cap — quadratic at 2M writes
  (~28 TB of memmove). It's a `VecDeque` now, with a regression test that
  proves amortized O(1) eviction.

## Merge cost is proportional to divergence, not world size

This is the structural property, and it is worth more than any constant
factor: `merge_forks(base, ours, theirs)` does **no per-entity work for
entities neither fork touched**.

Forks of one program run share structure (copy-on-write): an untouched
component column in `ours` is the *same allocation* as in `base`. The merge
discovers the touched-entity set by pointer comparison — an untouched column
costs one pointer check regardless of whether it holds ten rows or ten
million — and then runs the three-way field-level merge only over that set.
The merged world is built the same way: start from base (CoW), apply only
merged outcomes.

Consequence: the ~1 ms above is the cost of *the divergence* (350 touched
entities). Make the world 100x bigger with the same divergence and the merge
cost stays put. Worlds that crossed a process boundary (`fork_from_bytes`)
do not share allocations with the local base, so they fall back to a full
scan — correctness is identical, only the shortcut is lineage-gated.

The same CoW reasoning powers `fork()` (microseconds at any world size,
which is why speculation is free) and `diff()` (untouched columns are
skipped wholesale).

## The wire format

`fork_to_bytes()` writes `RADFORK2 <blake3> <body>` — canonical JSON written
directly into a string buffer, schema embedded once, component rows as
positional arrays, scalars spending bytes only where type fidelity demands
(`1` is an int, `1.0` is a float, `{"e":5}` is an entity reference). The
digest covers the raw body bytes, so verification on ingest needs no parse.
`save_world()` uses the same writer (`RADWORLD2` prefix); v1 tagged saves
still load.

For perspective, the v1 codec (tagged `serde_json` tree) measured 246 ms /
1.45 MB to encode and 184 ms to decode the same world — **without** carrying
any history. The v2 state sections alone measure ~16 ms / ~515 KB (15x
faster, 3x smaller); the payload above is larger because it now also carries
the **provenance closure** (the last write per live value plus its emit
chain — what makes cross-machine `why()` answer). History is most of the
bytes at this entity count: every entity costs one write record per
component.

## Delta sync pays double

The full payload is the price of the *first* transfer. After that,
`fork_delta(base, fork)` ships only the divergence — and because the
receiver already holds the base's history, the provenance closure shrinks
with the state: only records for touched values travel. At 10k entities
with 200 touched:

| | full (`fork_to_bytes`) | delta (`fork_delta`) | ratio |
|---|---|---|---|
| payload @10k/200 | 1.55 MB | 29 KB | **54x smaller** |
| encode @10k/200 | ~51 ms | ~2.4 ms | **~21x faster** |
| apply/decode @10k/200 | ~66 ms | ~1.3 ms | **~50x faster** |
| payload @1M/150 | 61 MB | **14 KB** | **~4,400x smaller** |
| encode @1M/150 | ~6.3 s | ~14 ms | **~450x faster** |
| apply/decode @1M/150 | ~6.6 s | ~24 ms | **~275x faster** |

The delta receipt is measured at 10k *and* 1M: the bigger the world, the
more delta sync pays, because the payload tracks what changed while the
full codec tracks what exists.

Touched entities are found by CoW pointer comparison (the same machinery as
merge), so encoding cost tracks the divergence, not the world. And because
`fork_apply` rebuilds on top of the receiver's own base (CoW restore +
surgical edits), the reconstruction *shares lineage with the local world* —
the O(divergence) merge fast path below works on wire-delivered forks,
which the full codec cannot offer. In syncdesk, a client's offline session
pushes in hundreds of **bytes** (`DPUSH`), not megabytes.

## Bookkeeping stays bounded

The causality ledger retains the most recent 100k writes and 100k emits
(~18 MB), evicting the oldest in amortized O(1). Long-running processes pay
a fixed memory cost for `why()`; queries that reach into evicted history
say so honestly. Replay a recorded trace when you need provenance older
than the window.

## The 1-hour soak

The longevity claim, held under sustained load: one syncdesk server, three
clients looping PULL → diverge offline → `DPUSH` a delta, for one hour.

| receipt | value |
|---|---|
| cycles completed | 79,024 (3 clients, paced ~10/s each) |
| merges committed | **63,803 — zero conflicts, zero retries** |
| delta size on the wire | 399-601 bytes per push |
| server memory | 6.6 MB start → 51.5 MB peak |
| final state | consistent `LIST`, clean `SHUTDOWN`, world intact |

The memory curve is linear and *accounted for*: the app's audit log is
append-only by design (~380 KB by hour's end), every served PULL pins one
of 16 base snapshots of its era, and the GC floats at ≤2x live. Subtract
designed growth and the runtime itself is flat — the in-process leak lab
measures **0.0 bytes leaked per cycle past VM teardown** on every phase of
the push path, enforced by a regression test.

That flatness was earned, not assumed. The first soak attempt blew past
**2.9 GB in 50 seconds**. The post-mortem found three real bugs — resource
writes leaked the value they displaced (quadratic against a growing log),
several write paths deep-copied payloads twice and abandoned one, and the
GC's auto-trigger had never been wired to the interpreter at all. All three
fixes are in; the 60-second A/B re-run peaked at 12.3 MB — **248x less**.
