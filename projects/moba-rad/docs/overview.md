# MOBA RAD

`projects/moba-rad` is the focused e-sports-grade movement dogfood project. It
keeps gameplay authority in RAD, rendering/input in the browser client, and
browser transport termination in a tiny host edge process.

The project root intentionally has only product folders:

| Folder | Purpose |
|---|---|
| `client/` | Vite + TypeScript + Three.js browser client |
| `server/` | RAD authority server plus WebTransport edge proxy |
| `docs/` | This project-local wiki |

## Current Goal

The POC proves the movement pipeline end to end:

1. Right-click input is sampled in the browser.
2. The client prediction runner advances the shared RAD movement source at fixed
   ticks and records per-tick prediction samples.
3. A focused client input transport sends a compact WebTransport datagram to
   the edge proxy and owns bounded resend of still-live unacked inputs.
4. The edge proxy forwards the payload to the RAD authority over localhost UDP.
5. The RAD authority validates and advances movement on a fixed tick.
6. The server sends state snapshots back through UDP and WebTransport.
7. The client authority applier reconciles accepted snapshots against local
   prediction and triggers rollback/replay when needed.
8. The client writes live netcode telemetry into a caller-owned snapshot so the
   page can show authority sync, tick drift, ack loss, stale snapshots,
   corrections, resends, server peer/input telemetry, roster records, and
   projectile/impact records.

The current protocol is `moba-rad/udp-v10-peer-snapshot`: compact fixed-layout byte
packets over WebTransport datagrams and RAD UDP bytebuf builtins. The edge proxy
still forwards opaque bytes; packet meaning stays in `server/src/protocol` and
`client/src/transport`.

The first playable entity is the role-named `player_1` avatar. Its current
visual model is `clockwork_mage`, selected through RAD `RenderAvatar` metadata
and a TypeScript avatar-model registry.

The protocol already carries `session_id` and `player_id`, and shared RAD
movement resolves avatars by `PlayerControlled.player_id`. The authority keeps
a bounded peer table in RAD (`PeerConnection` entities keyed by
`session_id:player_id`) and gives each peer fixed target-tick rings for move
and cast inputs.

## Boundaries

| Layer | Owns |
|---|---|
| Browser client | Three.js scene, DOM input controller, local prediction runner, command dispatch, ACK-qualified authority polling, input send/resend, authority snapshot application |
| WebTransport edge proxy | HTTPS, HTTP/3, QUIC, TLS, browser certificate/hash handling, byte forwarding |
| RAD authority server | packet grammar, ECS state, fixed ticks, movement validation, snapshots, UDP socket lifecycle |

Do not add game rules to the edge proxy. Do not add HTTP polling or JSON
fallback transports to this project.
