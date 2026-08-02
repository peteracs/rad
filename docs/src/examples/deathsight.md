# DEATHSIGHT

A roguelike where the dungeon floor shows you the probability of your death
**before every move** — and when you die anyway, the game searches the
multiverse for the timeline where you lived, computes **the exact turn your
death became mathematically inevitable**, and replays the ghost of the life
you should have lived.

None of this is engine code. It's ~700 lines of rad, because forking the
universe is a builtin. The brain lives in `lib_sight.rad`; two hosts import
it: `main.rad` (terminal) and `web_deathsight.rad` (browser).

## Play it in your browser (the mainstream version)

```
# pkg already built? just serve the repo root:
py -m http.server 8137
# then open http://localhost:8137/projects/playground/deathsight.html

# rebuild the wasm VM first if needed:
wasm-pack build --target web --out-dir ../../projects/playground/pkg core/vm
```

The page runs the **whole rad VM in a Web Worker** (WebAssembly) and renders
the @-line protocol as an animated dungeon: death-odds badges pulse on the
tiles, the autopsy types itself out, necromancy rewinds timeline by timeline,
and the ghost 👻 replays its escape past your corpse 💀. Three buttons:

| button | what happens |
|---|---|
| ☠ watch the doomed hero | beelines for the exit, dies, gets the full necromancy cinematic |
| 👁 watch the oracle escape | the sight plays itself — same dungeon, walks out alive |
| ⚔ PLAY | you, with wasd / arrows / clicking tiles — every move re-runs the deterministic universe with your move script baked in (the same trick the arena page uses for seeds) |

A demo death in-tab burns ~9,600 forked universes in ~5 s of wasm
(~1,900 universes/sec, sequential — wasm32 has no threads).

## Terminal version

```
rad projects/dogfood/deathsight/main.rad
```

First input line picks a mode:

| mode | who plays |
|---|---|
| *(blank)* / `demo` | a doomed hero who ignores the sight and beelines for the exit |
| `oracle` | the sight itself plays — watch the same dungeon get beaten |
| `play` | you: `w/a/s/d` to move, walk into a monster to smite it, `.` to wait, `q` to concede |

## The three mechanics

### 1. The sight

Every turn, every legal move is **staged on the live world**, then
`simulate_par()` samples 8 jittered monster futures 6 turns deep, in
parallel. The futures model *you* too (`FleeOrFight`, a system that runs
only inside the simulation): the number on each move means "probability you
die within 6 turns **even if you play well from here**". Then
`commit(checkpoint)` puts the universe back, byte for byte.

```
  # @ . . g . g . g . . > #     <- the corridor IS the shortest path. that's the trap.
  you 18/18 hp   [spitter-1 4, ghoul-1 6, ghoul-2 6, ghoul-3 6, hound-1 4]
  deathsight> stay 0% | N 0% | S 0% | E 0%
```

### 2. Necromancy (the point of no return)

The game keeps one `fork()` per turn — copy-on-write, effectively free.
When you die it rewinds 1, 2, 3… turns; at each rewind point it branches
the search across **every possible first move**, each followed by oracle
play. The smallest rewind that escapes brackets the exact move where your
death became inevitable:

```
NECROMANCY: searching the multiverse for the timeline where you live...
  rewind 1 turn(s) -> tried all 3 first moves: every line ends in death
  ...
  rewind 8 turn(s) -> tried all 5 first moves: every line ends in death
  rewind 9 turn(s), first move W -> A SURVIVING TIMELINE EXISTS

=== THE POINT OF NO RETURN was turn 3 ===
at turn 2 the multiverse still contained an escape —
you went E:smite ghoul-1, and after that every future ends in death.
```

You died on turn 10. Your death was sealed on turn 3 — the moment you
committed to grinding through the corridor, seven turns before your hp hit
zero. Then the ghost replay runs, move by move, annotated with what you did
instead (`ghost turn 2: goes W   (you went: E:smite ghoul-1)`): it backs
out of the corridor, takes the north route, strings the chasers out, kills
them one at a time at a choke, smites the spitter, walks out.

### 3. The autopsy

Every live hp write flows through events (`Clawed`, `Smote`, `Slain`), so
the causality ledger can name your killer — handler, event, emitter:

```
autopsy (the causality ledger):
Vital of you = { hp: 0, max_hp: 18 }   (set in frame 14)
  <- by `on Clawed` handler
  <- Clawed { monster: spitter-1, dmg: 2 } emitted in frame 13
  <- by top-level code
```

## Receipts

- **demo mode**: dies turn 10; necromancy probes 9 rewind points × every
  first move (43 full ghost playthroughs) — **9,600 forked futures in
  ~2.9 s (~3,300 universes/sec)** — and finds the surviving timeline.
- **oracle mode**: the same dungeon, beaten on turn 24. The winning line
  (hold the wall, kill the ghouls one at a time where they can't flank,
  sweep south, one-shot the spitter at the door) is **emergent** — nobody
  coded "hold the wall"; it falls out of min-death-probability with exit
  pressure.
- **deterministic**: two runs differ only in the timing line. The whole
  thing records to an **8 KB tape**:
  `rad projects/dogfood/deathsight/main.rad --record death.radr` then
  `rad replay death.radr` reproduces the death, the autopsy, and the
  entire multiverse search byte-for-byte, offline.
- **the fence**: every live turn ends with
  `assert_only_changed(turn_start, fork(), [Pos, Vital, Intent, Run])` —
  after a turn that staged candidates on the live world, simulated dozens
  of futures, and rolled everything back, the language proves nothing else
  in the universe changed.

## What this app found in the language (fixed in the VM this cycle)

- `why()` autopsies printed entity **ids** in event payloads —
  `Clawed { monster: 1, dmg: 2 }` — even though the ledger knows entity 1
  is `spitter-1`. The ledger already named the *written* entity
  ("Vital of you"); now emit payloads resolve spawn names too
  (`ledger_payload` in the VM, all three emit sites). All 838 VM tests
  pass; 8 pre-existing conformance failures around heterogeneous map
  literals are unrelated (checker tightening from an earlier cycle).

And what it found but did NOT fix (filed as gripes for the next cycle):

- `rad lint` flags `pub` declarations in a library module as "unused" when
  the file is linted standalone (18 false positives on `lib_sight.rad`) —
  the linter doesn't treat `pub` as an export.
- `rad fmt` (write mode) produced structurally wrong indentation on the
  pre-split `main.rad` (a `match` inside a `for` inside a long `fn` pulled
  every following top-level declaration one level right). The program still
  ran — braces stayed balanced — but the file read as if everything after
  `deathsight()` were nested inside it. `fmt --check` accepts the
  hand-formatted files, so the round-trip bug is in the writer.

And what it found in *writing rad* (no VM change needed, but instructive):

- A forecast is only as good as its model of you. Freezing the player in
  the futures made every deep future end in death — the sight went blind
  (uniform 100%), exactly the self-model bug RADTACTICS hit from the other
  direction.
- Frame order inside `simulate` must mirror the live loop (monsters answer
  your staged move *first*, then imagined-you acts) or the sight reports a
  one-turn-shifted universe: it said `stay 0%`, you stayed, you died.
- `commit()` rewinds resources too: the universes-explored counter and the
  RNG salt both lived in a resource and got rolled back with every
  necromancy rewind. Locals survive commits (the search cost accumulates in
  one), and the live RNG is VM state, not world state — `rand_int` salts
  stay deterministic under `rand_seed()` yet diverge across rewound
  timelines. The memory model's air gap, used as a feature.
- Manhattan distance walks heroes into wall pockets (the cleared-dungeon
  hero doing `stay` six tiles from the door, forever). The BFS flow field
  is 25 lines of rad.
