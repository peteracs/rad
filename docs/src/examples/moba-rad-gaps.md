# rad language gaps — found porting browser-moba's sim (the dogfood list)

Writing real game logic in rad (damage_core.rad, treeline.rad, duel.rad)
surfaced these. Ranked by how much they hurt clean code. Each should be
fixed in the language, then the specs rewritten to use it — that's the
dogfood loop.

Status: ALL SEVEN are FIXED and dogfooded. The shield unroll and the
resource ceremony in damage_core.rad are gone and the golden corpus
stayed bit-identical through both rewrites — the conformance gate did
its job. Conformance coverage lives in `tests/conformance/` (shift_ops,
update_indexed, field_names_keywords, resource_res, modules/pub_let_*,
plus the *_reject diagnostics) and the checker unit tests at the bottom
of `core/vm/src/checker/tests.rs`.

## 1. Array fields in components — FIXED
List fields in components already worked (`shields: list = [0, 0, 0]`);
the missing piece was element assignment in update blocks. Now:
```
update(e, Ent) { shields[i] = v }            // indexed update, in order
update(e, Ent) { items["sword"] = 1 }        // map fields too (typed keys)
update(e, Ent) { shields = xs, shields[0] = v } // plain seeds, index patches
let ys = set_at(xs, i, v)                    // pure expression form
let m2 = set_at(m, "k", v)                   //   ...for maps as well
```
One index level per entry; nested writes get an actionable parse error
with the `set_at` recipe. damage_core.rad's hand-unrolled s0/s1/s2 is now
a `shields` list with a `SHIELD_SLOTS` soak loop, bit-identical goldens.

## 2. Top-level `pub let` constants — FIXED
`pub let TICK_RATE = 30` parses, exports through bare `use`, and is
exempt from unused warnings (it's a module export). Guardrails: single
name only, immutable (`pub let mut` errors with a pointer at resources),
and two modules exporting the same constant name is a load error naming
both definition sites (never silent last-write-wins; private lets keep
their historical file-local coexistence). Module aliases (`use ... as m`)
don't expose lets — the checker says so explicitly and suggests bare
`use` or a pub fn. Bonus fix found in the follow-up pass: an ALIASED
module's own fns couldn't read the module's top-level lets (checker-only
"Undefined variable"; the compiled code was fine) — the checker now
defines those lets before checking fn twins.

## 3. Bit operators — FIXED
`<<` and `>>` are int expressions with C/Rust precedence (looser than
`+`, tighter than `&`/`^`/`|`, then comparisons). Semantics are identical
to the shl()/shr() builtins: logical shifts, out-of-range count -> 0.
Statement-position `xs << v` is still list append (now with chaining:
`xs << a << b`); using `<<` on a list in an expression points at push().
One deliberate precedence consequence: appending a bare comparison
(`xs << a > b`) parses as `(xs << a) > b` and errors with a parens hint —
write `xs << (a > b)`. `>>` lexes as two adjacent `>` so nested generics
keep parsing; the formatter knows to keep the pair glued.

## 4. Reserved words as field names — FIXED
`on`, `state`, `entity`, `component`, `for`, `match`, ... work in every
field position: component/struct/resource/event declarations, literals,
`.access` (f-strings included), update blocks, emit payloads, spread, and
match patterns. Literal keywords (`true`/`false`/`nil`) stay reserved.
Binding-position names are unchanged — `Comp { on: x }` binds `x`, and
shorthand patterns on keyword fields (`Comp { on }`) are a parse error
with an alias hint, since the binding could never be referenced. A field
literally named `indexed` also works (the marker form requires a field
name after it).

## 5. f-string expression power — STALE (already worked)
Both `f"{m["k"]}"` and `f"{m[\"k\"]}"` parse and run today; pinned in
conformance. The original failure predates the f-string lexer rework.

## 6. Builtin shadowing — FIXED
The checker now resolves calls the way the runtime does: bindings shadow
builtins. A parameter named `range` plus a `range(...)` call is a
compile-time error with a rename hint (it used to be a runtime
"Not callable: int"). Defining any non-function binding with a builtin's
name warns at the definition site.

## 7. Module-level mutable state ergonomics — FIXED
Resources stay the one tool for module state (deliberately NOT solved
with `pub let mut` — exported mutable globals are the wrong tool), but
the ceremony is gone. Declared resources auto-initialize from field
defaults, so `res(R)` reads the value directly — no Option, no unwrap —
and composes with `update` into one-line read-modify-writes:
```
fn rng_next(modulo: int) -> int {
    update(Rng) { s = (res(Rng).s * 1103515245 + 12345) % 2147483648 }
    return (res(Rng).s / 65536) % modulo
}
```
(was: get_resource + unwrap + recompute + set_resource). The checker
types `res(R).field` precisely, errors on components/unknown names, and
classifies res() as a readonly effect — allowed in `readonly fn`,
rejected in `pure fn`. damage_core.rad's RNG and DR-table reads use it;
goldens stayed bit-identical.

Fixed already during this effort (for the record): `pub` reachability
false positives (handler-only functions, pub exports), `%` on negative
operands documented by tests, update-block `=` vs `:` confusion is a
parse error with a clear message, mixed-value map literals widen to
`map<K, any>` with a warning (mirroring lists) instead of hard-erroring,
and `rad fmt` no longer splits `>>` into `> >`.

---

# Round 2: the BUFFCORE fake port (dream-code-driven design)

Method flip: instead of porting browser-moba and noting pain, we wrote
`buffcore.rad` — the buffs + stat pipeline cluster (sim/buffs.ts 57KB +
sim/stats.ts) — in the most beautiful rad imaginable, then made the
language catch up. Every failure became a feature, same gates as round 1
(conformance + checker tests; the file runs hand-verified MOBA math).

Features the dream code forced:

1. **`~` bitwise NOT** — `allowed & ~revoked` for permission masks
   (stun revokes MOVE|ATTACK|CAST exactly like it contributes StatMods).
2. **`for x in xs where cond`** — filtered iteration as sugar for a
   body-wrapping `if`; `for inst in b.active where defs[inst.def].is_cc`.
3. **`.field` accessor shorthand** — `mods |> map(.flat) |> sum` is the
   stat fold; chains (`.stats.hp`) project through nesting.
4. **`sum()` / `product()`** — the numeric folds every stat pipeline ends
   in; int-preserving, float-promoting, identity on empty.
5. **`self` in systems** — the VM has always pushed the visited entity;
   the checker now binds it, so systems can `emit E { unit: self }` and
   `remove(self, Tag)`. (Was a half-shipped feature: runtime yes,
   checker no.)
6. **Type-only system params** — `system recompute(StatsDirty, ...)`
   matches on a tag component without binding a name nobody reads.

Validated as already-working by the same file: `on Event(e) when guard`
(content scripts live next to content data — ignite's burn is a guarded
handler, not a closure in a def), struct spread in stacking policies
(`BuffInstance { remaining: d, ..live }`), sum-type-valued struct field
defaults, `list<Struct>` component fields, and f-string format specs.

The dogfood file: `projects/dogfood/moba/buffcore.rad` — buff stacking policies
(Renew/Replace/Extend/StackUpTo) as a sum type matched exhaustively,
tenacity-scaled CC, periodic pulses, cleanse with dispellability, the
4-bucket stat fold, and dirty-flag recompute, in ~340 lines.

---

# Round 3: the MISSILEWORK fake port (combat motion)

Same method, next cluster: the attack cycle + projectile flight
(sim/systems/attack.ts 21KB + projectiles.ts 19KB) as
`projects/dogfood/moba/missilework.rad`. Chase/windup/fire/recover as a sum type
whose STATES CARRY THEIR DATA (Windup knows its ticks and crit roll —
no flag soup), homing missiles that fizzle when their target dies,
piercing skillshots, leash-break windup cancels.

Features the dream code forced:

1. **Tuple element-wise arithmetic** — the vector dialect.
   `pos + dir * speed * DT`, `there - here`, `-v`, scalar broadcast on
   `*` and `/`, per-element int/float promotion, arity mismatches loud.
   Killed the dx/dz-pairs-by-hand wart that runs through duel/treeline.
2. **Variant fields with type annotations** — `Homing { target: entity }`.
   Variant fields used to be declarable only via default-value witnesses;
   types with no sensible zero value couldn't appear in variants at all.
3. **Required component fields** — `source: entity` with no default:
   every construction site must provide it (entity literals included);
   spread bases satisfy it; resources reject it (they auto-initialize,
   so there is no construction site to demand it at).
4. **Guard-clause nil narrowing** — `if target == nil { return }` makes
   `target` an `entity` for the rest of the scope; the early exit IS the
   else branch. Covers return/break/continue and exhaustive if/else arms.
5. **Writeback survives `despawn(self)`** — a system holding `mut`
   params may dispose of the entity it is visiting (projectiles on
   arrival); the writeback then has nothing to write to, by design. Was
   a hard runtime error the moment a projectile landed.

The fizzle found itself: in the demo, the skillshot killed the dummy
while an auto-attack missile was airborne — and the missile fizzled
organically, exactly the era rule the comment promised.

---

# Round 4: the SPELLWORK fake port (casting + CC)

Next cluster: the cast pipeline + crowd control gates
(sim/systems/casting.ts + cc.ts) as `projects/dogfood/moba/spellwork.rad`.
Ready -> Winding -> Channeling as a NATIVE state machine, cast
completion as a delayed event, validation as Result values, the cancel
gate as a transition.

Features the dream code forced:

1. **`emit E { .. } after N`** — delayed events. The timer lives in the
   event queue (one tick per flush, firing in emit order), not in a
   hand-rolled countdown field. Cast points, channel ends, and every
   era-engine timer are this shape. Stale timers are harmless: the
   handler guards on current state (the interrupt race in spellwork).
   Payloads are GC roots; parallel-batch workers reject it loudly (v1).
2. **`get_or(coll, key, default)`** — map/list lookup with a fallback;
   the cooldown-table read (`c.cooldowns |> get_or(slot, 0)`).
3. **`clamp(x, lo, hi)`** — the CDR formula
   (`cd * (1.0 - clamp(cdr, 0.0, 0.4))`), int-preserving.
4. **Result/Option exempt from the public-API-leak check** — a pub fn
   returning `Result<str, str>` leaks nothing; they're language
   vocabulary, not private types.

Validated as already-working: NATIVE `state` machines as per-entity
component fields — `flow: CastFlow = CastFlow::Ready` with
`transition(c.flow, "begin") |> unwrap` worked out of the box, illegal
transitions arriving as Err values; map-field keyed updates composing
with transitions in one update block; `entries()` destructuring with
`where` for cooldown ticking.

Noted polish, since resolved: `for (k, v) in map` two-binding iteration
works (pinned in `for_tuple_destructure.rad`, alongside round 14's
n-ary tuple destructure over lists). Still open by design:
`emit ... after` inside PARALLEL system batches is rejected loudly
(the delayed queue is main-VM state; parallel batches would race it).

---

# Round 5: the SHOPWORK fake port (economy)

Next cluster: the shop economy (ui/hud/shop/model.ts + content/items)
as `projects/dogfood/moba/shopwork.rad`. The catalog as a `pub let` CONSTANT map
of structs (not a per-call table-builder), era-accurate purchases (pay
total minus owned components, consume them — duplicate recipe refs each
eat one copy), recursive combine-cost over the build tree, 70% sell-back,
and the showcase: **purchases as transactions** — `fork()` before acting,
`commit()` to undo, the undo stack being a plain list of world forks.
No inverse-operation bookkeeping exists anywhere in the file.

Features the dream code forced:

1. **`index_of(xs, v)`** — first index or -1. Returns an int (not an
   Option) because the consumer is slot arithmetic: `if at >= 0 {
   set_at(slots, at, nil) }` and `-1` composes with it.

Validated as already-working: `pub let` content tables (maps of structs
indexed by id — the pattern buffcore/spellwork's `defs()` functions
should migrate to), fork values stored in plain lists, multi-level undo
(each commit restores a whole world), nil-slot inventories with
`list<any>` annotations.

One value-semantics lesson worth pinning: `pop(xs)` is PURE — it reads
the last element and leaves the list untouched. The undo idiom is
`commit(pop(undo))` then `undo = drop_last(undo)`. The conformance test
caught the aliasing assumption immediately.

---

# Round 6: the SIGHTWORK fake port (fog of war)

The biggest unclaimed sim file: vision.ts (39KB) as
`projects/dogfood/moba/sightwork.rad` — the RULES at unit granularity (the grid/
raycast model is that file's perf representation). Per-team visibility,
the CanSeeCallback stealth rule (own team always sees; enemies only
where true sight covers), the brush rule (hidden from enemies whose
sight doesn't come from inside the same grass clump), and fog ghosts
(last-seen positions in an entity-keyed map, cleared on re-sight).

Features the dream code forced:

1. **`any(xs, pred)` / `all(xs, pred)`** — short-circuiting predicate
   sweeps; `sources |> any(fn(s) { return covers(s, target) })` IS the
   vision rule. Vacuous truth on empties.
2. **Readonly closures in pipelines** — literal closures only got the
   PURE allowance while named fns got pure-or-readonly; now both ride
   the same rule, so `map(fn(t) { return name_of(t) })` works.
   (`name_of` itself was also missing from the readonly/effect lists.)
3. **VM bug: system writeback through capture cells.** A closure in a
   system body that captures the `mut` param (sightwork's
   `query { Unit } where Unit.team != tv.team`) promotes the param slot
   to a capture cell; the writeback choked on it ("expected component,
   got TeamVision" — type_name reads through cells, into_component
   didn't). Now it unwraps the cell, like IncLocal always did.

Validated as already-working: query `where` clauses referencing
enclosing locals (and the mut param itself), entity-keyed maps with
tuple values (`ghosts: map<entity, (float, float)>`), `remove_key`,
and the whole brush/stealth/ghost choreography in ~180 lines.

---

# Round 7: the WAREWORK fake port (waves + aggro)

The wave director + the call-for-help ladder (barracks.ts +
callForHelp.ts) as `projects/dogfood/moba/warework.rad`. Waves are delayed-event
chains re-arming themselves; the cannon rides the cadence; and aggro is
the era's priority table — where the dream insisted a ranking IS a key.

Features the dream code forced:

1. **Tuple ordering** — lexicographic comparison shared by
   min_by/max_by/sort_by, so multi-key ranking is a tuple key:
   `min_by(fn(t) { return (-rung, dist) })`. Recursive (nested tuples),
   numeric promotion per slot, arity mismatch is a loud error.
2. **Readonly effect inference for unannotated fns** — bodies that only
   READ the world (require/has/queries) infer the readonly effect and
   compose into pipelines without a `readonly fn` annotation, the same
   allowance closures got in round 6.
3. **Purity-claim repairs** — `require`/`require_all` sigs falsely
   claimed pure (so fns calling them inferred PURE, and only a parallel
   gap in the effect tables hid the contradiction); `get_entity` had no
   sig at all (same effect). All world-readers now agree across the
   three classification layers, and `require`/`get_entity`/`require_all`
   joined the readonly + effect lists.
4. **HOF callback params no longer demand type-level purity** —
   `min_by(fn ...)` declared `pure fn` parameter types, rejecting
   readonly callbacks at unification before the (correct) pipeline
   rules could allow them. The pipeline/effect layers own that
   discipline now.

The turret-aggro moment proves the table: a red champion strikes a blue
champion under the turret, and the turret — whose nearest threat is a
red minion — switches to the diver: champ-on-champ outranks nearest
through the same scan, different table. Every blue unit converges
(`diver hunted by: 21`).

---

# Round 8: the ORDERWORK fake port (the order pipeline)

The biggest sim file: commands.ts (53KB) as `projects/dogfood/moba/orderwork.rad`.
Orders are a sum type, the per-unit queue is a list of them (plain
replaces, shift appends), the hard-CC gate DISCARDS incoming orders, and
exotic CC FORCES the effective order without touching the queue — taunt
attacks the taunter, then the saved queue resumes where it was. The
effective order is a match EXPRESSION over the CC state; execution
matches the head with `when` guards (arrived move / dead target →
advance).

Features the dream code forced:

1. **Expression arms in match** — `pat => expr` is sugar for
   `pat => { expr }`, in both statement and expression matches, composing
   with `when` guards. Guard-chains replace if/else ladders:
   `Cc::Free {} when len(q) == 0 => Order::Hold {}` then
   `Cc::Free {} => q[0]`.
2. **VM bug: match EXPRESSIONS never matched variant patterns.** The
   expression compiler emitted qualified pattern names ("Cc::Free")
   where Op::MatchState compares bare variant names ("Free") — every
   variant arm fell through and the whole match yielded nil, SILENTLY.
   Undetected until now because the only MatchExpr producer was the
   let-else lowering, whose Some/Ok paths are single-segment (join ==
   last). Found because orderwork's units stood perfectly still.
3. **`drop_first(xs)`** — the queue-advance dual of drop_last.

Validated as already-working: sum-type values stored in component list
fields (the queue round-trips through the ECS and matches), `when`/`if`
arm guards, and the era's full CC-gate choreography in ~190 lines.

---

# Round 9: the EZREAL KIT fake port (multi-module composition)

The structural question every real port lives on: can a champion kit be
a thin CONTENT file over shared system libraries? `projects/dogfood/moba/kit/`:
core.rad (vocabulary: Pos/Vitals/Damage event/vector helpers),
surge.rad (the stacking-buff slice), missiles.rad (skillshots with
pierce, affect flags, per-hit index), and ezreal.rad — the kit: era
numbers as `pub let` tables, casts as functions, behavior as guarded
handlers on the missile layer's events. Q's signature cooldown refund,
W's ally/enemy branch, R's per-victim falloff
(`max(pow(0.8, hit_index), 0.3)`), Rising Spell Force on every hit.

**Zero language gaps.** First fake-port round where the dream code ran
without a single missing feature: diamond imports dedup, pub components
construct cross-module in entity literals, pub systems schedule from the
importer, pub events emitted in one module dispatch handlers declared in
another, pub let tables and pure helpers flow through — the
content-over-libraries architecture holds. (The only fix the run forced
was a GAME bug: skillshots needed the ini's AffectFriends flag, or Q
struck poor sona.) Pinned as a three-module conformance trio
(modules/kit_core, kit_systems, kit_app).

Eight earlier rounds bought this one: shifts, indexed updates, res(),
tuple math, required fields, delayed events, tuple ordering, readonly
inference, expression arms — the kit file uses nearly all of them and
reads like the content sheet it is.

---

# Round 10: the RECAPWORK fake port (death recap + kill economy)

game/deathRecap + sim/progression.ts as `projects/dogfood/moba/recapwork.rad`.
The recap is a QUERY OVER THE PAST — window the victim's damage history,
group per source, rank, mark KILLER + two ASSISTs — and the dream
insisted the event system itself should remember.

Features the dream code forced:

1. **`recent_events(name, window)`** — every dispatched event lands in
   the deterministic event log (main timeline, ring-capped at 4096);
   the builtin returns the payloads from the last `window` ticks,
   oldest first. Death recaps, combat windows, and "what hit me" panels
   stop hand-rolling ring buffers:
   `recent_events("Damage", 450) |> filter(...) |> group_by(...)`.
   Bonus: the RADSCOPE event panel read this log already — it had just
   never been populated on the native VM.
2. **`_` discards in tuple destructuring** — `let (_, total, _) = row`
   was "duplicate binding"; underscores are discards everywhere now
   (the for-loop and closure forms already knew).

The demo plays the era economy beat for beat: a 4-spree shutdown prices
at 300 + 4×100 + first-blood 100 = 800g, the recap ranks annie's 550
over brand's 300 over the tower's 190, and the next kill mints a plain
300. Window math, grouping, ranking — three pipeline lines.

---

# Round 11: the MATCHWORK fake port (the match frame)

The outermost loop: structureGating.ts + endOfGame.ts + respawn.ts +
fountain.ts as `projects/dogfood/moba/matchwork.rad`. The backdoor-protection
chain is DATA — each structure names its protectors, invulnerability is
DERIVED (`protected_by |> any(alive)`) instead of expose/shield calls
threaded through death handlers. The nexus falling flips a Match
resource the tick loop reads; champion deaths schedule level-scaled
respawns as delayed events; the fountain is a heal-zone system.

**Zero language gaps — the second saturation round.** Everything held:
`phase frame { ... }` groups (barely dogfooded before), system `after`
ordering (pinned in conformance with a deliberately scrambled phase),
the derived-DAG idiom over `any` + `get_entity`, `emit ... after` with
computed delays, guarded `Felled` handlers splitting structure/champion/
nexus consequences. The demo plays the whole skeleton: a backdoor
attempt shrugged off, the legal siege order, a level-6 death clocking
exactly 450 ticks, a fountain respawn at full vitals, and the match
halting the moment the nexus falls.

Two consecutive gap-free rounds with phases, ordering, and the full
event vocabulary in play: the dream-code method has saturated the sim
domain. What remains for a REAL port is scale (content volume, perf
gates, the compiled backends) — not language shape.

---

# Round 12: the NAVWORK fake port (pathfinding)

After two gap-free sim rounds, the pressure moved to the game's most
ALGORITHMIC corner: the navgrid (nav/navmesh.ts + game/navPull.ts + the
original's TestNavGrid). `projects/dogfood/moba/navwork.rad` is grid A* with
string pulling and the RE'd right-click nav-pull — and the data-
structure domain delivered three real gaps where the ECS rounds had
none:

1. **If-expressions** — `let aim = if on_footprint { player - click }
   else { unit - click }`. The dream reached for the expression form in
   four places. Now: `if cond { a } else { b }` with mandatory `else`,
   single-expression branches, `else if` chains, branch type
   unification (`assignable_from` both ways, same as match arms), and
   purity/readonly/sim-breach walkers all extended. Diagnoses non-bool
   conditions, missing else, and mismatched branches.

2. **Tuple map keys** — A* wants `best[(x, y)] = g`. `MapKey::Tuple`
   joined the key enum: hashes by value, sorts lexicographically after
   scalar keys (deterministic iteration preserved), floats banned
   recursively. Wire + replay codecs got recursive `["t", [...]]` key
   encoding (canonical, byte-identical re-encode), `json_stringify`
   renders tuple keys in display form.

3. **`sort()` couldn't order tuples** — sort_by/min_by/max_by gained
   lexicographic tuples in round 7, but plain `sort()` kept a private
   comparator. Its fallback now delegates to the same `compare_values`
   total order: one ordering everywhere.

Bonus formatter bug, found while gating: unary minus formatted as
`- 1` (the token-stream normalizer treated every Minus as binary).
Now classified by the preceding significant token; `3 - -2` keeps the
binary spaced and the unary attached.

The demo: 26-cell path around a wall, string-pulled to 4 waypoints, a
sealed map returns the empty path, an on-footprint nav pull resolves
one cell toward the clicker, and the era's provable no-op (budget under
one cell) returns the click unchanged — semantics pinned straight from
the 0x7df10c reverse engineering notes.

---

# Round 13: the SPATIALWORK fake port (the bucket grid)

sim/spatial.ts — the proximity index behind every "who is near X"
question — as `projects/dogfood/moba/spatialwork.rad`: tuple-keyed buckets,
max-radius query inflation, floor-divided cells (negative coords!),
canonical ascending-eid order. The data-structure domain delivered
again — five features/fixes and TWO real VM bugs:

1. **`id_of(entity) -> int`** — pure (the id is in the value), plus
   **entity ordering in `compare_values`**: `query { C } |> sort` is
   now the canonical determinism idiom (ascending eid, the same
   contract the JS grid documents).

2. **Tuple±scalar broadcast completed** — round 3 wired the VM for all
   four ops but the CHECKER only allowed `*`/`/`. `center - reach`
   (inflate a point on every axis) now types; scalar-left stays
   commutative-only (`1.0 - p` is still rejected).

3. **`group_by` got real keys** — the key fn was hard-typed `-> str`
   and the runtime stringified every key through `print_display`. Now
   the signature is generic (`map<K, list<T>>` keyed by what the key fn
   returns), tuple/int/entity keys stay themselves, and invalid key
   types (float, nil) error instead of silently becoming strings.

4. **Bucket-fill append** — `buckets[(cx, cz)] << eid` auto-vivifies a
   missing key with `[]` (the `m[k] << v` desugar seeds `get_or(m, k,
   [])`). The defaultdict idiom, scoped to the append operator only.

5. **VM bug: fused pipelines corrupted the stack in expression
   position.** Both fused paths parked their accumulators in LOCAL
   slots — frame-relative indices that are only correct when the
   operand stack is empty. Inside an f-string part or a call argument,
   the slots aliased live operand values (symptom: `VecBroadcast:
   expected list template`, or garbage results). The vectorized path
   now uses a GLOBAL scratch slot (same pattern as match-expression
   results) and stays enabled everywhere; the scalar loop fallback is
   gated to statement roots (let/assign/return/expr-stmt) via a
   one-shot `allow_pipe_fusion` grant, falling back to plain unfused
   calls in expression position.

6. **Bonus checker bug**: components used ONLY in entity literals were
   flagged "unused" — the reachability walker had no `EntityLiteral`
   arm. Fixed.

The demo: 4 buckets with a fat-bodied straddler caught only via
max-radius inflation, darius at (-0.5, -0.5) folding to cell (-1, -1)
(floor, not truncate), bit-stable sweeps across rebuilds, and a
group_by census keyed by cell tuples.

---

# Round 14: the REPLAYWORK fake port (replay/spectate)

The era's replay file is a COMMAND LOG: deterministic sim + timestamped
orders, nothing else. `projects/dogfood/moba/replaywork.rad` builds the whole
genre on rad's native machinery — checkpoints are `fork()`s, seek is
`commit(nearest)` + replay the remainder, what-if is the same pipeline
pointed forward. Two features harvested:

1. **For-loop tuple destructuring with parens** — `for (due, who, x, z)
   in tape` was capped at 2 bindings (map k/v). Parenthesized bindings
   over a LIST now destructure tuples of any arity (checker types each
   element; compiler normalizes to the existing bracket-destructure
   lowering). `where` filters and `_` discards compose; map iteration
   and query unpacking are untouched.

2. **`transient resource`** — the round's real find. The first replay
   run came back NOT bit-exact, and the diagnosis was architectural:
   the tape itself was a resource, so RECORDING the match changed the
   match's digest. The tape is metadata, not simulation state — the
   same words spatial.ts uses about its grid ("derived data, never
   enters the snapshot payload"). Two rounds wanting one concept:
   `transient resource Tape { … }` is excluded from `world_digest()`
   and `save_world()` (live, fork-digest, and delta-lineage paths all
   honor it), while `fork()`/`commit()` still carry its values. With
   the tape transient, the dream's headline assertion holds: bit-exact
   at every checkpoint, replay digest == live digest.

The demo: a 90-tick match with mid-game orders, 4 checkpoints; full
replay from t0 bit-exact at every checkpoint; scrub to t=75 from the
t=60 fork (digest-verified on arrival); a what-if rewind where kat
dodges instead — timeline provably diverged. Replay, spectate, and
what-if in ~130 lines, no replay "system" anywhere: the architecture
IS the feature set.

---

# Round 15: the SCENEWORK fake port (the scenario system)

game/scenarios as `projects/dogfood/moba/scenework.rad`: scenarios are DATA
(id, describe, staging closure, tick budget, verdict closure), booted
through the same seams as a real match, each one running in a FORK so
isolation is the type system instead of a convention. The harness is
~15 lines; the scenario table is the product.

The harness machinery ran first try — closures as data in tuples,
5-element for-loop destructure (round 14, immediately load-bearing),
`peek`/`assert`/`unwrap` on result forks, commit-to-blank between
scenarios. Then the harvest came in two layers:

1. **The mirror scenario caught a real dream-code bug**: damage applied
   mid-query-iteration let whoever iterates first deny the opponent's
   swing. The fix is the same one the original made: swings are
   EVENTS, applied after the sweep. The test harness round caught a
   combat bug — the methodology validating itself.

2. **Events in `simulate()` — the round's headline.** The fixed combat
   system was rejected: "cannot be used in simulate(): emits an event".
   But event-driven damage is rad's own recommended shape; a simulate()
   that can't run the event loop can't simulate the game — fatal for
   the train-the-MOBA-in-forks story. The VM already did the right
   thing (fork events restored, per-tick flush, leftovers packed into
   the result fork); the ban was a stale CHECKER rule. Lifted properly:
   `emit` is legal in simulated systems, and the safety obligation
   moves to a TRANSITIVE handler walk — every handler reachable through
   the system's emits (including handlers emitting further events) is
   checked; IO anywhere in the chain is rejected with the handler
   named ("handler `on Pong` calls IO builtin 'print'"). One real VM
   leak found while lifting: simulate didn't isolate the DELAYED event
   queue, so sim ticks aged live `emit … after` timers and sim-scheduled
   delays leaked into the live queue. Both now swap with the rest.

The demo: duel/standoff/mirror scenarios — 3 passed, 0 failed, with
`assert(failed == 0)` sealing the suite. Conformance pins the fork
event-loop semantics: a 3-tick sim accumulates handler effects plus an
in-fork cascade (13 hits), the live world reads 0, and live delayed
delivery still works after the sim ran.

---

# The hardening pass: three corners, un-cut

A self-audit of this list found three places where the fix shipped
expedient instead of right. All three are now solved properly:

1. **Delayed timers are snapshot state.** `WorldSnapshot` carried the
   in-flight event queue ("a snapshot that drops them is not a
   snapshot") but `emit … after N` timers lived outside it: commit()
   kept the ABANDONED timeline's timers ticking and lost the target's;
   simulate() dropped sim-scheduled timers at sim end (round 15 had
   documented that as "die with the sim" — a cop-out); pooled
   simulate_par workers carried timers between unrelated calls. Now
   snapshots carry `delayed`, restore_events_from()/commit()/simulate()
   move timers with the timeline, the wire format ships an optional
   `"delayed"` section (old tapes decode as empty), and merge_forks
   merges timers two-sided (timers age, so the events prefix rule can't
   apply; both-changed-differently is an honest conflict). Pinned in
   `tests/conformance/delayed_events_snapshot.rad`: a timer crosses
   fork(), re-fires exactly once after commit(), an abandoned timeline's
   timer never fires, and a sim-scheduled timer rides the result fork.

2. **`get_entity` is honestly typed.** Round 7 had parked it at `Any`
   ("side-stepping migration to entity | nil for now"), which switched
   off type checking downstream of every name lookup. It now returns
   `entity | nil` — and the checker immediately flagged six dogfood
   files' unguarded lookups, proving the point. The migration added the
   missing vocabulary word: `require_entity(name)` — the fail-fast dual
   of `get_entity`, the same get/require pairing components have.
   Known-good lookups read better than before; the one genuinely
   fallible site (matchwork's protector scan) keeps `get_entity` + the
   round-3 guard narrowing. Goldens stayed bit-identical through the
   sweep.

3. **The simulate handler walk resolves names.** Round 15's transitive
   handler check keyed handlers by raw AST event names — an emit
   through a module alias (`emit m.Event`) would have missed `on Event`
   in the defining module, letting an IO handler slip into a simulation
   unvetted. The registry now indexes raw names AND last segments, and
   lookups try raw, canonical, and last-segment spellings: over-matching
   is conservative (more handler bodies vetted), under-matching was the
   unsound direction.

Reviewed and deliberately NOT changed: the scalar pipeline fallback
stays statement-root-only (correct, just slower in expression position
— the vectorized path covers the hot shapes), and group_by's stricter
key errors (float/nil used to stringify silently) are the intended
semantics.

---

# Round 16: the CAPWORK fake port (capture points)

The biggest unclaimed gameplay file: capturePoints.ts (~20KB, the
Dominion objective system) as `projects/dogfood/moba/capwork.rad`. Progress is
ONE SIGNED NUMBER, west-positive — every era rule is arithmetic on it
(exclusive capture with diminishing stacking, owner regen to the pole,
neutral decay to zero, contested freeze) and the two-stage flip
(neutralize crossing 0, capture past ±99) is four guarded branches.
The verification IS round 15's scenario harness — two dream rounds
composing: six staged forks, simulated, judged by verdict closures.

Features the dream code forced:

1. **`sign(x)`** — the missing third of the abs/clamp family
   (Math.sign semantics: int-preserving, 0 and NaN map to 0). The
   neutral-decay formula is `-sign(p) * min(abs(p), RATE)`, exactly
   the original's.

2. **`peek_resource(fork, R)`** — the resource dual of `peek`: read a
   fork's resource without committing it. Score-keeping in simulations
   was unreadable before this (committing to inspect destroys the live
   world). The score-drain scenario reads the simulated Score resource
   straight off the result fork.

3. **Order-independent effect inference — the round's real find.** The
   evaluate system called `champs_on`, which calls `dist` — declared
   LATER in the file. Effect inference ran in declaration order, so
   the unclassifiable forward call degraded `champs_on` to
   unrestricted, and simulate() rejected the system with the absurd
   "calls 'champs_on' which has IO effects". Now a monotone fixpoint
   re-infers purity/effects with the COMPLETE function table (effects
   only narrow: pure ⊂ readonly ⊂ unrestricted, so it terminates).
   Where your helper sits in the file no longer changes what the
   checker believes about it.

The demo: 6 scenarios, 6 passes — solo capture flips at exactly beat
117 (99/0.85, the era's pacing), contests freeze progress to the bit,
a 2-champion dive neutralizes then captures through the two-stage
machine, regen drifts at exactly 30×0.14, stealth captures nothing,
and a held point drains 25 VP in 25 beats while the holder's stays
untouched.

---

# Round 17: the ROSTERWORK fake port (kits at scale)

Round 9 proved ONE champion can be a content file over shared
libraries; this round proves N. `kit/annie.rad` (the targeted-caster
shape: Disintegrate's kill refund, Incinerate's cone, Pyromania's
4th-cast stun) and `kit/trynd.rad` (the melee-stat shape: Bloodlust
rage arithmetic, Undying Rage) joined ezreal over the same core, and
`kit/roster.rad` runs all three through one schedule with asserts on
every signature beat AND on cross-kit isolation.

**Zero language gaps — the third saturation round, this time at
scale.** The harvest is the ARCHITECTURE LESSONS the existing checker
enforced unprompted:

1. **Privacy caught cross-kit reach-through.** The roster tried to
   construct ezreal's private `Kit` component; "Component 'Kit' is
   private" forced the right pattern — kits export their own
   initializers (`ezreal_setup`/`ezreal_cooldown`), internals stay
   theirs.

2. **Slot strings must be namespaced at N>1.** Ezreal's `on
   SkillshotHit when e.slot == "q"` would fire for ANY kit's q. Slots
   are now kit-qualified (`"ez.q"`), and the unguarded RSF handler
   (every missile stacks ezreal's passive — even annie's) gained a
   kit-marker guard. The bleed the roster asserts against was real:
   both fixes were forced by writing kit #2.

3. **Kills are library vocabulary.** Annie's mana refund cannot be
   decided at the cast site (death happens when the Damage EVENT
   applies, one flush later) — core.rad grew `Felled` and the refund
   became an on-kill listener, the era's BBOnKill shape. Same pass
   grew the `HpFloor` seam (the original's SetMinimumHealth), which
   makes trynd's entire ultimate a two-line attach/detach.

The trace is era-perfect: pyro charges across W+Q+Q, the 4th cast
stuns, the killing Q refunds 60 mana through the event cascade;
ezreal's RSF stays at 0 through annie's casts and rises on his own;
trynd survives a 900-damage nuke at exactly 1 hp and dies to the same
nuke after the five seconds expire.

---

# Round 18: the GROWWORK fake port (XP + leveling)

The last unclaimed progression half: ExpCurve.ini + Mission.ini
[Experience] as `projects/dogfood/moba/growwork.rad`. The curve is a ladder,
the death sweep splits by the SplitXP table (solo lanes 92%, duo
lanes mint 110% split 55/55 — the patch-history-verified era math),
grants cascade through multiple rungs, and ranks gate by
ceil(level/2) with the ultimate at 6/11/16.

**Zero language gaps — the fourth saturation round.** Six self-
asserting scenarios (the round-15 harness shape again): solo-92,
duo-110, radius exclusion, a 1200-xp grant cascading to level 4 with
3 points, rank gates refusing the 4th Q point and accepting R at 6,
and the 18 cap. Two dream-code learnings worth the ink:

1. One-statement-per-line shaped the harness into a `check(ok, id,
   detail)` helper — and the result reads better than the semicolon
   one-liners it replaced.
2. The only failure the round produced was `100.0 * 0.55 ==
   55.00000000000001` — float equality in a TEST assertion. The sim
   itself dodges this by construction (the fixed-point arithmetic
   rule from round 1); the harness now compares with tolerance. The
   era engine's integer-math discipline, rediscovered from the
   outside.

---

# Round 19: the formatter round (repo-wide fmt convergence)

Not a fake port — the loop's own debt. `rad fmt` had a house style
nobody wrote (341 of ~360 files "dirty"), so it gated nothing. The
dream-code corpus is the style spec; the formatter was rewritten to
produce it:

1. **Token-lexed depth tracking** replaced char-counted braces: a
   render-level STACK with one entry per open bracket ({, (, [ — all
   three; parens were not tracked at all), every bracket opened on a
   line mapping to the same level+1. Multi-line closure arguments —
   `filter(fn(e) {` … `})` — indent one step and close back exactly
   (they used to land at column 0), and braces inside strings or
   comments no longer corrupt the depth.
2. **4-space indent** (the corpus unit; it was 2).
3. **Trailing-comment alignment is the author's**: the whitespace run
   before `//` is kept verbatim instead of collapsed — with the
   one-space-per-pass drift bug fixed (operator rules emit a space,
   the verbatim run replaces it, idempotent).
4. **Invisible-byte honesty**: CRLF files format as CRLF (the bulk of
   the 341 was line endings alone), and a UTF-8 BOM survives.

Then the apply: 251 files reformatted in one sweep, `fmt --check`
now ZERO dirty across tests/, projects/dogfood/, examples/, and fully
idempotent. Proof of zero behavioral drift: all 241 conformance
snapshots pass (one was regenerated — a runtime-error LINE NUMBER
shifted because the blank-line-before-decl rule moved a statement
down one line; same error, same exit code), damage_core's golden is
bit-identical, and every dream file reproduces its final line.
`fmt --check` is now a meaningful gate for the first time.

---

# Round 20: the LEGION fake port (content volume)

The scale question round 17 left open: does the toolchain survive the
REAL roster? `tools/gen_legion.py` reads all 84 champions' stats.ts
(machine-generated from the era .ini files — perfectly regular) and
emits one rad content module per champion plus an index importing
them all; `legion.rad` musters the whole roster and brawls it.

**The toolchain holds.** An 85-module import graph (84 generated
modules diamond-importing one shared core, plus the index) lexes,
parses, checks, and compiles in ~0.5 s. Subdirectory imports
(`use "gen/legion_index.rad"`, untested before this round) resolve
relative to the importing file. The generator emits fmt-clean code
(round 19's gate covers generated content too). The era stats survive
extraction bit-exactly — annie's level-18 hp is 424 + 76×17 on the
field. Both brawl shapes flow from one tuple-driven spawn path, and
the only checker complaints were the dream code's own: `min_by`
returning Option (honest — empty list), and a tuple binding named
`range` shadowing the builtin.

**The volume finding is PERF, not language:** the idiomatic per-entity
brawl (full query + filter closure + min_by closure, per champion,
per tick) costs ~8.4 ms/tick at n=84 on the release VM — O(n²)
closure dispatch, exactly the shape spatialwork's bucket grid exists
to break. The language carried 84 modules without blinking; the next
ceiling is closure-heavy queries in hot loops, which is the compiled
backend's job (and the round-13 idiom's, until then).

The demo: 84 champions mustered through their own modules, level
curves verified against the .ini numbers, a 600-tick all-out brawl
(78 fallen, 6 standing, deterministic digest), 6/6 assertions.

---

# Round 21: the compiled backend + the perf envelope

The bare-metal arc, opened on legion. One source of truth — the 84
extracted stat rows — now feeds TWO backends: gen_legion.py emits the
rad modules AND `arena/src/gen_legion.rs`; `legion_trace.rad` is the
SPEC (six checkpoint lines + 84 per-champion endgame lines + END, all
floats at 6 decimals, `projects/dogfood/moba/golden_legion.txt`); and
`arena legion` (arena/src/legion.rs) is the compiled twin.

**Bit-identical on the first run — in full float math.** Treeline
proved the integer path; this proves f64: with the VM's semantics
mirrored exactly (eid-order sweep with LIVE position writes, FIRST
strict-minimum target ties, hits queued during the sweep and applied
in emit order at the flush, expression-for-expression float order),
all 91 trace lines match with zero tolerance. The VM's determinism
contract holds across the compile boundary. `arena golden-legion`
verifies it; `arena golden-tl` still passes beside it.

**The perf envelope, measured on the same machine, same algorithm,
same results:**

| backend                    | per tick (84 champs, O(n²)) | ticks/s |
|----------------------------|------------------------------|---------|
| rad VM (release, fat LTO)  | ~8.4 ms                      | ~119    |
| Rust (single thread)       | ~1.9 µs                      | ~525,000|

~4,400× single-threaded, before rayon or the GPU. A 600-tick
full-roster episode costs 1.1 ms compiled — ~875 episodes/s on one
core. THAT is the training-loop arithmetic the 10-minutes-on-a-3080Ti
goal rests on, now with a conformance gate proving the speed costs
zero correctness: the dream code in rad is the spec, the compiled
backend is the engine, and `golden_legion.txt` is the contract
between them.

---

# Round 22: the PPO loop — a curve that goes up

The compiled arena pointed at training, done honestly after a flatline
post-mortem. The first 10-minute run was a null result with a clean
diagnosis: the only dense reward was AMBIENT GOLD (a constant both
sides receive — zero gradient), the win bonus lived 1,800 decisions
past the actions that earned it, the GPU sat at 9% behind a
synchronous python step loop, and the eval (argmax vs the competent
scripted laner) read 0% at every skill level below "finished".

The v3 design, four layers:
1. **Throughput** — frame-skip 6 (per-instance, via tl_configure),
   4096 envs, inference_mode: ~300k env-steps/s end-to-end (2.3× v1),
   181M steps in the 10 minutes.
2. **Signal** — relative gold (mine minus theirs: the shared constant
   cancels, only the LEAD carries gradient) + champ damage diff;
   stage 0 runs 60-second scrims so the CS/gold verdict lands inside
   the credit horizon (gamma 0.99 to match); stage 1 widens to
   3-minute games at 35% of the budget. Curriculum is episode
   length/start state only — never the rules.
3. **League-lite** — half the envs mirror self-play, half vs a frozen
   checkpoint pool (refreshed every 2 min); frozen-side agents are
   masked out of the PPO batch.
4. **Meters that move** — CS/episode, gold lead, end-reason histogram
   (exported per-env through the FFI now; the old why-buckets binned
   everything into one slot), winrate vs RANDOM as the sanity gate,
   winrate vs the scripted bot last.

The 10-minute result, RTX 3080 Ti, single run:
- **CS/episode 0.015 → ~3.0** — last-hitting learned from scratch
  (the precise-timing skill random exploration almost never performs).
- **Winrate vs random: 0 → 100% by minute 2**, ≥75% thereafter.
- **Winrate vs the handcrafted laner: 0.4% → 65%** in 60-second
  scrims by minute 3; 30–43% in full 3-minute games.
- **Emergent phase shift in the end-reason histogram**: first bloods
  spike when aggression is discovered (909 in one iteration at the
  stage switch), then fall as both sides learn not to die.

A reward curve, a skill curve, and a behavior shift, in ten minutes,
on the conformance-gated compiled sim whose every rule is pinned to
the rad spec. The 10-minutes-on-a-3080Ti sentence is no longer a
goal; it is a measurement (`arena/runs/treeline_v3/curve.csv`).

---

# Round 23: the bot walks into the game

The OpenAI-Five moment, miniature: the policy trained in round 22
now plays the REAL browser game, against a human, with the net
running IN THE BROWSER. `?scenario=vsbot` boots a live match — you
are Tryndamere, the bot ("TreelineNet") is Ezreal, the champion whose
kit it learned — through the same scenario seams every debug match
uses.

The bridge (browser-moba/src/game/scenarios/debug/vsbot/):
- `export_onnx.py` ships the 1.4 MB MLP as ONNX; onnxruntime-web
  runs it client-side (~0 ms per decision at 5 Hz). No server.
- `encoder.ts` ports the arena's RL surface to live bitecs state:
  the same 134 egocentric features (era constants from
  gen_content.rs, milli→meters), the same (dist², kind, id) slot
  order, the same rules-only legality mask, the same per-mille
  direction tables for action decoding.
- The driver speaks the game's own Command seam — the scenario
  context grew a `commands` handle (the queue is THE determinism
  seam, so bot commands are replay-recorded like any seat's) — and
  auto-ranks along the arena's fixed skill order.

Verified live in the browser: scoreboard seats TreelineNet/Ezreal,
the model serves (HTTP 200), and the brain probe reads
`{tick: 774, action: 8, kind: "move", slots: 7}` — the policy
encoding the live world, masking, and issuing commands at 5 Hz.

Honesty clause, in the code too: the live game is richer than the
training sim. Features without cheap live equivalents (swing timer,
windup kind, slow timer, damage tallies) encode as 0 — the bot is
slightly blind there, and full-game macro (items, recalls) was never
in its action space. It does what 10 minutes taught it: lane, farm,
poke. The honest path to stronger is more training minutes on the
conformance-gated arena — the pipeline from curve to playable
opponent is now one export script long.

**The contract, widened to the full treeline systems.** The original
golden-tl compared champion summaries plus AGGREGATES (minion count,
hp sum, tower hp sum) — and aggregates can hide compensating drift
(one minion +5 hp, another −5 passes). treeline.rad's trace grew a
`deep_dump`: one line per live minion (kind/team/lane/waypoint/pos/
hp/cooldown/windup/target), per tower slot (hp/front/cooldown/
windup/target/grudge), per active missile (pos/direction/flight-left/
source/rank/pierce-history), at all 21 checkpoints; arena's TlArena
mirrors it field for field. The fixture went from 22 lines to 644 —
and the Rust twin matched ALL of them on the first run. Waves,
tower aggro/grudges, last-hit gold, xp, minion upgrades, missile
flight: every subsystem's per-unit state is now bit-pinned across
the backend boundary, for the integer sim (treeline) and the float
sim (legion) both.
