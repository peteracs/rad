# External baselines (D6) — published, including the losses

One reproducible script, two legs: an interpreter micro-suite against
Lua and QuickJS (and Node/V8, which we expect to lose to), and the same
10k-edit collaboration script against Yjs and Automerge. Every
implementation prints its computed result and the harness **refuses to
report a row whose results disagree across runtimes** — the comparisons
stay apples-to-apples by construction.

```powershell
# deps: py -m pip install lupa quickjs ; npm install (in collab/)
cargo build --release -p rad-vm
py benches/baselines/run_baselines.py
```

Numbers below: Intel i7-11700K class (Family 6 Model 167), Windows 10,
median of 5 runs, times in ms, lower is better. Regenerate any time —
`RESULTS.md` and `results.json` are overwritten by the script.

## Interpreter micro-suite — we lose most of these, on purpose

| bench | rad | Lua 5.5 (lupa) | QuickJS | Node (V8 JIT) |
|---|---|---|---|---|
| fib(30) recursive | 282 | **46** | 87 | 8 |
| loop_sum (10M iters) | 1139 | **105** | 339 | 14 |
| str_build (60k concats) | 3011 | 497 | **363** | 10 |
| sort (200k ints, stdlib) | **20** | 66 | 113 | 62 |

(Regenerate with the script; the table in `RESULTS.md` is the live one.)

Honest reading:

- **rad's bytecode interpreter is 6–12x slower than Lua** on scalar
  call/loop code and ~3x slower than QuickJS. Nobody should pick rad to
  compute Fibonacci. The "50x–200x" claims this repo makes are about
  *world-state operations* (below, and `docs/.../performance.md`),
  never about scalar interpretation — this table is what keeps that
  distinction honest.
- **str_build was 10.2x behind Lua when first published** (7193 vs 708
  same-run). Tier-1 #2 took it to **6.1x** (3011 vs 497 same-run): the
  concat path used to copy the accumulator **three times** per `+`
  (Arc→String, an exact-size push_str realloc, String→Arc) and
  f-strings/`+`-chains compiled to pairwise Adds, each re-copying the
  growing prefix. Now: one exact-capacity buffer per concat, and
  f-strings + provably-string `+` chains fuse into a single n-ary
  `ConcatN`. The remaining gap is the still-quadratic accumulator plus
  one Arc copy per concat.
- **The O(n) accumulation idiom beats every naive loop in the table**:
  `join(map(range(0, 60000), fn(i) { ... }), "")` measures **57 ms** —
  ~9x faster than Lua's naive loop (`str_build_map.rad`). And a finding
  the idiom benchmark caught: the `parts << x; join(parts)` "builder"
  form is currently **slower than the naive loop** (8.5 s) because
  list push deep-copies the whole list per `<<` — value-semantics lists
  pay O(n) per push. That's logged as its own Tier-1-class gap
  (`str_build_join.rad` is the receipt).
- **sort wins** because `sort()` drops into the native Rust sort — the
  pattern that matters: rad's model is thin scripting over fat native
  primitives, and wherever a workload hits a primitive, the interpreter
  tax disappears.
- Node is a JIT; losing micro-loops to V8 by 20–100x is the expected
  baseline for *every* interpreter in this table (Lua loses fib to V8
  5.5x too).

## The same 10k-edit script (2000 cells), peer sync — we win this one

10,000 edits from an identical MINSTD stream over 2,000 cells, one
edit = one keystroke in every system; then ship the divergence to a peer
holding the base and ingest it. Checksums cross-verified.

| system | apply 10k edits | delta bytes | peer ingest |
|---|---|---|---|
| **rad** (`fork_delta` → `fork_apply`) | **38 ms** | **45,251** | **7 ms** |
| Yjs | 106 ms | 100,864 | 45 ms |
| Automerge | 11,969 ms | 1,214,749 | 287 ms |

rad's three-way `merge_forks(base, ours, theirs)` afterwards: **+12 ms**.

Honest reading:

- **Different guarantees, stated plainly.** Yjs and Automerge are CRDTs:
  their payloads carry op history and merge pairwise without a common
  base. rad ships *state divergence* and merges three-way against the
  pulled base (RADTRACK/RADSHEET's PULL→SYNC model). When you have a
  base — every client/server and hub-replica topology — rad's wire is
  2.2x smaller than Yjs and 27x smaller than Automerge for the same
  edits, and the end-to-end sync (ingest + merge) is ~2–15x faster.
  When you genuinely need baseless pairwise convergence, use a CRDT.
- Automerge's 12 s apply / 1.2 MB delta is the cost of its model (every
  edit is a commit with history, in line with its own documentation) —
  not a misconfiguration. Batching edits into fewer changes shrinks both,
  at the cost of edit-level history granularity.
- rad's `DELTA_BYTES` is the RADPACK envelope (D1) — the same bytes the
  dogfood servers actually ship.

## Files

| file | what |
|---|---|
| `run_baselines.py` | the harness: runs all legs, medians, cross-checks results |
| `micro/*.{rad,lua,js}` | the four micro-benchmarks, one algorithm each |
| `collab/edits.rad` | the 10k-edit script, rad side |
| `collab/edits_yjs.mjs`, `collab/edits_automerge.mjs` | the same script, CRDT side |
| `RESULTS.md`, `results.json` | regenerated output of the last run |
