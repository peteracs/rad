# RADTACTICS

A tactics battler where the AI thinks by **forking the universe** — and where
who does the thinking is pluggable: the oracle, you, a sandboxed player
script, two machines over a wire, or a whole league of untrusted bots.

Two teams of four (knight / archer / mage / cleric) fight on a 10x7 grid.
The combat brain — schema, evented combat path, speculative systems,
legality, the oracle, the bot pipeline — lives in **`lib_combat.rad`** and is
imported with `use "lib_combat.rad"` by every program here. One brain, six
hosts: `main.rad`, `arena_server.rad`, `arena_client.rad`, `tournament.rad`,
and `web_arena.rad` (the same oracle, in your browser tab).

## How the oracle thinks (per unit, per turn)

1. `checkpoint = fork()` — microsecond copy-on-write snapshot of the world
2. stage a candidate order on the live world
3. one strict `simulate()` frame resolves the order in isolation — its
   **greedy value**
4. `simulate_par(fork(), [system::TargetNearest, system::SimResolve], 4, 3, salt)`
   — sample **3 jittered opponent behaviors 4 frames deep, in parallel**;
   keep the **worst** one (paranoia as a search strategy)
5. score `greedy*2 + worst`; `commit(checkpoint)` — perfect rollback,
   zero game-specific undo code
6. repeat for every candidate; the best blend wins and replays for real

The jitter inside `TargetNearest` is seeded **per fork by `simulate_par`** —
randomness in speculation is legal exactly when it is explicitly seeded
(the checker enforces this: `rand_*` is banned in `simulate()`, allowed in
`simulate_par()`).

A typical battle explores **2-7k forked futures** at ~1,000 universes/sec
(each "universe" is a 4-frame parallel simulation). Only the live path emits
events — which is what makes the autopsy work.

## Five ways to play

```
rad projects/dogfood/tactics/main.rad              # then: enter | play | bot <file>
powershell -File projects/dogfood/tactics/run_arena.ps1            # two-machine battle
powershell -File projects/dogfood/tactics/run_arena.ps1 -Record    # + referee tape & offline replay
rad projects/dogfood/tactics/tournament.rad        # 8-bot league
# browser arena: wasm-pack build --target web --out-dir ../../projects/playground/pkg core/vm
#                then serve the repo root and open projects/playground/arena.html
```

| mode | who commands red | what it demonstrates |
|---|---|---|
| *(blank)* / `auto` | the oracle | speculation at scale |
| `play` | you, one order per unit | `hint` = ask the oracle without taking the turn |
| `bot <file>` | a player-written rad script | `sandbox_run` under `{ "write": ["Intent"] }` |
| arena | a client per side | deltas both ways, zero polling |
| tournament | 8 bots round-robin | multi-tenant sandbox soak |
| browser | the oracle, in wasm | the whole VM (forks, oracle, why()) in a tab |

### Sandboxed bots (`projects/dogfood/tactics/bots/`)

A bot is plain rad source. The host prepends the schema, passes the unit's
identity through **`sandbox_run`'s data-only input channel** (the guest reads
`sandbox_input()["unit"]` — host values are never spliced into guest source),
and grants caps of exactly `{ "write": ["Intent"], "fuel": 2000000 }`. The
host peeks the Intent out of the returned fork, validates it against the
same `candidates_for()` list the oracle searches, and replays it through the
live evented path. The fork is discarded — untrusted code never touches the
real world.

- `berserker.rad` — honest: charges the nearest enemy. It **beat the old
  depth-2 oracle**; the blended depth-4 worst-case oracle now beats it in
  5 rounds. Dogfooding found the AI bug: pure deep search modeled *itself*
  as a berserker (TargetNearest drives every idle unit in the simulation),
  so the greedy term had to come back into the blend.
- `cheater.rad` — writes `Stats { hp: 999 }` → **denied by the write ACL**.
- `spy.rad` — calls `read_file` → **denied by the builtin mask**.

A bot that loops forever starves on fuel and forfeits. Three layers, one
line of caps JSON.

### Two-machine arena, protocol v2: divergence both ways, zero polling

The referee owns the world. A client syncs **once** (`PULL`, ~3.7 KB — the
last full world it ever sees). Each round it stages intents locally, pushes
`ORDERS` with `fork_delta(base, staged)` (~2-3 KB of Intent/Pos rows), and
the connection **parks** inside the referee until the enemy's orders arrive.
The referee resolves once and answers both parked connections with

```
RESOLVED <next_round> <winner|-> <new_digest>\t<delta>
```

where `<delta>` is `fork_delta(your_pushed_state, resolved_world)` — each
client receives exactly the consequences it didn't already know, applies
them with `fork_apply(staged, delta)`, commits, and is bit-identical with
the referee. No `ROUND` polling, no re-pulls, no idle traffic: a round costs
one connection, one delta up, one delta down.

Receipts from live runs:

- `-Tamper` makes red claim `hp: 999` in every delta: the referee reads
  Intents only, logs `TAMPER ... ignored (live world is sovereign)`, and the
  battle resolves identically to the honest run.
- Illegal orders (out of range, blocked tile) are `REFUSED`; the unit
  hesitates.
- Cross-machine provenance: `WHY blue-knight` is answered from the
  referee's causality ledger — handler, event, emitter — for a death the
  client never computed.
- Per-field resource deltas (`res_patch` in the RADDELTA1 body): ticking
  `Battle.round` no longer re-ships the battle journal. Up-deltas stay flat
  round over round.
- Per-field **entity** deltas (`ent_patch`): an hp tick on a 4-component
  unit ships `[eid, [["Stats", [["hp", 27]]]], []]` — not the unit's whole
  row. Full upsert rows remain only for spawns, renames, and newly attached
  components; patched components register in the delta's schema table so a
  receiver under schema drift re-runs its `migrate` block on the patched row.
- A 5-round match: one 3.7 KB sync, then ~10-12 KB of orders up and ~21 KB
  of consequences down per client — divergence, never worlds.
- `-Record`: the referee runs under `--record`. A 5-round networked match is
  a **64.9 KB tape** (23 frames, 78 io records — every accept/read/write);
  `rad replay` re-runs it **offline, no sockets, no clients**, the world
  digest verifies, and all 103 referee stdout lines reproduce byte-for-byte.
  A distributed bug report is now a file you can step through from the
  referee's seat.

### Tournament (`tournament.rad`)

Eight entrants — three file bots (including both hostiles) plus five
personalities **stamped from one source template** (`stalker`, `skirmisher`,
`kiter`, `sniper`, `coward`; bot source is data). Round-robin, every order
of every unit of every round is a fresh sandboxed guest VM:

```
=== final table (28 matches) ===
stalker     18    6  0  1
skirmisher  18    6  0  1
berserker   15    5  0  2
kiter       15    5  0  2
sniper      5     1  2  4
spy         3     0  3  4
cheater     3     0  3  4
coward      2     0  2  5

soak receipt: 2337 sandboxed guest VMs in 4.6 s (505 guests/sec),
725 hostile actions died at the fence, 0 touched the live world
```

The hostile bots finish at the bottom by forfeiting every turn — a
leaderboard of untrusted code that provably can't cheat.

### Browser arena (`web_arena.rad` + `projects/playground/arena.html`)

The whole rad VM compiles to WebAssembly (`wasm-pack build --target web core/vm`),
so the **same merged source** that runs in the terminal runs in a Web Worker:
the page fetches `lib_combat.rad` + `web_arena.rad`, strips the `use` line
(the page is the module loader), rewrites the `SEED` line per battle, and
hands the string to `RadRuntime.compile_and_run`. The battle narrates itself
over a line protocol (`@round` / `@unit` / `@act` / `@log` / `@why`) that the
page parses into an animated replay: grid, hp bars, act ticker, journal, and
the full `why()` autopsy for every fallen unit. A battle explores ~3k forked
futures at ~850 universes/sec **inside the browser** (simulate_par runs its
seeded futures sequentially there — wasm32 has no threads; same results,
same seeds).

## The receipts (single machine)

- **Autopsy**: after the battle, `why blue-knight` prints the causal chain
  of the killing blow. A ring death (`Sapped`) and a blade death (`Struck`)
  are distinguishable forever.
- **Blast fence**: every round (local or networked) ends with
  `assert_only_changed(round_start, fork(), [Pos, Stats, Intent, Battle])`.
- **Replay tape**: `--record` then `rad replay` — verified bit-exact
  (`world digest matches the recorded run`) even with modules, parallel
  jittered speculation, and sandboxed guests in the loop.

## Emergent behavior (nobody coded these)

- **Kiting**: wounded units flee exactly outside enemy threat range.
- **Focus fire**: wounded enemies pull pursuit 3x harder, so the team
  converges on kills without any target-selection code.
- **The blend**: pure deep search dives (it models itself as a berserker);
  pure greed walks into traps. `greedy*2 + worst_of_3_jittered_futures`
  beats both parents.

## What this app found in the language (all fixed in the VM this cycle)

- `sandbox_run` had no input channel — identity was string-spliced into
  guest source. Now: optional 4th argument → `sandbox_input()`.
- `rand_*` was statically banned in **all** speculated systems, but
  `simulate_par` seeds every fork explicitly — exactly the place opponent
  jitter belongs. The checker now distinguishes the two.
- `world_fork` had no surface type name, so a `pub fn` taking a fork
  couldn't satisfy the typed-public-API rule modules enforce.
- UTF-8 BOM at the top of a module was a parse error; the lexer now strips
  it (every Windows editor will eventually do this to you).
- Whole-row resource deltas dragged the battle journal into every push;
  the codec now ships per-field `res_patch` entries.
- Whole-row **entity** deltas re-shipped a unit's Bio-sized components for
  an hp tick; the codec now ships per-field `ent_patch` entries (with
  migrate-on-drift at the receiver).
- A simultaneous ring wipe awarded the win to whichever side was checked
  last — now a draw in all three battle loops.
- **A use-after-free in the VM** (the best find of the cycle): the new
  auto-GC could run while a builtin held heap values in Rust locals across a
  nested rad execution — `simulate()`'s saved event queue, `sort_by`'s keyed
  vec, decode-path `migrate` blocks. The web arena crashed 1-in-3 runs on a
  dangling event payload; auto-GC now pauses for the duration of any builtin
  dispatch, and a threshold-0 regression test pins the exact window.
- `clock()` trapped on wasm32 (`SystemTime::now()` is unimplemented there) —
  now `Date.now()`; `simulate_par` and parallel system batches fall back to
  sequential on wasm32 instead of trapping in rayon's pool spawn.

Stale gripes corrected from the last round table: `else if` chains and the
module system (`use "path.rad"`, `pub`, aliases, lockfiles, even remote
modules with sha256 pins) **already existed** — the dogfood apps just
weren't using them. The real gap was that nothing in `projects/dogfood/` exercised
them; `lib_combat.rad` now does, and it immediately surfaced the three
checker gaps above. Dogfooding works.
