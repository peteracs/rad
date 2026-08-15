# Protocol Ownership

The current packet grammar is `moba-rad/udp-v10-peer-snapshot`. Every packet begins
with:

```text
byte 0: magic 0x4d
byte 1: version 10
byte 2: kind
```

Numeric fields are little-endian u32/i32. Coordinates are signed fixed-point
integers with 1000 units per world unit.

Single source of truth:

| Side | File |
|---|---|
| RAD authority | `server/src/protocol/match_protocol.rad` |
| Browser client | `client/src/transport/matchProtocol.ts` |
| Edge proxy | no grammar; forwards bytes only |

Browser support modules stay separate from grammar ownership:

| File | Owns |
|---|---|
| `client/src/transport/matchWire.ts` | little-endian and fixed-point wire primitives |
| `client/src/transport/serverStateBuffer.ts` | reusable `ServerState` buffers and record copy/resize helpers |

## Rules

- Packet evolution must happen in the RAD protocol module and the matching
  TypeScript protocol module together.
- The edge proxy must not branch on packet kinds.
- The project must not add a second JSON/HTTP protocol.
- Browser and native clients must share the same packet bytes.
- Binary packets must use `udp_recv_bytebuf_timeout` and `udp_send_bytebuf` on
  the RAD authority side.

## Packet Layout

| Kind | Code | Bytes | Fields after header |
|---|---:|---:|---|
| Sync | 1 | 15 | `client_seq`, `session_id`, `player_id` |
| Move | 2 | 31 | `client_seq`, `session_id`, `player_id`, `target_tick`, `command_id`, `target_x_i32`, `target_y_i32` |
| Disconnect | 3 | 15 | `client_seq`, `session_id`, `player_id` |
| State | 4 | `92 + 44 * peer_record_count + 26 * avatar_count + 36 * projectile_count + 25 * impact_count` | local authoritative fields, status/correction, roster counts, fixed authority telemetry, peer records, then avatar/projectile/impact records |
| Error | 5 | 4 | `status_u8` |
| Cast | 6 | 35 | `client_seq`, `session_id`, `player_id`, `target_tick`, `command_id`, `dir_x_i32`, `dir_y_i32`, `fire_view_tick` |

State packets keep the local player's authoritative fields first so the client
can reconcile without scanning the roster tail. The roster tail is for visual
interpolation of other entities and is interest-managed per viewer:

```text
state local fields:
  server_ms:u32
  server_tick:u32
  server_seq:u32
  session_id:u32
  player_id:u32
  ack_client_seq:u32       # highest client packet sequence received by RAD
  ack_bits:u32             # 32-bit receipt window for resend/loss diagnostics
  command_id:u32
  x_i32
  y_i32
  target_x_i32
  target_y_i32
  target_active_u8
  status_u8
  correction_reason_u8
  avatar_count_u8
  projectile_count_u8
  peer_count_u8
  max_peers_u8
  input_queue_slots_u8
  pending_move_inputs_u8
  pending_cast_inputs_u8
  peer_connected_u8
  projectile_impact_count_u8
  peer_record_count_u8
  late_inputs:u32
  future_inputs:u32
  duplicate_inputs:u32
  overwritten_inputs:u32
  last_client_seq:u32      # highest client sequence seen by this peer
  last_applied_client_seq:u32 # highest input/cast sequence consumed by the fixed tick
  applied_ack_bits:u32     # 32-bit applied-input window for rollback cleanup

peer record, repeated peer_record_count times:
  player_id:u32
  session_id:u32
  last_client_seq:u32
  received_client_seq:u32
  last_applied_client_seq:u32
  applied_ack_bits:u32
  pending_move_inputs_u8
  pending_cast_inputs_u8
  connected_u8
  reserved_u8
  late_inputs:u32
  future_inputs:u32
  duplicate_inputs:u32
  overwritten_inputs:u32

avatar record, repeated avatar_count times:
  player_id:u32
  command_id:u32
  x_i32
  y_i32
  target_x_i32
  target_y_i32
  target_active_u8
  model_u8

projectile record, repeated projectile_count times:
  projectile_id:u32
  owner_id:u32
  command_id:u32
  x_i32
  y_i32
  velocity_x_i32
  velocity_y_i32
  spawn_tick:u32
  fire_view_tick:u32

projectile impact record, repeated projectile_impact_count times:
  event_id:u32
  projectile_id:u32
  owner_id:u32
  target_id:u32
  x_i32
  y_i32
  reason_u8
```

`avatar_count` and `projectile_count` are not "everything in the match." RAD computes them for the
recipient by including the local avatar plus avatars inside
`ServerConfig.snapshot_interest_radius`, and by including projectiles owned by
the viewer or inside that same interest radius. It also caps the counts with
`max_state_avatar_records()`, derived from `ServerConfig.snapshot_mtu_bytes`, so
the encoded payload stays under the configured safe UDP budget.

## Implemented Reliability Fields

- `client_seq` is assigned by the browser for sync, move, cast, and disconnect
  packets. In this POC it is monotonic and non-wrapping for one live match
  session; sequence exhaustion is a session/protocol upgrade event, not a silent
  wrap to 1.
- `session_id` identifies one browser/native match connection.
- `player_id` identifies the avatar that owns the input and snapshot.
- The disconnect packet frees one `session_id:player_id` peer and its
  avatar/slot.
- `target_tick` tells the RAD authority when a move should enter simulation.
  Move and cast packets share the same RAD input-window validation before their
  target-tick rings are mutated.
- `server_tick` identifies the authoritative simulation tick in each snapshot.
- `server_seq` lets the client ignore stale or out-of-order snapshots.
- `ack_client_seq` and `ack_bits` report client packet receipt history after a
  packet reaches a valid peer. The browser uses this window for resend/loss
  diagnostics, not as proof that simulation has consumed the input.
- A client sequence older than the 32-packet receipt/applied windows is rejected
  as duplicate/noise before it can mutate a move or cast target-tick ring.
- `last_applied_client_seq` and `applied_ack_bits` report fixed-tick
  application progress. The browser clears prediction rollback history through
  this applied window, so a lower-sequence input is not deleted just because a
  later sequence happened to apply first.
- `correction_reason_u8` is server-authored and says why the authoritative
  snapshot was sent or why input was accepted/rejected.
- `full-sync` status is sent on cold joins, including reconnect after peer
  expiration, and uses an unculled roster within the same MTU budget so the
  browser can rebuild local registries and mesh pools before returning to
  interest-managed snapshots.
- `avatar_count` and the fixed-stride avatar records let the browser buffer
  non-local, in-interest entities in a historical interpolation timeline.
- `projectile_count` and fixed-stride projectile records let the browser render
  authoritative projectiles without parsing JSON.
- `projectile_impact_count` and fixed-stride impact records report server-owned
  hit/range/lifetime projectile cleanup reasons.
- `peer_count`, `max_peers`, queue slot capacity, pending move/cast counts,
  and input rejection counters are server-authored peer telemetry for live
  match debugging.
- `peer_record_count` and fixed-stride peer records expose each connected
  peer's queue pressure, receipt/applied sequence state, and rejection counters
  without teaching the WebTransport edge any packet grammar.
- The browser consumes receipt `ack_bits` through `AckDiagnostics` so packet-loss
  handling can stay outside the transport boundary.
- `client/src/app/clientInputTransport.ts` resends the oldest missing or
  future, still-live move/cast input from the `ack_bits` window at a bounded
  cadence without adding extra state waiters or teaching the WebTransport edge
  any gameplay semantics.
- The browser HUD reads `writeNetcodeDiagnostics(out)` to show ack loss,
  accepted/stale/rejected snapshots, correction counts, resend counts,
  peer/input telemetry, roster/projectile/impact record counts, and latest
  status/correction reason without teaching the edge proxy any packet grammar.

## Next Protocol Work

The next fields/features are:

- input payload flags
- authority-side resend policy tuning beyond the current ack window and
  rejection counters
- direct WASM-memory render/config views for remaining browser-side JSON reads

These fields belong in the packet grammar, not in transport-specific wrappers.
