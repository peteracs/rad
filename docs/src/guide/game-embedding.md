# Game Embedding & MOBA Dogfood

Rad's game embedding path is the same inversion-of-control model as GUI
embedding: Rad owns simulation state, systems, events, and provenance; the
host owns input, rendering, audio, and platform APIs. The Orianna ability lab
is the current browser-game dogfood target for that stack.

Run it from the repo root:

```powershell
py -m http.server 8137
# open http://127.0.0.1:8137/projects/playground/orianna_arena.html
```

The app code lives in:

- `projects/dogfood/orianna_gui/arena_schema.rad` - public ECS schema and host event
  contract.
- `projects/dogfood/moba/map4_data.rad` - generated browser-moba map4 metadata,
  lane landmarks, and spawn points consumed by the Orianna schema.
- `projects/dogfood/orianna_gui/moba_stack.rad` - reusable game helpers.
- `projects/dogfood/orianna_gui/orianna_arena.rad` - Orianna-specific spell logic.
- `projects/playground/moba_host.js` - browser session host, module bundler, typed
  event bridge, runtime contract checks, undo/redo, and inspect support.
- `projects/playground/orianna_arena.js` - input, session pump, and HUD.
- `projects/playground/orianna_three_renderer.js` - Three.js renderer using the
  RAD `GameHostConfig` plus browser-moba map4 dimensions and lane landmarks.
- `projects/playground/orianna_arena.css` - full-bleed scene layout and HUD.

## Runtime Path

The Orianna ability lab uses the primary VM path:

1. `projects/playground/orianna_arena.js` creates a `RadMobaHost`.
2. `projects/playground/moba_host.js` imports `RadRuntime` from
   `projects/playground/pkg/rad_vm.js`.
3. The host bundles `arena_schema.rad`, `moba_stack.rad`, `orianna_arena.rad`,
   and transitive `use` imports such as `map4_data.rad` into one source string.
4. `RadRuntime.session_start()` compiles and runs the bundled Rad source, then
   the browser drives gameplay with `session_emit()`, `session_pump()`,
   `session_render_delta()`, `session_checkpoint()`, `session_undo()`,
   `session_redo()`, and `session_why()`.

The lab does not run through `core/c-backend/`, and it does not call
`core/simcore/`. `core/simcore/` is the separate compiled MOBA damage kernel
used by the broader MOBA dogfood path and golden corpus; `core/c-backend/` is
frozen legacy code and is not part of the browser/game runtime.

## `moba-rad` Phase 3: UDP Authority + WebTransport Client

`projects/moba-rad/` is the clean client/server dogfood scaffold for the next
MOBA runtime. The project root intentionally contains only `client/`,
`server/`, and `docs/`.

For the full browser networking boundary and rationale, see
[WebTransport Edge Networking](./webtransport-networking.md).
For the project-local runbook, see `projects/moba-rad/docs/`.

Run the client:

```powershell
cd path\to\rad\projects\moba-rad\client
npm run dev
# open http://127.0.0.1:5174/
```

Run the RAD authority server:

```powershell
cd path\to\rad\projects\moba-rad\server
npm run dev
# UDP match socket: 127.0.0.1:8788
```

The server `npm run dev` script invokes the local RAD CLI on
`server/src/main.rad`. If `target/debug/rad.exe` has not been built yet, run
`npm run dev:cargo` from the same folder or build `rad-vm` from the repo root.

Run the browser edge proxy in another terminal:

```powershell
cd path\to\rad\projects\moba-rad\server
npm run proxy
# WebTransport: https://localhost:4433/match
# Forwards datagram payloads to UDP authority: 127.0.0.1:8788
```

The edge proxy is a transport adapter, not a game server. It owns
HTTP/3/QUIC/TLS/WebTransport, creates one localhost UDP socket per browser
session, and forwards raw match packet bytes to the RAD authority. RAD remains
the only place for protocol grammar, movement validation, fixed ticks, and
state snapshots.

For stable browser runs, provide a certificate trusted by the browser:

```powershell
$env:MOBA_RAD_CERT_PEM="H:\path\to\localhost.pem"
$env:MOBA_RAD_KEY_PEM="H:\path\to\localhost-key.pem"
npm run proxy
```

If no PEM files are provided, the proxy creates a short-lived self-signed
identity and prints a SHA-256 certificate hash. Put that hash in
`VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH` before starting the client when using
browser certificate-hash authentication.

Phase 3 keeps the local feel loop while making the authoritative path explicit:

1. The browser host owns pointer input and Three.js rendering.
2. Right-click raycasts from the canvas into the black map plane.
3. The host records the move/cast input in a typed-array prediction ring and
   assigns a monotonically reserved target simulation tick.
4. The browser transport is `WebTransport` datagrams. It sends the same compact
   match packets as the native UDP path; there is no HTTP polling bridge.
5. `server/edge-proxy` terminates HTTP/3/QUIC WebTransport sessions and
   forwards packet payloads to the RAD UDP authority. RAD owns the game
   protocol and simulation, not the QUIC implementation.
6. The RAD server binds UDP on `127.0.0.1:8788` and accepts compact
   `moba-rad/udp-v10-peer-snapshot` sync, move, cast, and disconnect
   datagrams.
7. The server computes a tick-aware UDP receive timeout, drains a bounded
   packet budget, remembers peers by `session_id:player_id`, validates/queues
   move/cast inputs into that peer's fixed target-tick rings, rejects duplicate
   active `player_id` ownership, handles disconnect packets, expires idle peer
   entities, advances a fixed-tick accumulator, and sends binary state
   datagram snapshots to connected peers.
8. `projects/moba-rad/server/src/sim/components.rad` owns `Position`,
   `MoveTarget`, `MoveSpeed`, `RenderAvatar`, `PlayerControlled`,
   `MoveOrder`, `Tick`, and `AuthoritativeState`; movement behavior lives in
   `projects/moba-rad/server/src/sim/movement.rad`.
9. The client bundles that same simulation RAD source into WASM; the server
   imports it from `server/src/sim`.
10. `Tick { dt }` advances the controlled avatar toward the clamped target inside RAD on both
   sides.
11. The renderer reads the RAD world snapshot and places the avatar mesh from
    the `Position` component. TypeScript does not integrate movement.

Relevant files:

- `projects/moba-rad/server/src/sim/movement.rad` - one RAD source of truth for
  movement behavior, prediction ticks, and authoritative state application.
- `projects/moba-rad/server/src/sim/components.rad` - shared ECS components and
  event declarations.
- `projects/moba-rad/server/src/world/scene.rad` - map-plane dimensions and
  world clamping helpers.
- `projects/moba-rad/server/src/world/avatars.rad` - player avatar lookup,
  seeding, and render model metadata.
- `projects/moba-rad/server/src/server/state.rad` - server config, stats,
  sequence counters, lifecycle control, shutdown events, and typed stat events.
- `projects/moba-rad/server/src/server/clock.rad` - fixed-tick accumulator and
  catch-up cap.
- `projects/moba-rad/server/src/server/input_queue.rad` - target-tick jitter
  buffer with late/future/duplicate input rejection, shared peer-status helpers,
  and receipt/applied ACK tracking for move and cast inputs.
- `projects/moba-rad/server/src/protocol/match_protocol.rad` - compact UDP
  packet encode/decode helpers.
- `projects/moba-rad/server/src/transport/udp_match.rad` - bounded datagram
  receive loop and state snapshot sends.
- `projects/moba-rad/server/edge-proxy/src/main.rs` - WebTransport edge
  process that forwards browser datagrams to the RAD UDP authority.
- `projects/moba-rad/client/src/rad/main.rad` - client host contract and
  world seeding.
- `projects/moba-rad/client/src/radHost.ts` - browser session wrapper over
  `session_start`, `session_emit`, `session_pump`, `session_render_delta`, and
  authoritative reconciliation.
- `projects/moba-rad/client/src/transport/serverState.ts` - transport-neutral
  authoritative state shape.
- `projects/moba-rad/client/src/transport/matchProtocol.ts` - client-side
  binary packet encode/decode helpers.
- `projects/moba-rad/client/src/transport/matchWire.ts` - browser-side
  little-endian and fixed-point wire primitives shared by packet encoders and
  parsers.
- `projects/moba-rad/client/src/transport/serverStateBuffer.ts` - reusable
  `ServerState` buffers, record resizing, and copy helpers for pooled snapshot
  parsing.
- `projects/moba-rad/client/src/transport/webTransportStateRouter.ts` -
  parsed-state pools, bounded latest-state inbox routing, and ACK-qualified
  sync waiters.
- `projects/moba-rad/client/src/transport/matchTransport.ts` - client-side
  transport interface used by the game loop, including the `close()` lifecycle
  hook.
- `projects/moba-rad/client/src/transport/webTransport.ts` - browser
  WebTransport datagram implementation for the match transport interface.
- `projects/moba-rad/client/src/netcode/` - fixed client clock and typed-array
  prediction/reconciliation buffers.
- `projects/moba-rad/client/src/app/clientInputController.ts` - DOM listener
  lifecycle, pointer memory, Q aim state, resize/debug dispatch, and clean
  move/cast intent callbacks.
- `projects/moba-rad/client/src/app/clientAuthorityApplier.ts` - accepted
  authority snapshot application: ACK updates, visual projection, clock sync,
  applied-input cleanup, reconciliation decision, correction signaling, and
  prediction-runner replay.
- `projects/moba-rad/client/src/app/clientAuthorityRequester.ts` -
  ACK-qualified authority sync/poll cadence, in-flight guard, RTT/jitter timing,
  and transport-failure telemetry.
- `projects/moba-rad/client/src/app/clientCommandDispatcher.ts` - target-tick
  move/cast command reservation, prediction-ring writes, first retransmit
  scheduling, and fresh input send handoff.
- `projects/moba-rad/client/src/app/clientInputTransport.ts` - fresh move/cast
  datagram sends, bounded resend cadence, oldest-unacked input selection, and
  transport-failure telemetry.
- `projects/moba-rad/client/src/app/clientPredictionRunner.ts` - local RAD
  fixed-tick stepping, simulated-tick frontier, per-tick prediction samples,
  local scene samples, and authoritative replay after correction.
- `projects/moba-rad/client/src/scene.ts` - Three.js plane, right-click raycast, target
  marker, and snapshot-to-mesh projection.
- `projects/moba-rad/client/src/main.ts` - browser bootstrap and page lifecycle.
- `projects/moba-rad/client/src/app/MobaRadClient.ts` - app lifecycle owner:
  RAF scheduling, authority consumption, reconciliation orchestration, transport
  close, and scene disposal.
- `projects/moba-rad/server/src/main.rad` - RAD orchestration: UDP bind,
  fixed-tick loop, bounded input pump, snapshot fanout, and graceful `udp_close`
  on shutdown.

Dogfooding this phase added native RAD networking primitives:
`tcp_accept_timeout`, `udp_bind`, `udp_recv_from`, `udp_recv_from_timeout`,
`udp_recv_from_bytes`, `udp_recv_from_bytes_timeout`, `udp_send_to`,
`udp_send_to_bytes`, `udp_recv_bytebuf`, `udp_recv_bytebuf_timeout`,
`udp_send_bytebuf`, and `udp_close`. Timeout receives return `Option` instead of
sentinel handles so server loops can poll sockets without blocking simulation.
The bytebuf variants are the preferred binary-packet path; byte-list variants
remain compatibility helpers, and string variants remain useful for diagnostics
and simple text protocols. The MOBA project uses UDP directly on the authority
side and WebTransport on the browser side; TCP/HTTP probes are deliberately
absent from this project.

The current dogfood pass added sequence numbers, session/player identity,
receipt ack bits split from the applied ACK window, target ticks, server ticks,
bounded RAD peer entities, per-peer input rings, explicit disconnect,
peer timeout cleanup, client ack diagnostics, adaptive input delay, move/cast
input send/resend from the shared browser prediction ring, client rollback/replay
scaffolding, local visual
correction smoothing, server-authored correction reasons, cast/projectile
packet records, projectile impact records, fixed-stride peer-table records,
bounded browser snapshot backpressure, frame-driven authoritative snapshot
consumption, discarded-snapshot diagnostics, WebTransport reconnect hygiene,
live browser netcode telemetry, and
binary `moba-rad/udp-v10-peer-snapshot` packet encoding over RAD bytebuf
UDP builtins. The RAD authority also owns `ServerControl` lifecycle state and
closes its UDP socket with `udp_close` on graceful loop exit.
Next protocol work belongs in `server/src/protocol/match_protocol.rad` and the
matching client `transport/matchProtocol.ts`: input flags and richer
authority-side resend policy tuning. Do
not add a second JSON/HTTP protocol for those features.

## Dogfooded Features

| Feature | Where it is exercised |
|---|---|
| Public module stack | `orianna_arena.rad` imports schema and stack modules; the browser bundler merges them for WASM sessions. |
| Fail-fast entity lookup | `moba_stack.expect_entity` wraps `require_entity`; RADGUI's `ui_root` now uses the same nullable-free path. |
| Vec2 and geometry helpers | `Vec2`, `dist`, `move_towards_pos`, and `clamp_world_pos` drive movement, Q clamp, and missile travel. |
| Map4 movement scale | `map4_move_step` keeps champion `move_speed` stats but converts them to browser-moba map units before `MoveUnits` integrates position. |
| Cooldown/timer helpers | `ability_spec`, `cd_remaining`, `tick_cooldowns`, and `spend_and_cd` replace slot-specific boilerplate. |
| Spatial queries | `units_in_radius` powers W, R, zone ticks, and ball-hit detection. |
| Ability DSL | `AbilitySpec` carries slot, label, mana, cooldown, range, radius, and flags for Q/W/E/R. |
| Typed host event bridge | `ORIANNA_EVENT_SCHEMA` validates host-pushed events before `session_emit`. |
| Game host API | `RadMobaHost` wraps session start, event pumps, render deltas, checkpoints, undo/redo, and `why()`. |
| Three.js render host | `orianna_three_renderer.js` turns ECS snapshots into meshes, rings, beams, labels, and the browser-moba map4 plane. |
| Runtime/version checks | `RadRuntime.runtime_features()` and `HostContract` reject stale hosts before gameplay starts. |
| Inspector and time travel | The HUD's Inspect button calls `session_why`; Undo/Redo use session checkpoints to rewind the whole world. |

## Source Feature Ledger

When auditing the Orianna source, treat these as language features already used
by the project:

| Feature | Source receipt |
|---|---|
| Public module imports and exports | `use "arena_schema.rad"`, `use "moba_stack.rad"`, and `pub` declarations in the schema/stack modules. |
| Resources with default initialization | `MatchClock`, `Ids`, `HostContract`, and `GameHostConfig` are read with `res(...)` and reset with `set_resource(...)`. |
| Indexed component fields and lookup | `indexed id`, `indexed key`, plus `lookup(Unit, "id", ...)`, `lookup(Buff, "key", ...)`, and `lookup(BallHitMarker, "key", ...)`. |
| Struct/component spread updates | `SpellBook { sealed: true, ..require(caster, SpellBook) }`, `OriannaState { ..., ..state }`, and similar component replacements. |
| Option matching and pipeline unwrap | `match lookup(...) { Some(...) => ..., None => ... }` and `lookup(...) |> unwrap`. |
| ECS queries, entity creation, remove, and despawn | `query { ... }`, `entity "name" { ... }`, `remove(self, ...)`, and `despawn(...)`. |
| System params with mutable components and `self` | Systems such as `MoveUnits`, `BallTravelMove`, `ZoneTick`, and `BuffTick` mutate component params and use `self`. |
| Scheduled systems and event flushing | `on Tick` updates `MatchClock`, calls `schedule [...]`, then `flush_events()`. |
| Host-pushed typed events | `Tick`, `MoveCommand`, `CastCommand`, `ResetGame`, and `BasicAttackCommand` are declared in Rad and validated in JS before `session_emit`. |
| Render contract metadata | `GameHostConfig` publishes the map4 world as 282000 by 155000 units, origin `(-136210, -72080)`, bounds `(145790, 82920)`, west spawn `(-133650, 6700)`, east spawn `(133560, 5970)`, grid metadata, and camera defaults consumed by the Three.js host. |
| F-strings and list append sugar | Log/travel keys use `f"..."`; radius filters collect entities with `found << unit`. |

## Test Receipts

Native Rad check:

```powershell
target\debug\rad.exe projects\dogfood\orianna_gui\orianna_arena.rad --deny-warnings
```

Browser/Node session checks:

```powershell
wasm-pack build --target web --out-dir ..\..\projects\playground\pkg core\vm
wasm-pack build --target nodejs --out-dir ..\..\projects\playground\pkg-node core\vm
node --test projects\playground\test\orianna_arena.test.mjs
node --test projects\playground\test\session.test.mjs
```

What the browser lab verifies:

- click-to-move Orianna on canvas
- Q ground cast, W ball AOE, E allied shield/ball attach, R shockwave,
  automatic ball leash, and passive hit
- runtime/stack contract visible in the HUD
- Inspect mode showing component fields and causal `why()` output
- Undo/Redo rewinding and replaying whole-world checkpoints
- responsive layout at narrow viewport sizes

## Scaling To More Champions

Keep champion scripts thin. Shared MOBA mechanics should move into modules
like `moba_stack.rad`: target validation, spell specs, cooldown/cost checks,
projectiles, buffs, spatial filters, and host contracts. A champion module
should mostly define its data, charscript initialization, and spell-specific
effects.

For a larger roster, prefer generated public schema/data modules plus small
handwritten champion behavior modules. The browser host should keep accepting
typed events and reading ECS state; it should not grow champion-specific
logic beyond rendering metadata.
