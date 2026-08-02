# RADGUI — the ECS-native GUI framework

The ECS world IS the scene graph. A widget is an entity carrying a
`Widget` component; interactions are rad events; the browser renderer is
**one generic file** ([projects/playground/radgui.js](../../../projects/playground/radgui.js))
that knows nothing about any app — it draws whatever widget entities exist
and pushes DOM events back into the session. Because UI state is world
state, every rad superpower applies to GUIs automatically — and none of
them required app code:

| feature | how | app code required |
|---|---|---|
| undo/redo | fork() per interaction, Ctrl+Z = commit() | `ui_enable_undo()` |
| inspect mode | alt-click any widget -> `why()` overlay with the causal chain | none |
| speculative hover | hovering a button simulates the click in a fork and ghosts the diff | `ui_enable_preview()` |
| time-travel debugging | a traced second runtime, scrubbed frame by frame | RADSCOPE does it |
| session bug tapes | record/replay (native) and session deltas (wire) | existing rails |

## Run it

```
py -m http.server 8137          # repo root
# counter:  http://localhost:8137/projects/playground/radgui.html?app=projects/dogfood/radgui/counter.rad
# todo:     http://localhost:8137/projects/playground/radgui.html?app=projects/dogfood/radgui/todo.rad
# pixels:   http://localhost:8137/projects/playground/radgui.html?app=projects/dogfood/radgui/pixels.rad
#           (open it in TWO tabs — same canvas, live; refresh — it persists;
#            add &relay=ws://<host>:8378 to go cross-machine via the relay)
# biglist:  http://localhost:8137/projects/playground/radgui.html?app=projects/dogfood/radgui/biglist.rad
# orbits:   http://localhost:8137/projects/playground/radgui.html?app=projects/dogfood/radgui/orbits_live.rad
# RADSCOPE: http://localhost:8137/projects/playground/radscope.html
```

Tests: `node --test projects/playground/test/session.test.mjs` (after
`wasm-pack build --target nodejs --out-dir ../../projects/playground/pkg-node core/vm`).

(Rebuild the runtime first if needed: `wasm-pack build --target web --out-dir ../../projects/playground/pkg core/vm`.)

## The pieces

- [lib_gui.rad](../../../projects/dogfood/radgui/lib_gui.rad) — widget schema (`Widget`, `Text`, `Style`,
  `Input`, `Hidden`), events (`Click`, `Changed`, `Submit`, `Key`, `Tick`),
  builders (`ui_root/ui_col/ui_row/ui_label/ui_pre/ui_button/ui_input/`
  `ui_slider`), mutators (`ui_set_text/ui_show/ui_clear/ui_remove`), and
  the `UiConfig` flags the renderer obeys.
- [projects/playground/radgui_worker.js](../../../projects/playground/radgui_worker.js) — the
  session loop (`session_emit` + `session_pump` + world snapshot per
  frame) plus a second, traced TARGET runtime for debugger-style apps.
- [projects/playground/radgui.js](../../../projects/playground/radgui.js) — keyed DOM
  reconciliation by entity name; input/keyboard wiring; undo, inspect,
  and preview drivers.
- [counter.rad](../../../projects/dogfood/radgui/counter.rad), [todo.rad](../../../projects/dogfood/radgui/todo.rad) — rungs 0 and 1.
- [radscope.rad](../../../projects/dogfood/radgui/radscope.rad) + [projects/playground/radscope.html](../../../projects/playground/radscope.html)
  — the flagship (below). Demo targets in [targets/](../../../projects/dogfood/radgui/targets/).

## RADSCOPE — the GUI framework debugging the language it's written in

A visual time-travel debugger for rad programs, itself a rad program
rendered by RADGUI. Pick a target; it runs in a second runtime with
`trace_timeline` on — one CoW world snapshot per frame boundary. Then:

- scrub the slider: the entity tree and component values re-render at
  that exact frame
- click an entity, click `why?` on any component: the causality ledger
  answers **as of the scrubbed frame** (`why_at`)

The bundled `whodunit` target is a mystery: the hero's 100 gold dwindles
across 12 frames with three suspect handlers. Scrub to frame 10, ask why:

```
Purse of hero = { gold: 45 }   (set in frame 9)
  <- by `on Heist` handler
  <- Heist { burglar: "moonlight-pete", amount: 30 } emitted in frame 8
  <- by top-level code
```

Case closed, with provenance, at any point in time.

## Browser-verified receipts (rung by rung)

- counter: click round-trip + Style recolor + arrow keys (3 clicks -> "3"
  green; 4 ArrowDown -> "-1" red)
- todo: typing -> Submit -> row spawn; toggle; per-row delete; footer
  recount; input clearing via world echo
- RADSCOPE: whodunit traced to 13 frames; scrub to 10 shows
  `Purse gold: 45`, `Mood feeling: robbed`; why-at-frame names the burglar
- undo: todo row deleted then resurrected by Ctrl+Z (4 -> 3 -> 4 rows);
  counter 2 -> 1
- preview: hovering `+1` at count 1 paints a `→ 2` ghost on the label,
  then the universe rolls back (the live session never saw it)
- inspect: alt-click the count label ->
  `Style of ui-1 { fg: "#5dff8f" ... } <- by 'on Click' handler <- Click { target: ui-5 }`

## What this app changed in the VM/wasm (the language-dogfood half)

1. `World::snapshot_json_like` now includes **resources** (renderers need
   `UiConfig`; updated the one snapshot-shape test).
2. `WorldSnapshot::snapshot_json_like` — frozen frames dump with the same
   shape as live worlds, so the debugger renders timelines with the same
   code path.
3. `VM.trace_timeline` — CoW world snapshot per main-timeline frame
   boundary (capped at 4096), captured in `flush_events` at the exact
   keyframe point record/replay uses.
4. wasm `RadRuntime`: `run_traced`, `timeline_len`, `timeline_world(i)`,
   `timeline_events`, `why_at(frame, entity, comp)` — RADSCOPE's whole
   backend; plus `session_checkpoint`/`session_undo` (undo ring),
   `session_why` (inspect), `session_preview` (fork-simulate-rollback,
   under `in_simulation_fork` so speculation never touches the ledger).

## Round 2 — the 4chan critique, answered

The framework got publicly roasted; every structural complaint became a
fix, each with a dogfood app that forces it:

| critique | fix | proof |
|---|---|---|
| "full-world JSON per keystroke" | `session_render_delta()` — CoW-diffed upserts/removes/changed-resources only; renderer keeps an id-keyed mirror | painting one pixel ships one entity row |
| "no checkbox/select/image/textarea" | all four, native semantics + ARIA labels | todo uses real checkboxes; RADSCOPE has a paste-source textarea; orbits has an SVG image with alt text |
| "Bounds was planned, never shipped" | `Bounds` + `panel` (absolute layer over flex) | [pixels.rad](../../../projects/dogfood/radgui/pixels.rad): 256 positioned cells; [orbits_live.rad](../../../projects/dogfood/radgui/orbits_live.rad): Tick-animated moons |
| "no hover, no drag" | opt-in `Hoverable`/`Draggable` markers -> `Hover`/`Drag` events with panel-relative px | pixels paints by drag stroke; hint label follows hover |
| "slider scrub fills the undo ring" | checkpoints only on Click/Submit/Key/Drag-start | one Ctrl+Z = one whole paint stroke (verified: 2-cell stroke undone at once) |
| "preview only diffs Text" | structural ghosts: `+n/-n widgets` badge on the hovered button | todo's add button shows spawn counts |
| "event log is a ghost feature" | RADSCOPE event panel, sourced from the causality ledger (same provenance `why()` walks) | whodunit shows `t8: Heist { burglar: "moonlight-pete", amount: 30 }` |
| "two hardcoded targets" | paste any rad program, trace it | pasted 5-frame Hit/Hp program: 6 frames, entity tree, scrubbable |
| "O(widgets²) next_order" | one monotonic counter for names AND order | build-time scans gone |
| "wasm size never measured" | 3.32 MB at opt-level 3 (was ~2.9 at opt-s; `wasm-size` profile exists if it matters) | measured |

Still open (honest): no collab rung (relay sits unused), inspect-mode
`why()` is live-session only, no JS test harness, drag move events drop
under backpressure (fine for painting, not for sliders-as-knobs).

## Round 3 — the leftovers, shipped

| critique | fix | proof |
|---|---|---|
| "ship rung 4, coward" (collab) | `ui_enable_collab()` — host election over BroadcastChannel, replica edits forwarded to the host, host pumps once and streams `session_delta` wire to every tab, late joiners adopt `session_state` | two sessions painted the same canvas: replica drew a red stroke, both worlds agree (7 red / 10 green each side) |
| "time-travel debugger can't EDIT time" | `run_traced_with_patch(src, frame, ent, comp, field, value)` — the patch lands when the causality clock hits the frame, then determinism replays the future. RADSCOPE grew a "rewrite history" bar | whodunit: `Purse.gold=1000` at frame 8 → frame 9 recomputes to 870, final stdout becomes "the hero is left with 776 gold" |
| "delta still walks every entity" | archetype-level CoW short-circuit: pointer-equal entity list + column Arcs = whole archetype skipped, O(1); only written columns get row compares | the 256-cell board costs ~0 compares when one cell changes |
| "no persistence" | `ui_enable_persist()` — world rides `fork_to_bytes` into localStorage (saves only after restore had its chance; collab replicas never clobber the host's save) | painted, reloaded the tab, the drawing was still there |
| "errors nuke the app" | runtime handler errors -> toast; the session keeps running. Compile errors still get the full dump | emitted at a nonexistent entity: toast with the message, 256 cells still alive |
| "no focus events" | `Focusable` marker -> `Focus { target, focused }` | wired in the renderer alongside Hover/Drag |
| "hover styles cost a round trip" | `Style.hover_bg/hover_fg` — renderer-local swap, zero session traffic; `Style.anim` = CSS transition ms | pixels' clear button reddens on hover; orbits' moons glide with `anim: 120` |
| "drag moves drop under backpressure" | renderer keeps the trailing move and ships it after the in-flight pump; pixels interpolates the line between samples | a fast (30,30)->(150,90) stroke paints 10 connected cells, not 2 dots |
| "debugger can't debug itself" | third RADSCOPE target: its own merged source | traced itself; its own `ui-*` widget entities appear in its own entity tree |
| "linter cries about pub exports" (2 cycles old) | reachability pass skips `pub` declarations — exports are for importers | lib_gui compiles warning-free |

Plus: struct-update syntax works on component/resource literals
(`UiConfig { undo: 1, ..c }`), which killed the enumerate-every-field
boilerplate in lib_gui's config mutators.

Still open (honest, round 3 edition): collab host election is
last-write-wins-naive (450 ms race window — fine for tabs, not for
adversaries); patch & replay only patches ONE component per run; the
BroadcastChannel transport is same-browser only (the WS relay from the
D4 demo plugs in where the channel does, untested with RADGUI); no JS
test harness still.

## Round 4 — the harness, the election, and the diff

The big one first: a real JS test harness
([projects/playground/test/session.test.mjs](../../../projects/playground/test/session.test.mjs),
`node --test` against a nodejs-target wasm build). Nine tests covering
the layer every demo stands on: lifecycle, render deltas, undo/redo,
the collab replication protocol (host/replica/late-join/out-of-order),
persistence round-trips, patch & replay, and the real lib_gui+counter
stack. It paid for itself on its FIRST run, catching two shipped bugs:

1. **fork_apply's base fingerprint was blind to resources** — it checked
   allocator/entity-count/event-count, so an out-of-order delta whose
   missing predecessor only changed a resource applied cleanly to the
   wrong world. Deltas now carry a blake3 content digest of their base
   (`bdig`) and fork_apply refuses on mismatch.
2. **patch & replay was off by one** — the patch keyed off the causality
   clock, the debugger's slider scrubs timeline indexes. Patch frame 3
   landed in timeline[2]. Now keyed off the timeline index directly.

| critique | fix | proof |
|---|---|---|
| "setTimeout election, split brain, orphaned replicas" | claim-based election (lowest id wins, deterministic), host heartbeat every 1.5 s, replicas re-elect from their own identical world on 5 s of silence; two simultaneous hosts resolve by id comparison | three peers booted simultaneously: exactly one host; killed the host: a survivor took over and painting kept syncing |
| "replicas can't undo, host undo desyncs" | replica Ctrl+Z forwards a history op to the host; host rewinds the SHARED timeline and broadcasts a full-state resync | replica undid a stroke painted from itself: both worlds went 1 → 0; Ctrl+Shift+Z brought it back on both |
| "no redo" | `session_redo` (undo-tree-pruned-to-a-line semantics: a new checkpoint clears the redo branch), Ctrl+Shift+Z / Ctrl+Y | tested in-harness and live |
| "flagship feature hides its value" | the diff view: before patching, the bridge captures the doomed timeline; every scrub then shows old-vs-new at that frame | frame 10 after `Purse.gold=1000` @ 8: `hero.Purse.gold: 45 → 970`; pre-patch frames: "(identical)" |
| "stale hover restore bug" | hover colors are pure CSS now (`:hover` + custom properties) — no JS save/restore to go stale | mid-hover world updates can't be stomped, by construction |
| "transition: all" | scoped to background-color/color/left/top/width/height/opacity | padding/layout changes no longer tween |
| "no virtualization" | the app owns the window: `Scrollable` marker -> `Scroll{top}` events; recycle a fixed row set + two spacers | [biglist.rad](../../../projects/dogfood/radgui/biglist.rad): 10,000 rows in 31 entities / 31 DOM nodes; scrub to the middle: "showing 5000..5025" |
| "persistence loses 1.4 s and clobbers on schema change" | saves debounced 300 ms after each eventful frame; version-stamped against a source hash (stale saves discarded loudly); quota errors disable persistence with a toast, once | reload mid-drawing: the canvas survived; the stamp logic is what kept collab+persist from clobbering each other |
| "relay still unwired" | `?relay=ws://host:port` swaps BroadcastChannel for the D4 fan-out relay — same protocol, zero code changes elsewhere | two peers over a real WebSocket: one host, one replica, strokes synced both ways |
| "no programmatic focus" | `ui_focus(entity)` bumps `FocusNow.seq`; the renderer focuses once per seq | todo refocuses its input after every add (blurred first, to prove it) |
| "toasts stack forever" | one reused toast element | chatty errors update it in place |
| "handler-only functions flagged unused" (pre-existing) | reachability now treats pub events as an external surface: hosts inject them, so their handlers are roots | every GUI app compiles warning-free |

Dogfooding receipts from this round alone: biglist found the
flex-shrink-crushes-spacers renderer bug (a 240,000 px spacer silently
shrank to fit); the harness found the two VM bugs above.

Still open (honest, round 4 edition): the relay trusts everyone in the
room (no auth, no signing — LAN-demo grade); patch & replay is one
component per run and the diff view caps at 64 frames; `beforeunload`
still can't flush a save synchronously (the 300 ms debounce is the
real mitigation); no IME/composition handling in text inputs; the
declarative builder DSL remains unbuilt (every widget is still
create/set/update imperative).

## Gripes found writing apps (for the next cycle)

- **Fixed:** reserved words can now be used as field names in declarations,
  literals, updates, emits, spreads, and field access. `Hidden { on: true }`
  and `hidden.on` are valid; binding positions still reject keyword names.
- **Fixed:** f-string interpolations can contain string-literal indexing.
  Both `f"{ent["id"]}"` and `f"{ent[\"id\"]}"` parse and run.
- The input-value echo problem (renderer vs typing) needed a framework
  convention: lib_gui's default `on Changed` writes `Input.value`, and the
  renderer only applies world CHANGES, never reconciles against the DOM.
