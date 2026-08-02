# Orianna Ability Lab

Browser MOBA dogfood for Rad's gameplay stack. Rad owns the simulation and
the browser host renders the ECS world through the Three.js canvas.

For the full architecture, runtime path, and source feature ledger, see
[Game Embedding & MOBA Dogfood](../guide/game-embedding.md).

Runtime summary: the lab uses the primary `core/vm` WASM `RadRuntime` through
`projects/playground/moba_host.js`. It does not run through `core/c-backend/`
or call `core/simcore/`.

Run:

```powershell
py -m http.server 8137
# http://127.0.0.1:8137/projects/playground/orianna_arena.html
```

Checks:

```powershell
target\debug\rad.exe projects\dogfood\orianna_gui\orianna_arena.rad --deny-warnings
node --test projects\playground\test\orianna_arena.test.mjs
```

Files:

- `arena_schema.rad` - public ECS schema and host event contract.
- `projects/dogfood/moba/map4_data.rad` - browser-moba map4 source data.
- `moba_stack.rad` - Vec2, spatial queries, cooldown helpers, ability specs,
  logging, buffs, and fail-fast entity lookup.
- `orianna_arena.rad` - Orianna's Q/W/E/R/passive implementation plus the
  automatic 1125-unit ball leash.
- `projects/playground/moba_host.js` - browser module bundler, typed event
  bridge, runtime contract checks, undo/redo, and inspect/why support.
- `projects/playground/orianna_three_renderer.js` - Three.js scene renderer
  driven by RAD snapshots and browser-moba map4 metadata.

Map contract: RAD exports the browser-moba map4 plane as exactly 282000 by
155000 world units, with origin `(-136210, -72080)`, west spawn
`(-133650, 6700)`, and east spawn `(133560, 5970)`. Orianna and the ball spawn
at the west spawn; the primary red dummy marks the east spawn.
