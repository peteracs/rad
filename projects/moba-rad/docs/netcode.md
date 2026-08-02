# Netcode Architecture

The movement POC now has a real authoritative netcode shape instead of a fast
empty transport pipe.

## Current Contract

| Layer | Contract |
|---|---|
| Client clock | Fixed 128 Hz accumulator with a catch-up cap, run `PREDICTION_LEAD_TICKS` ahead of the authoritative tick so reconciliation always finds the compared server tick already recorded |
| Client prediction | Typed-array ring buffer for move/cast input ticks and predicted positions |
| Client authority gate | Filters rejected, wrong-session/player, stale, and invalid authority snapshots before ACK diagnostics or rollback can consume them |
| Client authority applier | Owns accepted authority snapshot application: ACK updates, visual projection, clock sync, applied-input cleanup, reconciliation decision, correction signaling, and replay trigger |
| Client reconciliation policy | Decides older-command echo suppression, rollback need, soft visual correction, and hard-correction signaling from scalar snapshot/prediction inputs |
| Client reconciliation | Ignore stale server sequence numbers, snap to authoritative state on mismatch, replay local inputs through the current tick, and visually smooth local correction jumps |
| Client rendering | Local meshes interpolate predicted/authoritative samples; remote meshes render delayed historical snapshots from a pre-warmed mesh pool |
| Client input controller | Owns DOM listener binding, pointer memory, Q aim state, resize/debug events, and clean move/cast intent callbacks |
| Client ack diagnostics | Reads receipt `ack_client_seq`/`ack_bits` into `AckDiagnostics` for loss ratio, move/cast resend choice, and adaptive input-delay recommendations |
| Client authority requester | Owns ACK-qualified sync/poll cadence, in-flight gating, RTT/jitter timing samples, and transport-failure telemetry |
| Client command dispatcher | Converts local move/cast intent into target-tick reservations, prediction-ring records, retransmit scheduling, and fresh input sends |
| Client input transport | Owns fresh move/cast datagram sends, resend cadence, oldest-unacked input selection, sent-packet counters, and transport-failure telemetry without parsing state snapshots |
| Client prediction runner | Owns local RAD fixed-tick stepping, simulated-tick frontier, per-tick prediction samples, local scene samples, and authoritative replay |
| Client rollback cleanup | Clears prediction history through server-authored `last_applied_client_seq` + `applied_ack_bits`, not through receipt ACKs or a lossy high watermark |
| Client snapshot backpressure | `ServerStateInbox` keeps parsed WebTransport snapshots in a bounded latest-state ring so stale bursts cannot grow memory or add latency; discarded snapshots are counted for diagnostics |
| Client reconnect hygiene | Unexpected WebTransport close/read-loop end clears cached browser handles and pending sync waiters so the next send can reconnect |
| Client live diagnostics | `MobaRadClient.writeNetcodeDiagnostics(...)` writes into caller-owned telemetry storage; the browser HUD shows authority sync, RTT/jitter, tick drift, prediction lead, ack/loss, reconciliation rate, resend pressure, accepted/stale/dropped snapshots, server peer/input telemetry, peer record count, applied ACK bits, mesh-pool metrics, roster records, projectile records, and projectile impact records |
| Client lifecycle | `MobaRadClient.dispose()` cancels RAF, disposes the input controller, closes the match transport, and disposes Three.js resources |
| Wire protocol | `moba-rad/udp-v10-peer-snapshot`: magic/version/kind header, little-endian numeric fields, fixed-point coordinates |
| Wire input | kind `2`, 31 bytes: `client_seq`, `session_id`, `player_id`, `target_tick`, `command_id`, `target_x_i32`, `target_y_i32` |
| Wire cast | kind `6`, 35 bytes: `client_seq`, `session_id`, `player_id`, `target_tick`, `command_id`, `dir_x_i32`, `dir_y_i32`, `fire_view_tick` |
| Wire sync | kind `1`, 15 bytes: `client_seq`, `session_id`, `player_id` |
| Wire disconnect | kind `3`, 15 bytes: `client_seq`, `session_id`, `player_id` |
| Wire state | kind `4`, `92 + 44 * peer_record_count + 26 * avatar_count + 36 * projectile_count + 25 * impact_count` bytes: local authoritative fields, correction reason, fixed peer/input telemetry, peer records, avatar roster, projectile roster, and projectile impact records |
| Wire state scaling | Per-viewer interest culling plus `ServerConfig.snapshot_mtu_bytes` caps each roster below the UDP payload budget |
| Server clock | RAD fixed-tick accumulator at 128 Hz |
| Server idle wait | RAD computes a tick-aware UDP receive timeout, then drains the rest of the packet budget nonblocking |
| Server peer table | RAD `PeerConnection` entities keyed by `session_id:player_id`, capped at 8 active peers for this POC |
| Server peer lifecycle | RAD expires idle peers after `ServerConfig.peer_timeout_ms`, frees their table slots, and despawns their avatars |
| Server lifecycle | RAD `ServerControl` owns the authority run flag; graceful shutdown exits the fixed loop and closes the UDP socket with `udp_close` |
| Server jitter buffer | RAD `PeerInputRing` and `PeerCastRing`, 32 slots per peer each, apply inputs only when `server_tick == target_tick` |
| Server validation | Rejects duplicate, late, too-far-ahead, stale-session, invalid-number, and malformed packets before simulation; packets set move targets only, never positions |
| Server combat | Fixed-tick projectile simulation records rollback history, advances projectiles, and resolves lag-compensated hits |
| Server replay | Live authority writes a deterministic replay tape: initial world plus applied move/cast inputs by execution tick |

## Why Target Ticks

Datagrams do not arrive evenly. The client waits for an authority sync, then
tags each input command with the server tick where it should take effect. The
browser schedules move and cast commands onto monotonically increasing target
ticks before writing them into the bounded prediction ring, so a same-frame
right click plus Q cast cannot overwrite one another locally. The RAD authority
queues the input and applies it from the fixed-tick loop, not from the socket
receive callback. That keeps simulation order independent from packet arrival
jitter.

The client predictor treats target ticks as deadlines, not one-frame-only
appointments. If an authoritative snapshot advances the local clock past a
queued move's exact target tick before the browser predictor sees that tick, the
input remains due and is applied once on the next local simulation tick. This
prevents the visible target reticle from moving while the predicted avatar stays
idle. Rollback replay re-arms locally applied move inputs inside the replay
window before simulating them again from authoritative state.

The input lead is dynamic. `AckDiagnostics` combines the receipt ACK loss window
with the latest authority RTT/jitter sample:

```text
lead_ticks = ceil(RTT_ms / (2 * tick_ms)) + ceil(jitter_ms / tick_ms) + 2
```

The result is clamped between `INPUT_DELAY_TICKS` and
`MAX_INPUT_DELAY_TICKS`. On localhost this usually resolves to 3-4 ticks, while
the chaos-harness budget (`+120ms` RTT sample plus jitter) expands to roughly
12 ticks. This keeps browser inputs aimed into the RAD authority's future input
queue instead of arriving after the fixed tick has already simulated. The
client cap intentionally matches the server's `max_input_lead_ticks` window so
adaptive prediction cannot create commands the authority must reject as
too-far-ahead.

`client/src/netcode/authorityStateGate.ts` owns authority snapshot acceptance.
It increments received/accepted/stale/rejected counters, records the latest
status/correction reason, rejects wrong-session/player packets, rejects invalid
tick/sequence/ACK fields before they can poison diagnostics, rejects non-finite
authoritative local-avatar coordinates before rollback or Three.js can consume
them, and only advances `last_server_seq` on fresh accepted snapshots.

`client/src/netcode/reconciliationPolicy.ts` owns the scalar decision that
follows acceptance: ignore an older authority echo while a newer local command is
still active, replay when prediction history is missing, replay when the
same-command target-active bit disagrees, and distinguish soft visual correction
from a hard correction signal. `client/src/app/clientAuthorityApplier.ts`
supplies caller-owned decision scratch, so this branch does not allocate in the
frame loop.

## Reconciliation

The browser stores predicted positions by tick. Each authoritative state packet
carries `session_id`, `player_id`, `server_tick`, and `server_seq`. The client
drops stale snapshots plus snapshots for other sessions or players. For a fresh
local-player snapshot, it compares the authoritative position to the predicted
position at the same tick. `client/src/app/clientAuthorityApplier.ts` owns that
accepted-snapshot chain: update receipt ACK diagnostics, project accepted
visual state, sync the local prediction clock, clear applied inputs, ask the
reconciliation policy, and trigger replay if needed. If the error is above the
epsilon, or the target state disagrees for the same command, the client:

1. emits `AuthoritativeState` into the local RAD session,
2. re-arms locally applied move inputs in the rollback window and replays them
   from `server_tick + 1` through the current client tick,
3. records a new render sample from the RAD snapshot.

Prediction history is written at fixed-tick granularity. A browser frame can
consume multiple 128 Hz simulation ticks before rendering once;
`client/src/app/clientPredictionRunner.ts` therefore refreshes RAD state and
records `PredictionBuffer.recordPosition(...)` after every `session.tickFixed()`
call, not just at the end of the RAF frame. That prevents intermediate ticks
from becoming false `missing prediction` rollbacks when a server snapshot lands
on one of those ticks.

### Prediction Lead

Per-tick recording alone is not sufficient. The client clock must also run a
fixed `PREDICTION_LEAD_TICKS` (currently 4) *ahead* of the latest authoritative
tick. The lead is injected when an authoritative snapshot syncs the clock
(`clock.setTick(server_tick + PREDICTION_LEAD_TICKS)`), and
`client/src/app/clientPredictionRunner.ts` then integrates and records every tick
from the current prediction frontier up to `clock.tick` — including any ticks a
snapshot sync jumped the clock over.

Without the lead, the client clock pins exactly to `server_tick`. Because an
authoritative snapshot for tick *T* is processed at the top of the frame, before
that frame records its own prediction for *T*, `hasPositionAt(T)` is false on
essentially every snapshot received while moving. The reconciliation then fires
on every snapshot — not because the predicted and authoritative positions
disagree (the shared `sim/movement.rad` integrator is bit-identical on both
ends) but purely because the compared sample has not been recorded yet. The
symptom is a reconciliation rate near `128/128` during movement and `0/128`
while idle (idle gates authority polling off, so no snapshots are processed).
Leading the authority keeps the compared (past) server tick reliably inside the
prediction ring, dropping the moving-window reconciliation rate to ~0%.
`remoteRenderTick()` subtracts the same lead so remote-avatar interpolation keeps
its full delay buffer and stays paced against authoritative time.

This invariant is locked headlessly by
`client/test/predictionDesyncRepro.test.ts`, which drives the real
`FixedTickClock`, `PredictionBuffer`, and `PredictedMoveApplier` against a
deterministic authority running the same integrator. It asserts that the
zero-lead path reconciles on >80% of moving snapshots (all from missing
prediction, never a position error) while the lead + per-tick-recording path
stays under 5%.

## Buffer Sizing & Flood Resilience

Both ring buffers are sized so the worst case the chaos harness can emulate
(`+120ms` latency with `+/-15ms` jitter each way, ~300ms RTT ≈ 39 ticks at
128 Hz) never overruns them.

- **Client prediction history** (`client/src/netcode/predictionBuffer.ts`,
  `PREDICTION_RING_SIZE = 256`) holds 2.0 s of input/position history — roughly
  6.7x the worst-case RTT. A snapshot delayed by the full RTT still lands on its
  original, un-lapped slot, so reconciliation never snaps for lack of history.
  The size is a power of two so slot indexing is a single `& mask`, and every
  read is guarded by an exact-tick comparison (`positionTicks[slot] === tick`):
  a tick that *has* been lapped reports stale rather than mis-reconciling against
  a recycled slot.
- **Server input rings** (`server/src/server/input_queue.rad`, 32 slots/peer
  each) cannot overflow because `queue_move_input` and `queue_cast_input` only
  accept ticks in `(current, current + max_input_lead_ticks]` (24 < 32). A
  packet storm or jitter burst can therefore occupy at most 24 live slots per
  input kind; everything past the window is rejected as too-far, and a same-tick
  storm collapses into one slot (latest-seq-wins) instead of consuming slots.
  The rings are fixed-size, so there is no allocation, no growth, and no panic
  under flood.

Move and cast inputs share one RAD rejection gate:
`reject_unqueueable_input(...)` checks the receipt/applied ACK windows and the
target-tick lead window before either input kind can touch its ring. The status
helpers (`mark_duplicate_input`, `mark_late_input`, `mark_future_input`, and
`mark_input_queued`) are the only place that advances queue rejection counters
and `last_status`, so move/cast telemetry cannot drift while the packet grammar
evolves.

## Visual Interpolation

The scene graph is deliberately downstream from simulation. `MobaRadClient`
pushes avatar samples into `MobaRadScene.applyAvatarState(state, tick)` only
after a fixed simulation tick refreshes the RAD world. The render loop then
calls `MobaRadScene.render(clock.interpolationAlpha)`, and
`client/src/render/avatarInterpolator.ts` writes interpolated scalar fields into
caller-owned render-state structs. This gives high-refresh displays smooth mesh
movement without letting Three.js transforms feed back into collision,
prediction, or reconciliation.

Remote avatars use the roster tail in binary state packets. The client buffers
each non-local `player_id` in `client/src/render/avatarTimeline.ts` and renders
`REMOTE_INTERPOLATION_DELAY_TICKS` behind the estimated server tick. That keeps
other players and creeps smooth through jitter without predicting their future
inputs.

The interpolation paths allocate their `AvatarRenderState` scratch objects once
when an avatar view is created. The requestAnimationFrame path only updates
numbers and existing Three.js transforms.

## Interest Management

State packets are MTU-bounded. The authority computes the viewer's current
authoritative position and writes only avatars that are inside
`ServerConfig.snapshot_interest_radius`, always including the viewer's own
avatar. `max_state_avatar_records()` derives the hard cap from
`ServerConfig.snapshot_mtu_bytes`, `state_packet_header_len()`, and
`state_avatar_record_len()`, so a larger match cannot silently cross the safe UDP
payload budget.

The current default is 1200 bytes. With the v10 92-byte state header and no
projectiles or impact records, a snapshot can carry up to 42 fixed-stride avatar
records when there are no peer-table records. Peer-table records are budgeted
before avatar records: each connected peer costs 44 bytes and carries
`player_id`, `session_id`, receipt/applied sequence state, pending move/cast
counts, connection state, and rejection counters. With the current 8-peer cap,
that still leaves room for 29 avatar records inside the 1200-byte snapshot
budget. This keeps binary snapshots below the fragmentation danger zone while
preserving one single source of protocol truth in RAD.

## Mesh Pooling

Remote avatars spawn and despawn as peers enter and leave the visual roster.
Constructing a `THREE.Mesh`/material on demand would force the WebGL driver to
compile a shader mid-frame (a multi-millisecond hitch), and discarding meshes on
despawn would hand the JS garbage collector work that surfaces as render-loop
micro-stutters. `client/src/render/avatarMeshPool.ts` removes both.

`RemoteAvatarMeshPool` pre-allocates `REMOTE_AVATAR_POOL_SIZE` avatar/target-
indicator mesh pairs when the scene is built, so every shader compiles once at
load. Entries are keyed by avatar model. `acquire(model)` hands back an idle
entry of the matching model (or `null` when the pool is saturated, which the
scene treats as "skip this remote this frame"); `release(handle)` only hides the
meshes and marks the entry idle — it never disposes. `RemoteAvatarView` wraps a
handle instead of owning meshes, so a despawn returns the handle to the pool with
zero allocation and zero GPU churn. `RemoteAvatarMeshPool.dispose(scene)` tears
down every entry's geometries and materials on match teardown.

Projectile and impact visuals use the same rule. `ProjectileMeshPool`
pre-allocates skillshot meshes, and `ImpactEffectPool` pre-allocates short-lived
hit/range/lifetime effect meshes. V10 impact records are deduplicated by
`event_id` in `client/src/app/authoritySnapshotProjector.ts` through
`client/src/netcode/seenIdRing.ts`, a fixed `Uint32Array` ring, before they reach
the scene. Spawning an effect only flips pooled mesh state and sets scalar
position/scale fields, and removes the impacted projectile from the projectile
pool. No geometry, material, array, or Three.js vector is allocated in the render
loop.

## Movement Authority

The wire `move` packet carries a click target and target tick, not an
authoritative position. RAD queues the input by tick, clamps the target to the
map, and applies it by emitting `MoveOrder`. `MoveOrder` updates `MoveTarget`
only. `Position` changes later in `tick_avatar_movement` by
`MoveSpeed.units_per_sec * dt`, so a packet with a far target cannot teleport an
avatar across the map.

Browser command sequencing lives in `client/src/netcode/inputSequencer.ts`.
That module owns monotonically increasing command IDs, non-wrapping
live-session `client_seq` values, target-tick scheduling, prediction-lead
diagnostics, and the sent-input counter. `client/src/app/clientCommandDispatcher.ts`
asks it to write command reservations into caller-owned scratch before recording
prediction or sending datagrams, so DOM/UI glue does not own netcode timing
state. The POC does not fake sequence wrap support: exhausting the client
sequence range must start a new match session or upgrade the protocol's ACK
math.

Raw browser input lives in `client/src/app/clientInputController.ts`. It binds
and unbinds DOM listeners, remembers the latest pointer coordinates, owns the Q
aiming state, decodes debug-toggle events, and emits clean move/cast intent
callbacks. `MobaRadClient` turns those canvas coordinates into world-space
intent only; it does not own key/pointer lifecycle.

Fresh command dispatch lives in `client/src/app/clientCommandDispatcher.ts`. It
records move/cast commands in the shared prediction ring, schedules the first
retransmit window, and asks `ClientInputTransport` for the fresh datagram send.
Resend cadence then lives in `client/src/app/clientInputTransport.ts`. It sends
move/cast datagrams through the dumb `MatchTransport`, reuses one
`PredictionInputSnapshot` scratch record for resend selection, and chooses the
oldest still-live input missing from the receipt ACK window. `MobaRadClient`
records scene-derived intent; it does not own DOM lifecycle or packet retry
policy.

Rollback move replay lives in `client/src/netcode/predictedMoveApplier.ts`.
That module owns the re-arm step for already-applied move inputs before a
correction replay, then ticks the local RAD session forward through the rollback
window. Cast records stay in the prediction ring for ACK/resend, but they are
not replayed as movement.

## Static Terrain

Static map collision is defined in `server/src/world/scene.rad` and resolved in
the shared RAD movement system. The authority and browser predictor therefore
use the same collider coordinates and slide-correction math. Move targets that
land inside an expanded wall are snapped to the nearest free edge, and each
movement step resolves the colliding axis while preserving the free axis.

The browser reads the collider fields from the cold `MobaScene` resource and
renders matching wall meshes in `MobaRadScene`; those meshes are visual-only and
never drive physics.

## Peer Ownership

Every accepted input belongs to a remembered peer entity. The edge proxy still
does not know this grammar; it forwards bytes only. RAD parses the packet,
resolves or creates the `PeerConnection`, writes into that peer's
`PeerInputRing`, and snapshots only that peer's `player_id`. This keeps
transport identity, gameplay ownership, and visual selection from collapsing
into one global "controlled avatar" shortcut.

`server/src/transport/udp_match.rad` owns byte-level UDP packet dispatch and
peer response selection only. It centralizes packet-served status emission,
cold-join `full-sync` snapshot selection, player-conflict rejection, and
peer-table-full rejection. The fixed tick, input validation, replay logging, and
simulation work stay in `server/src/server/*` and `server/src/sim/*`; the
transport handler does not apply movement directly.

RAD rejects a second active session trying to claim the same `player_id`.
Within a match, `player_id` is gameplay ownership, not just a display filter.

Idle peers are also owned by RAD. `cleanup_udp_peers()` expires old
`PeerConnection` entities, despawns the avatar identified by
`PlayerControlled.player_id`, removes that player's lag-compensation history,
increments `ServerStats.peer_expirations`, and lets the next `sync` or `move`
recreate the peer without teaching the edge proxy any game protocol.

Graceful leave uses the binary disconnect packet. The browser sends it
best-effort during `MobaRadClient.dispose()`, then closes WebTransport. RAD owns
the actual peer/avatar cleanup and increments `ServerStats.peer_disconnects`.

Cold join is also RAD-owned. If a `sync`, `move`, or `cast` packet creates a new
`session_id:player_id` peer (including reconnect after expiration), the response
uses `full-sync` status and `encode_full_state_packet(...)`. That snapshot
temporarily bypasses vision-radius culling, still respecting the configured MTU
record cap, so the browser can rebuild remote avatar/projectile registries and
pooled meshes before normal interest-managed snapshots resume.

## Authority Lifecycle

The live RAD server loop is controlled by `ServerControl` in
`server/src/server/state.rad`. `start_authority(now_ms)` records the start time
and marks the authority running. `request_authority_shutdown(reason, now_ms)`
emits `AuthorityShutdownRequested`, flushes it immediately, records the reason
and shutdown time, and flips the run flag to false.

`server/src/main.rad` checks `authority_should_run()` as the loop guard. On a
graceful exit it runs one last peer cleanup pass, finalizes the deterministic
replay tape, calls `udp_close(udp_socket)`, and prints the recorded shutdown
reason. This keeps native socket lifetime in RAD-owned server lifecycle code
rather than the WebTransport edge proxy.

## Deterministic Replays

`server/src/server/replay_log.rad` writes `moba-rad-match.replay` for live
authority runs. It captures the initial world/digests once, then appends one
compact line per move or cast input when that input is actually consumed by the
fixed-tick queue. The replay resource is transient, so logging metadata does not
change `world_digest()`.

See [Deterministic Replays](./replays.md) for the file format.

## Receipt ACKs And Applied Progress

The state packet carries two different progress signals. `ack_client_seq` and
`ack_bits` are a receipt ACK window: they advance when a well-formed packet
reaches the RAD peer boundary, even if the target tick is still in the future
or the packet is rejected as late/too-far. `client/src/netcode/ackDiagnostics.ts`
inspects that 32-packet receipt window, counts missing sequence ids, maintains a
loss ratio, and raises the target input delay when loss or RTT/jitter spikes.

`client/src/app/clientInputTransport.ts` also uses the receipt ACK window to
resend the oldest missing or future, still-live input command at a bounded
cadence. Move commands resend through `MatchTransport.sendMoveOrder()`, and
cast commands resend through `MatchTransport.sendCast()` from the same
caller-owned scratch record. Prediction history is not cleared from that receipt
ACK.

The RAD input queue treats a sequence older than both 32-packet ACK windows as
duplicate/noise before it can mutate `PeerInputRing` or `PeerCastRing`. That
prevents a hostile packet from reusing an ancient `client_seq` with a fresh
future `target_tick` to bypass duplicate detection.

Rollback cleanup uses the applied ACK window: `last_applied_client_seq` plus
`applied_ack_bits`. The high watermark says the latest sequence the fixed tick
has consumed, and the bitfield proves which lower sequences in the 32-packet
window were also consumed. That prevents a later-applied sequence from deleting
a lower-sequence input that is still needed for local replay.

## Snapshot Backpressure

WebTransport datagrams can arrive faster than the app asks for the next
authority state. `client/src/transport/stateInbox.ts` keeps those parsed
snapshots in a fixed-size ring, and `MobaRadClient` drains the freshest
available state from `MatchTransport.latestState()` once per frame. Explicit
`MatchTransport.state()` remains only for sync/poll packets. Move and cast
inputs are datagram sends, not state-awaiting RPC calls. Older queued snapshots
are deliberately discarded because `MobaRadClient` already uses `server_seq` to
reject stale authority; feeding an old backlog into reconciliation would add
latency without improving correctness.

Sync/poll responses are ACK-qualified, not queue-qualified. Before sending a
sync packet, the transport discards any queued inbox states and registers a
waiter that only resolves when a state packet for the same session/player has a
receipt ACK window covering the sync packet's `client_seq`. This keeps RTT and
jitter samples tied to the actual RAD authority response instead of whatever
snapshot happened to be buffered in the browser.

`client/src/app/clientAuthorityRequester.ts` owns when those sync/poll requests
are sent, whether one is already in flight, and how successful responses update
RTT/jitter diagnostics. It returns accepted-by-transport `ServerState` packets
to `MobaRadClient`; snapshot freshness, wrong-session rejection, and rollback
still happen later through `AuthorityStateGate` and reconciliation.

The inbox is transport-local. It does not parse packet fields, decide gameplay,
or affect resend/rollback state. It only bounds memory and prevents old
snapshots from becoming a hidden queue between RAD authority and browser
prediction. `ServerStateInbox.droppedCount` includes both capacity overwrites
and older queued snapshots discarded when the app drains the latest state, and
snapshots explicitly discarded before a sync wait. The HUD can show when the
browser is shedding stale authority data to preserve latency.

The WebTransport read loop hands state datagrams to
`client/src/transport/webTransportStateRouter.ts`, which parses them into a
fixed pool of `ServerState` containers. `matchProtocol.ts` owns the wire grammar
and can write into caller-provided state storage; the router rotates through a
pool larger than the inbox so 128Hz snapshots do not allocate a new JS object
graph per datagram. Gameplay still receives normal `ServerState` objects, but
their lifetime is transport-owned and bounded by the latest-state queue.

Sync waiter replies get a separate bounded state pool inside the same router.
When a parsed datagram ACKs the requested sync sequence, the router copies it
into the waiter's buffer before resolving the promise. That prevents later
datagrams from mutating the rotating parse-pool object while `MobaRadClient` is
still crossing the `await` boundary to apply the authority state.

## Reconnect Hygiene

`client/src/transport/webTransport.ts` treats the WebTransport session as a
replaceable browser handle. If `transport.closed` resolves or rejects, or if the
datagram read loop ends, it asks `webTransportStateRouter.ts` to reject pending
sync waiters and clear stale inbox state, then drops the cached writer/transport
references and resets the write chain.
The next `sendMoveOrder()`, `sendCast()`, or `state()` call then reconnects
through the same WebTransport URL and certificate settings.

This is deliberately below gameplay. The transport does not guess which inputs
to replay and does not mutate protocol fields; `PredictionBuffer`,
`AckDiagnostics`, `ClientInputTransport`, and the RAD authority still own resend
and reconciliation.

## Live Diagnostics

The browser page includes a low-frequency netcode HUD wired through
`client/src/ui/netcodeHud.ts`. It does not read private fields directly and it
does not allocate in the animation loop. Instead, `MobaRadClient` exposes
`writeNetcodeDiagnostics(out)`, and `client/src/app/clientNetcodeTelemetry.ts`
writes the scalar RTT/jitter, correction, resend, transport-failure,
server-peer, and roster/projectile counters into the caller-owned
`NetcodeDiagnosticsSnapshot`.

The HUD shows:

- authority connection phase (`booting`, `syncing`, `live idle`, `live moving`,
  or `offline`)
- local/client tick versus server tick estimate
- smoothed sync/poll request RTT and jitter observed through the authority
  request path
- current prediction lead in ticks
- highest acked client sequence and observed ack-window loss
- accepted snapshots versus total snapshots, plus stale snapshot rejections
- dropped snapshots discarded by the bounded latest-state inbox
- authoritative reconciliation rate, total correction count, and
  visual-smoothing correction count
- input resend packets and transport failures
- server-authored peer count/capacity, input queue capacity, pending move/cast
  counts, late/future/duplicate/overwrite counters, last seen/applied client
  sequences, peer record count, and applied ACK bits
- remote/avatar roster record count plus projectile snapshot and impact counts
- active/idle remote-avatar, projectile, and impact-effect pool counts
- latest server status and correction reason

This makes live movement debugging possible without adding stringly logs to the
transport layer. The edge proxy still forwards opaque bytes only; all packet
meaning remains in RAD and the mirrored TypeScript packet module.

## Periodic Netcode Reports

`client/src/netcode/netcodeLogger.ts` is the low-noise run logger. It samples
the same caller-owned `NetcodeDiagnosticsSnapshot` as the HUD, accumulates by
simulated client tick, and writes one compact line every 128 ticks instead of
logging raw packet events.

The logger is opt-in. Normal play and manual movement tests keep diagnostics in
the HUD only; `MobaRadClient` does not construct or sample the logger unless the
browser client is started with `VITE_MOBA_RAD_NETCODE_LOG=1`. That keeps console
I/O and report formatting out of the requestAnimationFrame path while preserving
the same reporting tool for chaos/soak runs.

Example interval:

```text
[00:10] Ticks: 128 | Ping: 8ms (jit: 3ms) | Loss: 0.0% | Reconciles: 0/128 (0.0%) | LateInputs: 0 | Meshes: A_act:1/64 P_act:0/96
```

Read it this way:

- `Reconciles` is the prediction health signal. Anything above zero means the
  local predicted simulation diverged from authority during that tick window.
  The interval rate is capped at `128/128`; if multiple correction events land
  in the same tick window, the logger appends `CorrectionEvents` with the raw
  event count.
- `LateInputs` is server-authored queue pressure. Non-zero means input arrived
  after its target tick had already simulated; raise input lead or inspect
  latency/chaos settings.
- `Meshes` reports active pool use versus capacity, so team-fight visibility
  spikes do not silently allocate new Three.js resources.

When the client disposes, the logger writes one final `NETCODE REPORT` block
with duration, average ping/jitter, ack-window packet loss, total corrections,
maximum correction distance, input queue health, stale snapshot count, and peak
avatar/projectile pool usage. This is the artifact to keep from a 10-second
chaos run.

## Local Correction Smoothing

Authoritative reconciliation is immediate in the RAD session: the client emits
`AuthoritativeState`, replays unacknowledged local inputs, and refreshes the hot
WASM render buffer before the next frame. The visual mesh is the only thing that
eases. When a predicted position differs from the server by more than
`RECONCILE_ERROR_EPSILON`, `client/src/netcode/reconciliationPolicy.ts`
classifies it as a smooth visual correction and `MobaRadClient` asks
`MobaRadScene` to start a short local correction blend.
`client/src/render/correctionSmoother.ts` then blends from the already-rendered
mesh position toward the corrected RAD position over `LOCAL_CORRECTION_SMOOTH_MS`.

That boundary keeps the authority honest: gameplay state snaps to the server and
replays deterministically, while Three.js only hides the visual discontinuity.

## Hot-Path Allocation Rules

- `client/src/netcode/predictionBuffer.ts` uses typed arrays, not object queues.
- `client/src/netcode/authorityStateGate.ts` owns authority snapshot filtering,
  u32 protocol-field guards, local-avatar finite/boolean guards, stale sequence
  rejection, and state counters, so app/render code only sees accepted
  snapshots.
- `client/src/app/clientAuthorityApplier.ts` owns accepted authority snapshot
  application: receipt ACK updates, visual projection, clock sync,
  applied-input cleanup, reconciliation decisions, correction signaling, and
  prediction-runner replay.
- `client/src/netcode/reconciliationPolicy.ts` owns rollback/correction
  decisions as scalar math and writes into caller-owned decision scratch inside
  the authority applier.
- `client/src/app/clientInputController.ts` owns DOM listener lifecycle, latest
  pointer coordinates, Q aiming state, resize/debug dispatch, and canvas-space
  move/cast intent callbacks.
- `client/src/netcode/inputSequencer.ts` owns command IDs, non-wrapping
  live-session client sequences, target-tick monotonicity, and sent-input
  diagnostics while writing reservations into caller-owned scratch.
- `client/src/app/clientAuthorityRequester.ts` owns ACK-qualified authority
  sync/poll cadence, one in-flight request guard, and RTT/jitter timing samples
  while reusing one ACK diagnostics scratch record.
- `client/src/app/clientCommandDispatcher.ts` owns move/cast command
  reservation, prediction-ring recording, first retransmit scheduling, and
  fresh-send dispatch while reusing one input-reservation scratch record.
- `client/src/app/clientInputTransport.ts` owns fresh input datagram sends,
  bounded resend cadence, oldest-unacked move/cast selection, and
  transport-failure telemetry while reusing one resend scratch record.
- `client/src/app/clientPredictionRunner.ts` owns local RAD fixed-tick stepping,
  the simulated-tick frontier, per-tick prediction samples, scene avatar samples,
  and authoritative replay after correction.
- `client/src/netcode/predictedMoveApplier.ts` owns due-move application plus
  rollback-window re-arming/replay mechanics, using one reusable input scratch
  record inside the prediction runner.
- `client/src/app/authoritySnapshotProjector.ts` owns accepted authority
  snapshot projection into remote avatars, authority ghosts, projectile meshes,
  and impact effects, reusing scratch structs outside the app coordinator.
- `client/src/app/clientNetcodeTelemetry.ts` owns scalar RTT/jitter, correction
  rate, resend, transport-failure, server-peer, and roster/projectile diagnostic
  counters, then writes them into caller-owned snapshots.
- `client/src/netcode/seenIdRing.ts` owns fixed-ring event dedupe for projectile
  impacts, keeping duplicate impact records out of scene code without per-frame
  allocation.
- `client/src/netcode/ackDiagnostics.ts` keeps packet-loss accounting outside
  the render and transport layers and can update caller-owned snapshots without
  allocating on accepted state packets.
- `client/src/netcode/predictionBuffer.ts` exposes oldest-unacked input data by
  writing into caller-owned scratch, not by allocating resend objects.
- `client/src/netcode/predictionBuffer.ts` also tracks whether each move input
  has been applied by local prediction, so skipped target ticks are recovered
  without double-applying commands or dropping rollback history.
- `client/src/netcode/runtimeDiagnostics.ts` defines caller-owned telemetry
  snapshots for HUD/debug surfaces; `client/src/ui/netcodeHud.ts` updates only
  on a 250 ms interval, never from the requestAnimationFrame path.
- `client/src/netcode/netcodeLogger.ts` is opt-in and is not constructed by
  normal play, so periodic report formatting stays out of the RAF hot path.
- `server/src/server/input_queue.rad` keeps move/cast duplicate, late,
  too-far-ahead, queued, expired, and applied-ACK accounting in shared helpers,
  so the fixed tick loop sees compact ring state instead of transport-specific
  branches.
- `client/src/netcode/fixedTickClock.ts` returns scalar tick counts.
- `client/src/transport/webTransport.ts` reuses packet buffers and
  serializes writes so binary datagrams do not race shared buffers.
- `client/src/transport/matchWire.ts` owns endian and fixed-point primitives,
  keeping byte math out of transport lifecycle and app coordination.
- `client/src/transport/serverStateBuffer.ts` owns reusable `ServerState`
  object graphs and copy helpers for parse pools and sync-waiter buffers.
- `client/src/transport/webTransportStateRouter.ts` owns parsed-state pools,
  ACK-qualified sync waiter buffers, and routing into the bounded latest-state
  inbox.
- `client/src/transport/stateInbox.ts` bounds parsed snapshot backlog and
  drains the latest state without `Array.shift()` churn.
- `client/src/render/avatarInterpolator.ts` interpolates caller-owned
  `AvatarRenderState` structs without allocating in RAF.
- `client/src/render/correctionSmoother.ts` stores correction blend state as
  scalars and writes into caller-owned render state.
- `client/src/render/avatarTimeline.ts` stores fixed-size historical samples
  for remote avatars and scans them without sorting or allocating in RAF.
- `client/src/scene.ts` reuses Three.js vectors for click raycasts.
- `client/src/render/avatarMeshPool.ts` pre-warms remote avatar meshes so peer
  spawn/despawn reuses hidden meshes instead of compiling shaders or allocating
  in RAF.
- `client/src/render/impactEffectPool.ts` pre-warms hit/range/lifetime effect
  meshes and reuses scalar mesh state for v10 projectile impact records.
- `client/src/scene.ts` disposes geometries, materials, and the WebGL renderer
  through `MobaRadScene.dispose()`, draining the mesh pool on teardown.
- `client/src/radHost.ts` caches the fixed tick event payload and refreshes the
  hot avatar render state from a direct WASM-memory `Float32Array` view.
- `session_render_delta()` remains only for cold resource/config bootstrap; it
  is not used for movement-frame avatar refreshes.

## Direct WASM Render Bridge

The local browser RAD session exposes hot avatar render state through
`RadRuntime.session_render_buffer_refresh()`, `session_render_buffer_ptr()`, and
`session_render_buffer_f32_len()`. Rust owns the backing `Vec<f32>` inside
`RadRuntime`; JS reads a `Float32Array` view over the same WASM linear memory.
The view is rebuilt only if the pointer, length, or `memory.buffer` identity
changes.

Layout:

```text
[0] version = 1
[1] stride = 9
[2] count

per avatar record:
  [0] entity_id
  [1] player_id
  [2] x
  [3] y
  [4] target_x
  [5] target_y
  [6] target_active
  [7] command_id
  [8] model_code
```

`client/src/radHost.ts` applies those scalar records into stable `RadEntity`
objects already owned by the browser cache. Combined with pooled binary
snapshot parsing in `client/src/transport/matchProtocol.ts`, the existing
prediction, reconciliation, and `worldView` paths keep working without parsing
JSON or allocating a fresh network state graph in the movement loop.

Boundaries this preserves:

- The RAD VM still owns the data: JS gets a read-only numeric window, never write
  access to authoritative state.
- Visual/render state stays downstream: meshes read the table, the table never
  reads meshes.
- Cold scene resources still use the incremental JSON render delta until they
  have their own typed resource ABI.

## Test Coverage

The authority netcode runs without sockets in
`server/src/test/netcode_smoke.rad`, driven by `npm run test:netcode` (and the
umbrella `npm test`). It exercises the same functions the UDP loop calls:

- `remember_peer` creates exactly one bounded peer per `session_id:player_id`
  and reuses it on repeat packets.
- `queue_move_input` accepts a fresh future input and rejects the duplicate,
  late, and too-far-ahead cases, advancing the matching diagnostic counters.
- `queue_cast_input` uses the same target-tick rejection helpers as movement,
  then rejects zero-length cast vectors before touching the cast ring.
- `apply_peer_move_for_tick` applies the matching tick, acks the client seq,
  sets the low ack bit, and drives the shared movement target end to end.
- The peer table saturates at `max_peers()` and rejects further peers.

Keep this suite green when changing the jitter buffer, ack bookkeeping, or peer
table, since the live authority loops forever on a real UDP handle and cannot be
asserted against directly.

`server/src/test/movement_smoke.rad` (`npm run test:movement`) locks the
movement authority and the anti-teleport invariant: a single tick advances
`Position` by at most `MoveSpeed * dt`, and an out-of-bounds move target is
clamped into the map plane and still resolves to a bounded step rather than a
teleport. It also locks the default MTU-derived avatar cap and verifies that a
far-away player is omitted from a viewer's interested roster.

`server/src/test/input_flood_smoke.rad` (`npm run test:flood`) is the DDoS /
buffer-overflow guard for the per-peer input ring: it floods a peer with a 64-
packet same-tick storm (must collapse to one slot, latest intent wins) and a
50-tick burst past the lead window (only the in-window ticks may queue, the rest
are rejected as too-far), proving the fixed ring never overflows, leaks, or
panics under a packet storm.

`server/src/test/collision_smoke.rad` (`npm run test:collision`) dogfoods the
shared static-collision resolver in `sim/movement.rad`. Because the authority and
the browser predictor run that exact code against the same `world/scene.rad`
collider data, the invariant holds identically on both ends: targets inside
terrain are corrected to a free edge, a blocked axis stays blocked, the free
axis slides, and the avatar never ends a tick inside terrain.

`server/src/test/shutdown_smoke.rad` (`npm run test:shutdown`) covers the
authority lifecycle resource without opening sockets: start records a running
authority and shutdown records the stop time/reason while flipping the loop guard
off. The live server's `main.rad` is the only place that owns the UDP handle and
therefore the only place that calls `udp_close`.

On the client, `client/src/render/avatarInterpolator.ts` has node unit coverage
in `client/test/avatarInterpolator.test.ts` (`npm test` in `client/`). That test
runs without Three.js because the interpolator and `worldView` depend only on
`render/avatarModelId.ts` for the default model id; the Three.js mesh factories
in `render/avatarModels.ts` are kept out of the pure render-state graph, so the
visual-smoothing math is asserted in isolation.

`client/test/correctionSmoother.test.ts` covers local correction smoothing
without Three.js: the smoother starts from the already-rendered mesh position,
smoothsteps toward the corrected authoritative point, and expires at the end of
the blend window.

`client/test/predictionBuffer.test.ts` audits the prediction ring against the
worst-case chaos RTT: it asserts the ring is a power of two with >= 2x RTT
headroom, that a snapshot delayed by the full RTT still reconciles with ~zero
error, that a lapped tick reports stale instead of mis-reconciling, and that the
selective ack window surfaces the oldest unacked input for resend.

`client/test/avatarTimeline.test.ts` covers remote-entity interpolation,
including out-of-order samples. `client/test/matchProtocol.test.ts` covers the
binary state roster tail that feeds those timelines.

`server/src/test/projectile_smoke.rad` (`npm run test:projectile`) covers the
projectile/lag-compensation gameplay slice: position-history rollback, the
hit-at-view-tick / miss-at-current-tick pivot that makes lag compensation
meaningful, fixed-velocity travel, lifetime/out-of-bounds reclaim, and
`ProjectileHit` telemetry. It also asserts the 64-sample lag history covers the
maximum chaos RTT rewind budget. The live authority clock records history, advances
projectiles, and resolves hits each fixed tick. See
[Projectiles & Lag Compensation](./projectiles.md).

`server/src/test/replay_smoke.rad` (`npm run test:replay`) covers deterministic
replay tape logging and playback reset. It records a scripted fixed-tick input
stream, finalizes the file, asserts the header, initial-world block, applied
input lines, footer, and input count, then uses `load_world` to hard-reset before
replaying the same stream to the same authoritative avatar state.

The whole prediction/interpolation/ack pipeline is stress-tested against bad
networks by the edge proxy's debug chaos emulator (`server/edge-proxy/src/chaos.rs`):
opt-in latency, jitter, and packet-loss injection on both datagram directions,
fully bypassed unless an env knob is set. Its pure decision model (loss roll,
clamped jitter, non-negative delay) is unit tested via
`cargo test --manifest-path edge-proxy/Cargo.toml`; see the
[runbook](./runbook.md) for the env knobs and what "silky under
`+120ms / 5% loss`" should look like.

## Still Missing

- Typed WASM-memory resource/config reads; avatar movement already uses direct
  WASM-memory render reads.
- Server-side resend policy tuning beyond the ack-window loss, resend,
  stale-snapshot, transport-failure, v10 peer records, and input rejection
  counters.
- Server-side collision against creeps and command legality beyond map-plane
  clamping, static terrain sliding, speed-limited target movement, and
  projectile hitboxes.
- Standalone replay playback tooling that parses `moba-rad-match.replay`,
  feeds the recorded `M`/`C` stream automatically, and verifies the final digest.
