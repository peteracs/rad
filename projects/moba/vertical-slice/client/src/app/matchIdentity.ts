import type { MatchClientIdentity } from '../transport/matchProtocol';

// session_id only travels over the wire (u32) and as a peer-table key, so it can
// use the full protocol range for uniqueness.
const MAX_PROTOCOL_ID = 2_000_000_000;

// player_id additionally round-trips through the packed Float32 render buffer
// (radHost reads it back as `Math.trunc(f32)`). Float32 represents integers
// exactly only up to 2^24; a larger id is silently rounded (e.g. 1529554473 ->
// 1529554432), so the locally-seeded avatar's id stops matching `this.playerId`
// and the champion is never found in the predicted world. Keep player ids inside
// the f32-exact range. 2^24 still leaves ~16.7M ids, so cross-tab collisions are
// negligible.
const MAX_PLAYER_ID = 16_777_216; // 2^24

// Per-tab persistence. `sessionStorage` is scoped to a single tab and survives
// reloads, which is exactly the identity lifetime we want:
//   - two tabs  -> two stores -> two distinct (session_id, player_id) pairs
//                  -> two peers on the server that can see and sync with each other.
//   - reloading -> same store -> same pair -> the server's `remember_peer`
//                  re-finds the existing peer and we reconnect to the SAME avatar.
// This mirrors the industry pattern of a client-persisted identity used as a
// server-validated claim (vs. a transient per-connection id that would force a
// handshake round-trip before local prediction could start).
const SESSION_ID_KEY = 'moba-rad:session-id';
const PLAYER_ID_KEY = 'moba-rad:player-id';

// The subset of the Storage contract the identity actually uses. Injectable so
// the parsing/persistence rules are testable outside a browser; production
// callers use the defaults (Vite env + per-tab sessionStorage).
export interface MatchIdentityStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function createMatchIdentity(
  envPlayerId: unknown = import.meta.env.VITE_MOBA_RAD_PLAYER_ID,
  store: MatchIdentityStore | null = safeSessionStorage(),
): MatchClientIdentity {
  return {
    sessionId: persistentProtocolId(SESSION_ID_KEY, MAX_PROTOCOL_ID, store),
    playerId: playerIdFromEnv(envPlayerId) ?? persistentProtocolId(PLAYER_ID_KEY, MAX_PLAYER_ID, store),
  };
}

function randomProtocolId(ceil: number): number {
  const bytes = new Uint32Array(1);
  globalThis.crypto?.getRandomValues(bytes);
  const id = bytes[0] % ceil;
  return id > 0 ? id : 1;
}

function persistentProtocolId(key: string, max: number, store: MatchIdentityStore | null): number {
  if (!store) return randomProtocolId(max);

  // Reject stored ids outside the valid range (e.g. an old build that persisted
  // a non-f32-safe player_id) so they are regenerated instead of reused.
  const existing = Number(store.getItem(key));
  if (Number.isInteger(existing) && existing > 0 && existing <= max) {
    return existing;
  }

  const id = randomProtocolId(max);
  try {
    store.setItem(key, `${id}`);
  } catch {
    // Quota/availability failures are non-fatal: fall back to an in-memory id for
    // this load. Identity simply won't survive a reload, which is acceptable.
  }
  return id;
}

function safeSessionStorage(): Storage | null {
  try {
    return globalThis.sessionStorage ?? null;
  } catch {
    // Accessing sessionStorage throws in some sandboxed contexts.
    return null;
  }
}

function playerIdFromEnv(value: unknown): number | null {
  if (typeof value !== 'string' || value.trim() === '') return null;

  const id = Number(value);
  if (!Number.isInteger(id) || id <= 0 || id > MAX_PLAYER_ID) {
    throw new Error('VITE_MOBA_RAD_PLAYER_ID must be a positive integer <= 2^24 (render-buffer f32 limit)');
  }
  return id;
}
