# Orianna Ability Lab

This folder contains the Rad implementation of Orianna's playable ability lab:
Q, W, E, R, passive hit, cooldowns, mana, ball travel, buffs, damage,
the 1125-unit automatic ball leash, inspect/why receipts, and undo/redo
integration.

Canonical docs live in `docs/src/examples/orianna-ability-lab.md` and
`docs/src/guide/game-embedding.md`. This README is only the local runbook.

## Files

- `arena_schema.rad` - public ECS schema and browser host event contract.
- `moba_stack.rad` - shared MOBA helpers: Vec2 math, spatial queries, cooldowns,
  buffs, damage, logging, and fail-fast entity lookup.
- `orianna_arena.rad` - Orianna-specific spell logic used by the browser lab.
- `../moba/kit/orianna_dsl.rad` - raw DSL/provenance receipt for Shockwave,
  Protect missile line damage, delayed timers, and `why()` output.
- `../../playground/orianna_arena.html` - browser canvas entrypoint.
- `../../playground/orianna_three_renderer.js` - Three.js renderer using the
  RAD world contract and browser-moba map4 metadata.

## Native Checks

Run from the repo root:

```powershell
cargo build -p rad-vm --bin rad
target\debug\rad.exe projects\dogfood\orianna_gui\orianna_arena.rad --deny-warnings
target\debug\rad.exe projects\dogfood\moba\kit\orianna_dsl.rad --deny-warnings
```

The first Rad file is the GUI/session module. The second prints the raw
Shockwave and Protect causality receipts.

## Browser Lab

Build the browser WASM package once:

```powershell
wasm-pack build --target web --out-dir ..\..\projects\playground\pkg core\vm
```

Then serve the repo root:

```powershell
py -m http.server 8137 --bind 127.0.0.1
```

Open:

```text
http://127.0.0.1:8137/projects/playground/orianna_arena.html
```

If port `8137` is busy, use another port and change the URL accordingly.

## Browser Controls

- Click the arena to move Orianna.
- Press/click `Q`, then click a ground position for Command: Attack.
- Press/click `W` to detonate Dissonance at the ball.
- Press/click `E`, then click a blue ally to shield and attach the ball.
- Press/click `R` to cast Shockwave from the ball.
- Press/click `A`, then click an enemy for the passive hit.
- Use Inspect, Undo, Redo, Pause, Step, and Reset from the HUD.

## Node Session Tests

Build the Node WASM package:

```powershell
wasm-pack build --target nodejs --out-dir ..\..\projects\playground\pkg-node core\vm
```

Run the session tests:

```powershell
node --test projects\playground\test\orianna_arena.test.mjs
```
