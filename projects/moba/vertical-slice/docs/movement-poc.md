# Movement POC

The movement POC proves that RAD can own both prediction and authority without
copying movement rules into TypeScript.

## Shared RAD Source

The simulation that both the authority and the browser prediction client run is
split by responsibility so engine code never names a roster character:

| File | Responsibility |
|---|---|
| `server/src/sim/components.rad` | Data model: `Position`, `MoveTarget`, `MoveSpeed`, `RenderAvatar`, `PlayerControlled` components and the `MoveOrder`, `Tick`, `AuthoritativeState` events. No logic, no content. |
| `server/src/sim/movement.rad` | Movement behavior only: per-avatar chase, the `MoveOrder`/`AuthoritativeState`/`Tick` handlers, and the `tick_movement` system. |
| `server/src/world/scene.rad` | `MobaScene` render/scene config plus the map-bound `clamp_world_x/y` helpers. |
| `server/src/world/avatars.rad` | World content: role-named avatars resolved per `player_id` (`player_avatar` lookup/auto-seed) and the roster character new avatars are dressed as (`default_avatar_model`). No global "controlled avatar". |

`RenderAvatar.model` defaults to empty in the shared data model. The concrete
roster character (`clockwork_mage`) is chosen only in `world/avatars.rad`, and
the browser maps that id to a visual model through `render/avatarModels.ts`,
falling back to its own default. Gameplay, movement, protocol, and scene code
must never depend on a specific character name.

These files are deliberately free of `use` statements: the browser session
runtime cannot resolve module imports, so `client/src/radHost.ts` concatenates
the shared sources (`components`, `scene`, `avatars`, `movement`) and the client
entry. The native authority instead resolves them through `use` from
`server/src/main.rad`. Either way, prediction and authority run identical logic.

The seed entity is named by gameplay role (`player_1`), never by character, and
the authority resolves avatars by `PlayerControlled.player_id`.

## Client Flow

1. `client/src/app/clientInputController.ts` owns DOM pointer/keyboard binding
   and emits clean move/cast intent callbacks.
2. `client/src/app/MobaRadClient.ts` converts those canvas coordinates into
   world-space intent through the Three.js scene raycast.
3. `client/src/scene.ts` raycasts into the Three.js map plane.
4. `client/src/app/clientCommandDispatcher.ts` reserves the target tick, records
   the command in a typed-array prediction ring, schedules the first resend
   window, and dispatches the fresh input send. Move and cast commands share the
   ring, so packet-loss resend sees one ordered client input stream.
5. `client/src/app/clientPredictionRunner.ts` advances the local RAD session at
   fixed ticks, emits a player-owned `MoveOrder` when that target tick arrives,
   and records a prediction sample for every simulated tick.
6. `client/src/app/clientInputTransport.ts` sends the binary move/cast packet
   through `MatchTransport` and owns later ACK-window resend selection.
7. Each frame, the client consumes the latest parsed authoritative snapshot
   from the bounded transport inbox. Input sends do not wait for per-command
   state responses.
8. The render loop displays predicted movement from the local RAD session,
   interpolating the Three.js mesh between the last two confirmed ticks by the
   fixed-tick clock's sub-tick alpha so motion stays smooth above 128 Hz.
9. If an authoritative correction exceeds the prediction epsilon,
   `clientAuthorityApplier.ts` owns the correction decision and signal, then
   asks `clientPredictionRunner.ts` to snap gameplay state, re-arm, and replay
   locally applied moves in the rollback window. The local Three.js mesh blends
   visually into the corrected position over a short fixed window.
10. `client/src/ui/netcodeHud.ts` samples caller-owned diagnostics every 250 ms
   so the browser can show RTT/jitter, ack loss, stale/dropped snapshots,
   reconciliation rate, resends, server peer/input telemetry, mesh-pool metrics,
   roster/projectile/impact records, and current authority status without
   allocating in the render loop.

## Authority Flow

1. `server/src/main.rad` binds UDP and runs a fixed-tick loop.
2. `server/src/transport/udp_match.rad` drains a bounded datagram budget.
3. `server/src/protocol/match_protocol.rad` parses binary sync, move, cast, and
   disconnect packets.
4. `server/src/server/input_queue.rad` validates and queues player-owned
   target-tick inputs in per-peer move/cast rings through shared duplicate,
   late, lead-window rejection, expired-input, and applied-ACK helpers.
5. `server/src/transport/udp_match.rad` expires idle peers so dead sessions do
   not pin table slots.
6. `server/src/server/clock.rad` applies queued inputs from the fixed tick loop.
7. The server sends binary state snapshots with `session_id`, `player_id`,
   `server_tick`, `server_seq`, receipt `ack_client_seq`, receipt `ack_bits`, and
   server-authored correction reasons plus fixed peer/input telemetry.
8. Browser disposal sends a binary disconnect best-effort; RAD owns
   peer/avatar cleanup.

## Current Limits

This movement slice covers the competitive movement loop: fixed 128Hz RAD
authority ticks, target-tick input queues, client prediction/reconciliation,
receipt/applied ACK windows, bounded latest-state backpressure, ACK-qualified
sync waits, and pooled browser snapshot parsing. The remaining items are outside
the movement POC rather than blockers for testing move feel:

- richer command legality beyond plane clamping, static terrain
  sliding, speed-limited movement, and lag-compensated projectile hitboxes
- typed WASM-memory reads for cold resource/config data; hot avatar movement
  already uses direct WASM-memory render reads
- damage/cooldown/resource rules behind confirmed projectile hits; cast packets,
  authoritative projectile snapshots, v10 projectile impact reason records, and
  pooled impact effects are already live
