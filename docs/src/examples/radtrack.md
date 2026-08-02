# RADTRACK — an offline-first issue tracker, as a stress test of rad's distributed half

RADTACTICS stressed frames, speculation, and adversarial sync. RADTRACK
stresses everything that project *didn't*: long-lived persistent state,
legitimate concurrent edits from cooperating humans, three-way merge with
conflicts a person has to look at, schema migration against an organically
grown save, and time measured in days instead of frames.

## The pieces

| file | what it is |
|---|---|
| `lib_track.rad` | shared schema + evented mutation path + queries (imported by everything) |
| `track.rad` | the offline CLI client: add/list/show/edit/tag/close, save/load, pull/sync |
| `server.rad` | the sync server: PULL/SYNC/RESOLVE protocol, merge policy in rad |
| `upgrade_v2.rad` | the v2 deploy: schema migration (rename + added field) against a real save |
| `demo/run_sync_demo.ps1` | two clients, one server, a conflict, a resolution, convergence receipts |

Every mutation flows through an event carrying *who* and *when*
(`Changed { ticket, field, to, by, at }`), and handlers are the only writers
after spawn — so `why(t, Ticket)` answers "who closed my ticket?" with a
name, even for edits that arrived over the wire from another machine.

## The sync model (the rad way)

- `PULL` — full fork bytes once; the server remembers the base by digest.
- work offline as long as you like; state persists via `save_world()`.
- `SYNC` — push `fork_delta(base, fork())` (your divergence, ~1 KB). The
  server reconstructs your world with `fork_apply`, three-way merges it
  (`merge_forks(base, server_now, yours)`), and answers with the delta from
  *your pushed state* to the merged world. Nobody re-downloads a world.
- conflicts are **data**, not prose: `Conflict` sum values. Server policy
  (written in rad) auto-resolves bookkeeping (`Ticket.updated` → max,
  `Tracker.next_id` → max); everything else ships to the human as JSON, and
  the client answers `RESOLVE` with per-conflict picks fed to
  `merge_forks_with`.
- convergence is **proved**, not assumed: after every sync, client and
  server compare `world_digest()` — the new state-only digest builtin.

## Receipts from live runs

From `demo/run_sync_demo.ps1` (real output):

- alice and bob pull the same 1073 B base, then edit offline.
- alice syncs first: `1251 B up, 229 B down`, then
  `converged: digests agree` on both ends.
- bob's sync surfaces exactly one human conflict —
  `[0] T-1 Ticket.priority server: 1 yours: 4` — he keeps his, `RESOLVE`
  merges, digests agree again. His close of T-2 merged without ceremony.
- alice re-syncs and receives bob's resolution as a 1230 B delta; all three
  worlds print the same `world_digest()`.
- bob asks the *server* `swhy T-1` and gets cross-machine provenance:
  `Changed { field: "priority", to: "4", by: "bob" } … [via wire b2d32a70]`.
- the whole multi-user session was recorded with `--record incident.radr`
  (31.5 KB) and `rad replay` reproduced the server log byte-for-byte:
  `53 io record(s) consumed, 0 leftover … world digest matches`.
- `upgrade_v2.rad` migrated the organically-grown v1 save: renamed
  `assignee` → `owner` (alice's assignment survived), seeded `estimate`
  from priority per row, and the v2 save round-trips with no migration.

## D1: RADPACK — the wire diet, measured here

The round-table's first deliverable (binary wire format, ≥4x) was built and
measured against this app's own payloads. `bench_pack.rad` grows a
400-ticket tracker organically (evented edits, bodies, closures) and
measures every surface; `run_sync_demo.ps1 -Record` produces the tape.

| surface | JSON (before) | RADPACK (after) | ratio |
|---|---|---|---|
| `save_world` (400 tickets) | 75,501 B | 9,204 B | **8.2x** |
| `fork_to_bytes` (state + provenance) | 227,673 B | 29,469 B | **7.7x** |
| `fork_delta` (40-edit offline session) | 21,798 B | 3,504 B | **6.2x** |
| `--record` incident tape | 31,510 B | 7,663 B | **4.1x** |

Receipts that mattered more than the ratios:

- **Content addressing held.** The 400-ticket world's fork digest is
  byte-identical before and after the format change (`9cbdb5c5…`), because
  the digest covers the uncompressed canonical body, not the envelope.
- **Compat is proven, not promised.** The pre-RADPACK tape
  (`incident_v1.radr`, raw JSONL) replays verified on the new binary; a
  DEFLATE-vintage packed tape decodes too (pinned by unit test); legacy
  `RADFORK2`/`RADDELTA1`/`RADWORLD2` payloads pass through untouched.
- **Dogfooding caught a real break.** The first cut changed what
  `world_digest()` hashed — old tapes diverged on replay (19 of 53 io
  records consumed, then a wire mismatch). Fixed to hash the exact legacy
  bytes; the old tape then verified. A digest is an API.
- **Sub-threshold payloads stay readable.** This demo's 1 KB syncs still
  ship as legacy JSON on purpose: the envelope only pays past ~4 KB, and
  debuggability of small payloads is worth more than a few dozen bytes.

## D2: the soundness gate — fuzz the boundary, in anger

The contract: **bytes from outside the process produce an `Err`, never a
panic.** The harness (`core/vm/src/fuzz_tests.rs`) is structure-aware because it
has to be — every payload carries a blake3 digest, so blind byte-flipping
dies at the integrity check without reaching the parser. The mutator
re-seals half its mutants with *correct* digests so they penetrate to the
JSON layer, schema validation, the id allocator, and the migrate machinery;
the other half keeps stale digests to exercise the rejection path. The
corpus is grown by seeded rad programs (unicode names, in-flight events,
despawn free-lists) plus this app's real saves and incident tapes; a
semantically-poisoned corpus covers shapes random mutation never finds
(allocator lies, duplicate ids, u64-max ids, arity drift). Survivors of the
codec are piped onward into `merge_forks`. CI runs it every push; a nightly
ASAN job (`soundness.yml`) soaks it deep — ASAN, not Miri, because the zstd
envelope crosses a C FFI boundary Miri can't execute.

The gate found three real bug classes on its first soak (504,000 cases,
7 panics):

1. **A tamper *report* that aborts.** Digest-mismatch errors quoted the
   claimed digest with `&claimed[..12]` — a multi-byte UTF-8 char at byte
   12 turned the error message itself into a panic. Six sites
   (`radpack::open`, `open_file`, both wire decoders, two provenance
   previews). The error path is attacker-reachable by definition; now
   char-boundary safe.
2. **One entity id = remote DoS + abort.** A payload claiming entity id
   2⁶⁴−1 spun the free-list gap-fill for 60 seconds, then died on
   `Entity ID overflow`. Fixed at the boundary with the world's own
   invariant: every issued id is live or free, so `next_id ≤ live + free`
   — validated *before* any insert, in `fork_from_bytes` and `fork_apply`
   both. The poison now dies in 0.02 s with an honest `Err`.
3. **Silent id truncation.** Wire ids were cast `as u32` — id 2³²+5
   truncates to 5, and a hostile delta *despawns or patches the wrong
   entity* without any error at all. Every wire id now goes through
   `try_from`.

After the fixes, the identical soak: **552,000 cases, 0 panics** —
20,160-case decode sweep × 25 budget, 36,000 byte-level tape mutations,
998 decoded mutants reaching the merge engine, 18 poisons × 2 codecs, and
the gc_pause stress (collector firing at *every* allocation through the
full codec+merge+commit path). The harness also fuzzes itself: a planted
panic must be caught and reported, so a forever-green gate stays
falsifiable. All 809 + 17 tests green; findings auto-dump repro files to
`temp/rad_fuzz_findings/`.

## D3: name claims become resolvable — the gap #6 payoff

Gap #6 below was this app's sharpest finding: two offline clients both
running `next_id: 3` both create `T-3`, and the resulting `NameConflict`
was the one conflict kind a human *couldn't* answer through
`merge_forks_with`. D3 closed it: a name claim now takes a **rename
resolution** — a list of new names, one per claiming entity ("" unnames) —
and the merge applies the renames, then **re-validates** the claims: chosen
names that still collide, with each other or with an entity the forks never
touched, come back as conflicts. A rename can resolve a claim but can never
steal a name unnoticed. (`RenameConflict` takes its chosen name the same
way.)

Receipts from `demo/run_name_demo.ps1` (real output):

- alice and bob pull the same base (`next_id: 3`), go offline, and both
  `add` a ticket — both trackers mint `T-3`.
- alice syncs first: clean merge, her `T-3` (dark mode) lands.
- bob syncs: `1 conflict — name claim: 'T-3' was created on both sides`.
  The picker offers `keep both? server's stays 'T-3/a', yours becomes
  'T-3/b'`; bob answers `yes`, which ships `RESOLVE 0:rename=T-3/a|T-3/b`.
- The merge applies both renames; `list all` on every machine shows
  `T-3/a Add dark mode toggle` **and** `T-3/b Fix login redirect bug` —
  nobody's ticket was lost, nobody re-entered anything.
- `world_digest` agrees on **all three machines** after the resolve:
  alice, bob, and the server all print `45aa9531…` — the D3 PASS criterion,
  verbatim.
- Provenance survived the rename: `swhy T-3/b` answers with bob's spawn
  history `[via wire 4dd4cae3, remote frame]` under the *new* name.
- `Tracker.next_id` didn't even conflict: both sides bumped 3 → 4, and
  convergent edits are not conflicts.
- Safety is pinned in `core/vm/src/merge.rs` tests: renames that still collide
  re-conflict (`rename_resolution_still_colliding_reconflicts`), renames
  cannot steal an untouched entity's name
  (`rename_resolution_cannot_steal_untouched_name`), and `""` unnames
  without dropping data.

## D4: streaming VM sessions — no compile_and_run per keystroke

The deliverable: a session API (push events, pump frames, subscribe to
deltas) on the embeddable runtime, native and WASM. `RadRuntime` grew
`session_start / session_emit / session_pump / session_delta /
session_apply / session_state / session_load / session_digest`
(`core/vm/src/wasm.rs`), backed by a new `VM::enqueue_event` that mirrors
`Op::Emit` — host-pushed events get real trace ids and causality records,
so `why()` answers for a keystroke the same as for a rad-emitted event.
The session base is held as a bare `Arc<WorldSnapshot>`, not a gc `Value`:
runtime fields are invisible to the collector, and D2 taught us what
happens to unrooted values.

Receipts, measured live in 3 real browser tabs (`projects/playground/collab.html`,
collaborative notes over `BroadcastChannel`, each tab running the full VM
in a worker):

- Host election + late-join handshake: tab 1 becomes `(host)`, tabs 2-3
  adopt its `session_state` and become `(replica)`.
- A note typed in the host appears in both replicas; a note typed in a
  *replica* routes to the host, runs the `AddNote` handler there, and
  lands in all three tabs.
- **Latency: 19-20 ms** edit→applied across tabs (PASS bar: <100 ms) —
  measured replica→host→broadcast→replica, wall clock.
- **`world_digest` identical on all 3 tabs** after every flush
  (`94d61293d3946668…` in the live run): replicas never ran a handler,
  they converged on `fork_delta` alone — 2 deltas, 1,251 B total, for two
  edits including spawn + resource bump.
- Wrong-lineage protection live: a delta applied out of order is refused
  by the base fingerprint (`fuzz`-grade honesty in a UI path).
- Pinned natively in `core/vm/src/wasm.rs` tests: 3-session convergence with
  equal digests per frame, out-of-order refusal, late-joiner state
  adoption, per-frame output, host-event causality — 6 tests, all green.
- Dogfooding found two real embedding bugs before any user did: the
  host-election reply raced WASM init (a replica flipped to host because
  the `host-here` answer arrived before the channel handler existed), and
  cross-tab latency was measured with `performance.now()` whose origin
  differs per tab (negative milliseconds). Both fixed in the demo.

## D6: external baselines — published, including the losses

"No baselines = vaporware" was the round-table's sharpest demand, with the
PASS explicitly requiring numbers **even if we lose some**. Done:
`benches/baselines/run_baselines.py` is the one reproducible script
(median of 5, results cross-checked across runtimes — a row whose computed
values disagree is refused, not reported). Numbers live in the repo README
and [External Baselines](../reference/external-baselines.md).

What we lost, plainly: rad's interpreter is **6–12x slower than Lua 5.5**
on scalar call/loop code, ~3x behind QuickJS, and naive string concat is
10x behind Lua (immutable strings, no rope/builder — a real gap, now with
a number on it). Node/V8 beats every interpreter in the table, as JITs do.

What we won: stdlib sort (rad 28 ms vs Lua 63 / QuickJS 123 — thin
scripting over fat native primitives), and the leg the language is *for*:
the **same 10k-edit script** (identical MINSTD stream, 2000 cells,
checksums verified) against the field's standard CRDTs —

| | apply | delta bytes | peer ingest |
|---|---|---|---|
| rad | **38 ms** | **45,251** | **7 ms** |
| Yjs | 106 ms | 100,864 | 45 ms |
| Automerge | 11,969 ms | 1,214,749 | 287 ms |

— plus three-way `merge_forks` at +12 ms. Caveat stated in the writeup:
CRDTs converge pairwise without a base; rad syncs against a pulled base,
which is exactly the PULL→SYNC topology RADTRACK and RADSHEET run.

## D7: the counterfactual finale

The last deliverable on the board, and the one no mainstream stack can
follow: a RADSHEET build ships with a one-character formula bug (`SUM`
drops the last row of every range), two humans spend a recorded session
entering numbers, and the books silently don't balance — `TOTALS 2490
2795 5285` with row 4 visible in the grid and missing from every sum.

Three commands, all answered **from the tape**
(`projects/dogfood/radsheet/incident/`):

1. `rad replay incident.radr` — the corruption reproduces byte-for-byte:
   `17 io record(s) consumed, 0 leftover … world digest matches`.
2. `rad replay incident.radr --with fixed.rad` — the counterfactual. The
   *recorded keystrokes* replay against the one-character fix: the clean
   sheet prints (`2565 2855 5420`), and the diff comes from the tapes
   themselves — blast radius `{Cell: 3}` (exactly B5, C5, D5), original
   digest `966b71db…` vs edited `663aa015…`, `0 unused` io proving the
   fixed build consumed the identical input stream.
3. `rad replay incident.radr --serve` + `why(B5)` — forensics on the
   corrupt timeline: `val: 2490 ← on CellSet ← CellSet { raw:
   "=SUM(B1:B4)", by: "alice" } frame 26`. Who typed the formula whose
   result was corrupted, from a file, months later.

That closes the board: D1 RADPACK, D2 soundness gate, D3 resolvable name
claims, D4 streaming sessions, D5 RADSHEET, D6 published baselines, D7
counterfactual replay — every PASS criterion met with receipts in-repo.

## Tier-1 #1: indexed queries — real, wired, battle-tested

The post-D7 review's sharpest claim: `query_where`/`lookup` "are either
undocumented or vaporware — both are bugs." The verdict from wiring them
up for real:

- **`indexed` fields and `lookup()` were real** (parser → checker →
  compiler → a maintained hash index in the world) but no dogfood app had
  ever used them — and the battle tests immediately found out why that
  mattered:
- **The founding bug: every client lost its indexes the moment it
  pulled.** `fork_from_bytes` built its world with empty index
  declarations, so `commit()` of any wire-ingested fork silently wiped the
  live world's indexes — `lookup()` then errored "not indexed" forever.
  Reproduced against the pre-fix binary
  (`lookup() requires an indexed field` after a wire round-trip; the fixed
  build answers `hp=10`). Fixed twice over: decode worlds are seeded with
  the program's declarations (indices build as rows land), and `commit()`
  reconciles against the program's `indexed` declarations — the compile
  result is the source of truth, snapshots only carry derived state.
- **`lookup_all` added** — the multi-match verb every app was
  hand-rolling: all matches, ids ascending. `lookup` with duplicate keys
  now returns the **min id** (was: insertion order, which differed between
  a live world and one rebuilt from a save).
- **Battle tests** (`core/vm/src/index_tests.rs`, 19 cases): maintenance through
  set/update/remove/despawn/handlers, id reuse after despawn (no ghost
  matches), unicode/empty-string/bool/float/entity keys, float bit-pattern
  semantics pinned (`0.0` vs `-0.0` are distinct buckets — documented, not
  hidden), loud errors on unindexed fields, survival across fork/commit
  rewinds, **the wire codec**, save/load, delta+merge, schema migration
  (index built over migrated rows), a 5k-entity full-scan oracle
  cross-check, GC pressure at collect-every-allocation, and record/replay
  digest verification.
- **RADTRACK wired**: `Ticket.status` and `Ticket.assignee` are `indexed`;
  `ticket_ids()` is one hash probe; old saves load and index fine; the
  full two-client sync demo still converges. The receipt
  (`bench_index.rad`, release build, 50k tickets): scan **135 ms/query**,
  `lookup_all` **0.2 ms/query** — ~675x, identical results.
- Also found by the suite's own harness: compiling without the checker
  silently produces a world with **no indexes at all** —
  `indexed_component_fields` flows through the checker's component table,
  which embedders bypassing the checker would never learn. Documented in
  the test header.

## Tier-1 #2: the O(n²) string hot path — 2.8x off the loop, 124x off the idiom

The profile found **three full copies of the accumulator per `s = s + x`**:
`Arc<str>` → `String` (`into_string`), an exact-capacity `push_str` that
always reallocs, and `String` → `Arc` on the way back into the heap. On
top of that, f-strings compiled to k-1 chained binary `Add`s — every part
re-copying the growing prefix — and so did `a + "b" + f"{c}"` chains.

The fix, in three layers:

1. `binary_add`'s string arm builds **one exact-capacity buffer** (3
   copies → 1 + the Arc copy).
2. New `Op::ConcatN`: n-ary concat in a single buffer. **f-strings now
   cost one allocation** regardless of part count.
3. **Chain fusion**: the compiler flattens `+` chains containing a string
   literal or f-string into one `ConcatN`. Sound because rad has no
   implicit coercion — such a chain either succeeds all-string or was a
   type error under pairwise `+` too. Evaluation order unchanged; fusion
   never reassociates across explicit parentheses.

Receipts (release, 60k-iteration str_build, same-run ratios from the
baselines harness): **10.2x behind Lua → 6.1x** on the naive loop; the
O(n) idiom `join(map(range(n), fn), "")` runs **57 ms — ~9x faster than
Lua's naive loop**. All 839 + 17 tests green through the new lowering
(every f-string in the suite now compiles to ConcatN).

And the benchmark caught a fresh Tier-1-class finding: the "builder"
pattern `parts << x` is **slower than the naive concat loop** (8503 ms vs
2602) — value-semantics list push deep-copies the entire list per `<<`
(`bi_push` → `Arc::make_mut` with a shared backing vector). Every
`rows << x` loop in every dogfood app pays O(n) per push. Logged as the
next structural gap (persistent vectors or uniqueness-aware push).

## Tier-1 #3: the convergence receipt across a schema migration

The scariest review finding, because it was invisible: `world_digest`
hashes the canonical body *including the schema*, so a v1 client and a v2
server with the same logical data print different digests **by
construction** — the convergence receipt that D3/D4/D5 lean on goes blind
exactly during a rolling upgrade.

The fix is two primitives plus a protocol:

- **`schema_digest()`** — fingerprint of the program's declared layouts.
  Peers exchange it first: equal → raw digests comparable; different →
  a digest mismatch means *vintage*, not *divergence*.
- **`world_digest(fork)`** — the state-only digest of a fork **without
  committing it**. The upgraded side decodes the older peer's bytes
  (migrate-on-ingest runs the declared `migrate` blocks) and digests the
  migrated view: both sides of the comparison now carry the same schema,
  so equality is logical convergence.
- **`CERTIFY`** in the RADTRACK protocol: the client ships full fork
  bytes; the server answers `<migrated-view-digest> <own-digest>
  MATCH|MISMATCH`. The `digest` command detects skew and certifies
  automatically.

Receipts from `demo/run_rolling_demo.ps1` (real output):

- Phase 1 (both v1): `converged: digests agree` — unchanged.
- The server upgrades (`assignee→owner`, `estimate` derived: alice's P1
  ticket gets `est=8`); raw digests now differ (`e1bb…` vs `747a…`).
- alice, still on v1: `schema skew detected (rolling upgrade) —
  requesting certification` → server: `CERTIFY: migrated client view
  747a3e80… vs ours -> MATCH` → client prints **`converged: server
  certifies our migrated view`**. The receipt survived the migration.
- Honesty preserved: alice edits offline and asks again →
  `DIVERGED (certified): … MISMATCH` — a real divergence still reports
  truthfully, through the same migrated-view lens.
- Pinned in `core/vm/src/migration_tests.rs`: cross-version certification
  (migrated v1 view ≡ native v2 twin), derived-field coverage (a twin
  with one wrong derived value does NOT certify), state-only fork
  digests (in-flight events don't move them; inspecting a fork doesn't
  mutate the live world), and schema_digest tracking declarations (data
  doesn't move it; an added or renamed field does).

## Tier-2 #1: property-fuzzing the rad-level evaluator

D2 fuzzed the plumbing; D7 then shipped *application logic* that corrupted
cells, and only the counterfactual replay caught it after the fact.
`core/vm/src/sheet_property_tests.rs` makes that bug class unable to reach
main: the shipped `lib_sheet.rad` engine (baked in via `include_str!`) is
driven with randomly generated grids, edit storms, formulas, and three-way
merges, and must uphold:

- **P1 derive-invariant** — every stored `(val, kind)` re-derives from
  its `raw`.
- **P2 range/chain agreement** — `=SUM(rect)` equals the explicit
  `=A1+A2+…` chain over the same cells. Different iteration code paths:
  **D7's dropped-last-row dies here**, and the suite proves its own teeth
  by re-planting that exact bug and asserting a violation fires.
- **P3 algebraic identities** — `COUNT·AVG ≈ SUM`, `MIN ≤ AVG ≤ MAX`.
- **P4 reflow idempotence** — a second cascade changes nothing.
- **P5 the D5 property** — after a random three-way merge resolved with
  the server's policy + reflow, P1 holds for every cell: derived state
  can never silently survive a merge of its sources again.
- **P6 determinism** — every scenario records and replays
  digest-verified.

Generated sheets are DAGs by construction (formulas reference strictly
earlier rows) so P1/P4 stay falsifiable; the cycle path keeps its own
fixpoint-cap coverage in the engine smoke. Receipts: 48 scenarios per CI
run (~12 s), 300-scenario soak green in 77 s, 0 violations.

## Tier-2 #2: the collab demo crosses a real network

The review's critique: D4's three tabs shared a BroadcastChannel — "an IPC
claim, not a network claim." Now `projects/playground/relay/relay.mjs` (a dumb
WebSocket fan-out; all session semantics stay in the rad VM) and a
transport switch in `collab.html`: `?relay=ws://host:8378` replaces
BroadcastChannel with real sockets, same protocol byte-for-byte.

Measured live, 3 peers through the relay: a replica loaded the page AND
connected via `192.168.16.1` (a non-loopback interface — page, WebSocket,
and deltas all routed through the NIC stack, not same-process IPC), typed
an edit, and all three peers converged on digest `9f48db18…` with
**45–55 ms** edit→applied through the relay path. A second machine joins
with the same URL pointed at the host's LAN IP — zero code changes; the
single-box runs above are what was measured tonight, stated plainly.

## What this app found in the language

Dogfooding receipts — each of these was hit while building RADTRACK, not
hypothesized:

1. **`sys_args()` was dead on arrival** (fixed). The builtin existed, but
   the CLI rejected any positional after the script — there was literally
   no way to pass arguments to a rad program. Added `rad file.rad --
   <args…>`; `sys_args()` now returns exactly the post-`--` args (it used
   to leak the interpreter path and rad's own flags), and it's
   replay-managed so recorded sessions replay with the args they saw.
2. **No state-only digest** (fixed: `world_digest()`). Fork digests cover
   provenance, in-flight events, and id free-lists — all legitimately
   different on two machines whose worlds agree — so there was no way to
   *prove* two processes converged. First demo run printed
   `synced, but digest mismatch` on a correct sync. `world_digest()`
   hashes the canonical `save_world` body only; pinned by
   `world_digest_tracks_state_not_history`. (And since Tier-1 #3 it
   survives rolling migrations via `schema_digest()` + certification —
   see that section above.)
3. **Built-in `Conflict` can't cross a `pub fn` boundary.** The checker
   flags `pub fn conflict_json(c: Conflict)` as a private-type leak, so
   conflict serialization can't live in a shared library — it's duplicated
   in `server.rad` instead. Built-in sum types should be public.
4. **`-> entity` can't be nil-checked.** `get_entity` returns an untyped
   value that compares against `nil`, but the moment a wrapper declares
   `-> entity`, `if t == nil` is a type error. There's no `Option<entity>`
   idiom, so typed lookup helpers are impossible; every call site uses raw
   `get_entity`.
5. **No indexed queries** (fixed: Tier-1 #1, see the section below).
   `entities(Ticket)` was a full scan and every list view paid O(world).
   `Ticket.status`/`assignee` are now `indexed`; `ticket_ids()` answers
   with one `lookup_all` hash probe — ~675x faster at 50k tickets. Sorting
   by priority is still by hand: indexes are hash, not ordered (the
   remaining half of the gap, honestly scoped).
6. **Distributed id minting was unsolved at the language level** (fixed:
   D3). Two offline clients both running `next_id: 5` will both create
   `T-5`, and the merge used to report a structural `NameConflict` that
   `merge_forks_with` *could not* resolve. Since D3, a name claim takes a
   rename resolution ("keep both as T-5/a, T-5/b") and the merge
   re-validates the chosen names — see the D3 section above for the
   three-machine convergence receipts.
7. **Conflict prompts vs. scripted stdin.** The client reads conflict
   resolutions with `readline()` — which is the same stream the session
   script pipes commands through. It works, but only because conflicts
   arrive deterministically; an interactive TTY/pipe distinction
   (`is_tty()`) doesn't exist.
8. **No date/duration type.** `now_unix_s()` + hand-rolled `fmt_age`
   ("3d ago") is the entire time story. A tracker wants "due Friday".

## Run it

```powershell
# the full two-client conflict demo (add -Record for the incident tape)
powershell -File projects/dogfood/radtrack/demo/run_sync_demo.ps1 -Record

# the D3 name-claim demo: both clients mint T-3 offline, picker keeps both
powershell -File projects/dogfood/radtrack/demo/run_name_demo.ps1

# replay the recorded server session from the referee's seat
./target/debug/rad.exe replay projects/dogfood/radtrack/demo/incident.radr

# the v2 schema deploy against the save the demo grew
./target/debug/rad.exe projects/dogfood/radtrack/upgrade_v2.rad -- projects/dogfood/radtrack/demo/server_world.radw

# poke at it interactively
./target/debug/rad.exe projects/dogfood/radtrack/server.rad -- mytracker.radw   # terminal 1
./target/debug/rad.exe projects/dogfood/radtrack/track.rad -- you ./mydir      # terminal 2
```
