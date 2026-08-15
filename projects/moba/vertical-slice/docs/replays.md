# Deterministic Replays

RAD MOBA replay logging is authority-owned. The WebTransport edge proxy stays a
dumb byte forwarder, and the browser never authors replay state.

## Contract

The server records:

- the replay format version,
- authority start time,
- `schema_digest()`,
- initial `world_digest()`,
- the full initial `save_world()` payload,
- every accepted player input at the exact simulation tick where it executed,
- final input count and final `world_digest()`.

The input stream is compact line protocol:

```text
M <tick> <session_id> <player_id> <client_seq> <command_id> <target_x> <target_y>
C <tick> <session_id> <player_id> <client_seq> <command_id> <dir_x> <dir_y> <fire_view_tick>
```

Only applied inputs are recorded. Packets that are rejected, expired, duplicate,
or too far ahead never enter the replay tape because they never affected
simulation.

## Files

| File | Responsibility |
|---|---|
| `server/src/server/replay_log.rad` | Transient replay resource, header/footer writes, and applied move/cast record helpers. |
| `server/src/server/input_queue.rad` | Calls replay helpers only when fixed-tick move/cast inputs are consumed. |
| `server/src/main.rad` | Starts the default replay file for live authority runs and finalizes it before closing UDP. |
| `server/src/test/replay_smoke.rad` | Verifies header, input lines, footer, input count, and deterministic playback after `load_world` reset. |

The replay resource is `transient`, so recording metadata does not move
`world_digest()` and does not become part of the deterministic match identity.

## Output

Live server runs write:

```text
moba-rad-match.replay
```

from the server working directory. Tests use a temporary local replay file and
remove it before exiting.

## Playback

Playback loads the recorded initial world with `load_world`, then feeds each
`M`/`C` line back through the same fixed-tick input path at its recorded tick.
`load_world` hard-resets the world — it replaces the live entity set with the
saved one rather than appending to it — so a replay started inside a live process
cannot drift on leftover entities. Because the authority simulation is then
deterministic, the replayed `world_digest()` matches the tape's `final_digest`
exactly. The smoke suite asserts both: the authoritative avatar position and the
full whole-world digest convergence.
