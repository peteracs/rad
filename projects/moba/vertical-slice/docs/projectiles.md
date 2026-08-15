# Projectiles & Lag Compensation

The first gameplay element on top of the movement/netcode foundation: skillshot
projectiles resolved with server-authoritative lag compensation. The simulation
is live in the RAD authority fixed-tick loop. Browser clients can send compact
cast packets and receive authoritative projectile plus projectile-impact records
in state snapshots; client-side projectile prediction spawns pooled meshes
immediately, advances them locally at projectile speed, and correlates them with
authoritative projectile ids when snapshots arrive.

## Files

| File | Responsibility |
|---|---|
| `server/src/sim/projectiles.rad` | `Projectile` component, `ProjectileHit`/impact log telemetry, spawn, fixed-velocity ticking, lifetime/out-of-bounds reclaim, and lag-compensated hit resolution. |
| `server/src/sim/lag_compensation.rad` | `PositionHistory` ring entities keyed by `player_id`, per-tick recording, and tick-indexed rollback queries. |
| `server/src/server/clock.rad` | Live fixed-tick integration: apply queued inputs, flush movement, record history, advance projectiles, resolve hits. |
| `server/src/server/input_queue.rad` | Peer expiration/disconnect cleanup for player avatars and position-history entities. |
| `server/src/protocol/match_protocol.rad` | Cast packet decode plus projectile and projectile-impact records in state snapshots. |
| `client/src/transport/matchProtocol.ts` | Browser cast packet encode plus projectile and projectile-impact record parse. |
| `client/src/transport/matchWire.ts` | Shared browser wire primitives for fixed-point projectile coordinates. |
| `client/src/transport/serverStateBuffer.ts` | Reusable projectile and impact records for pooled snapshot parsing. |
| `client/src/scene.ts` | Projectile mesh pool, impact effect pool, local visual prediction, authoritative correction, and cleanup for predicted/authoritative projectiles. |
| `client/src/netcode/seenIdRing.ts` | Fixed-ring `event_id` dedupe before impact records reach scene code. |
| `client/src/render/impactEffectPool.ts` | Pre-warmed hit/range/lifetime visual effects driven by v10 impact records. |
| `server/src/test/projectile_smoke.rad` | `npm run test:projectile` — covers rollback, the hit/miss pivot, projectile travel, lifetime/out-of-bounds reclaim, and hit telemetry. |

## Projectile Lifecycle

1. `cast_projectile(owner_id, dir_x, dir_y, spawn_tick, fire_view_tick)` spawns
   an entity carrying `Position` (at the owner) plus `Projectile`. The direction
   is normalized to a fixed `projectile_speed()` velocity, so the client never
   dictates speed.
2. `tick_projectiles(dt, server_tick)` advances each projectile by
   `velocity * dt`. A projectile that leaves the playable plane is despawned
   (reclaimed, not clamped, so it cannot hug the boundary), and one that outlives
   `projectile_lifetime_ticks()` is despawned even while in bounds.
3. `resolve_projectile_hits(server_tick)` tests each projectile against every
   non-owner `PlayerControlled` avatar using a lag-compensated circle check. The
   first confirmed hit emits `ProjectileHit`, despawns the projectile, and (via
   the `on ProjectileHit` handler) advances `ProjectileStats`. Hit, range, and
   lifetime cleanup also write fixed-stride impact records for v10 state
   snapshots.

All hot paths reuse fixed entity/ring storage and per-entity helpers with early
returns, so neither ticking nor resolution allocates in the 128 Hz loop.

## Lag Compensation

An esports authority cannot trust the client's view of where a target *is*, but
it must honor where the shooter *saw* the target when they acted. Each tick the
authority records every player's authoritative position into a `PositionHistory`
ring (64 ticks, 500 ms at 128 Hz) keyed by `player_id`. The proxy chaos profile
can emulate roughly 300 ms RTT (`+120ms` base latency plus jitter on both
directions), so the retained rewind depth is intentionally above the 40-45 tick
high-ping budget and is asserted by `server/src/test/projectile_smoke.rad`.

When resolving a hit, the target is rolled back to `historic_slot_for_tick`: the
newest recorded sample at or before the shooter's `fire_view_tick` (their
interpolated render tick). Rollback never reads a *future* tick, so a laggy
shooter is never credited with information they could not have seen. If the view
tick predates the retained window, the query falls back to the current position
rather than fabricating one.

The smoke test pins the invariant that makes lag compensation meaningful: the
same aim point hits when judged at the shooter's view tick (target was there) and
misses when judged at the newer authoritative tick (target has since moved):

```text
target at tick 30: (30, 0)   <- shooter's view tick
target at tick 40: (140, 0)  <- current authoritative position
projectile at (30, 0):
  lag-compensated (view tick 30) -> HIT
  naive (current tick 40)        -> would MISS
```

This keeps visual render state, predicted state, and authoritative collision
strictly separated: the scene graph never feeds collision, and collision never
trusts a client-reported position.

## Authority Integration

The simulation is integrated in the per-tick block of `pump_authority_clock`
(`server/src/server/clock.rad`), after the movement handler has resolved for
that tick:

```text
apply_queued_move_for_tick(next_tick)
emit Tick { dt: server_tick_dt() }
flush_events()
record_all_history(next_tick)                 // capture post-movement positions
tick_projectiles(server_tick_dt(), next_tick) // advance live projectiles
resolve_projectile_hits(next_tick)            // lag-compensated collision
```

Peer cleanup removes the matching `PositionHistory` entity before despawning the
avatar, so rollback state cannot outlive a disconnected or expired owner.

## Impact Effects

V10 state snapshots carry fixed-stride projectile-impact records. The client
projects accepted snapshots through
`client/src/app/authoritySnapshotProjector.ts`. That adapter deduplicates impact
records by `event_id` through `SeenIdRing`, removes the matching projectile mesh
from the projectile pool, and spawns a pre-warmed impact effect from
`ImpactEffectPool`.

The effect is visual-only. `hit`, `range`, and `lifetime` records can choose
different presentation settings, but they do not feed back into prediction,
collision, or authority state. The current render path advances only scalar
age/scale values on pooled meshes.

## Still Missing

- Damage, cooldowns, and resource costs behind `ProjectileHit`.
- Interpolated (sub-tick) rollback between two history samples for casts that
  land between recorded ticks; today rollback snaps to the newest prior sample.
- Per-projectile collision against terrain/creeps beyond player avatars.
