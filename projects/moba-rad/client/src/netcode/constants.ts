export const NET_TICK_HZ = 128;
export const FIXED_DT = 1 / NET_TICK_HZ;
export const INPUT_DELAY_TICKS = 2;
export const MAX_INPUT_DELAY_TICKS = 24;
export const REMOTE_INTERPOLATION_DELAY_TICKS = 8;
export const REMOTE_AVATAR_HISTORY_SAMPLES = 32;
export const REMOTE_AVATAR_POOL_SIZE = 64;
export const PROJECTILE_POOL_SIZE = 96;
export const IMPACT_EFFECT_POOL_SIZE = 64;
export const PROJECTILE_SPEED = 120;
export const PROJECTILE_LIFETIME_SECONDS = 3;
export const INPUT_RETRANSMIT_INTERVAL_MS = 24;
export const MAX_CLIENT_CATCHUP_TICKS = 8;
// The local prediction simulates this many ticks AHEAD of the latest
// authoritative tick. Without a lead the client clock pins to the server tick,
// so the snapshot for tick T arrives before the client has recorded its own
// prediction for T -- `hasPositionAt(T)` is false and every snapshot during
// movement forces a (cosmetically masked) reconcile. Leading the authority
// keeps the reconciled (past) server tick reliably inside the prediction ring.
export const PREDICTION_LEAD_TICKS = 4;
export const PREDICTION_RING_SIZE = 256;
export const RECONCILE_ERROR_EPSILON = 0.08;
export const RECONCILE_ERROR_EPSILON_SQ =
  RECONCILE_ERROR_EPSILON * RECONCILE_ERROR_EPSILON;
export const HARD_CORRECTION_DISTANCE = 0.5;
export const HARD_CORRECTION_DISTANCE_SQ =
  HARD_CORRECTION_DISTANCE * HARD_CORRECTION_DISTANCE;
export const LOCAL_CORRECTION_SMOOTH_MS = 72;
