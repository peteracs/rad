import { strict as assert } from 'node:assert';
import test from 'node:test';
import { createMatchIdentity, type MatchIdentityStore } from '../src/app/matchIdentity.js';

const MAX_PLAYER_ID = 16_777_216; // 2^24, render-buffer f32-exact ceiling
const MAX_SESSION_ID = 2_000_000_000;
const PLAYER_ID_KEY = 'moba-rad:player-id';
const SESSION_ID_KEY = 'moba-rad:session-id';

class FakeStore implements MatchIdentityStore {
  readonly values = new Map<string, string>();
  setItemFailure: Error | null = null;

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    if (this.setItemFailure) throw this.setItemFailure;
    this.values.set(key, value);
  }
}

test('a valid env player id wins over persistence and is used verbatim', () => {
  const store = new FakeStore();
  const identity = createMatchIdentity('42', store);

  assert.equal(identity.playerId, 42);
  assert.equal(store.values.has(PLAYER_ID_KEY), false, 'env ids are not persisted');
  assert.ok(identity.sessionId >= 1 && identity.sessionId <= MAX_SESSION_ID);
  assert.equal(Number(store.values.get(SESSION_ID_KEY)), identity.sessionId);
});

test('malformed env player ids fail loudly instead of silently degrading', () => {
  for (const bad of ['abc', '0', '-3', '3.5', `${MAX_PLAYER_ID + 1}`]) {
    assert.throws(
      () => createMatchIdentity(bad, new FakeStore()),
      /positive integer/,
      `expected rejection for env value ${JSON.stringify(bad)}`,
    );
  }
});

test('blank or non-string env values fall back to the persisted identity', () => {
  const store = new FakeStore();
  store.values.set(PLAYER_ID_KEY, '123');
  store.values.set(SESSION_ID_KEY, '456');

  assert.deepEqual(createMatchIdentity(null, store), { sessionId: 456, playerId: 123 });
  assert.deepEqual(createMatchIdentity('   ', store), { sessionId: 456, playerId: 123 });
  // Only strings configure the override; a numeric 99 is not an env value.
  assert.deepEqual(createMatchIdentity(99, store), { sessionId: 456, playerId: 123 });
});

test('the persisted identity is stable across reload-like repeat calls', () => {
  const store = new FakeStore();
  const first = createMatchIdentity(null, store);
  const second = createMatchIdentity(null, store);

  assert.deepEqual(second, first);
  assert.ok(first.playerId >= 1 && first.playerId <= MAX_PLAYER_ID);
  assert.ok(first.sessionId >= 1 && first.sessionId <= MAX_SESSION_ID);
});

test('out-of-range or garbage persisted ids are regenerated and re-stored', () => {
  const store = new FakeStore();
  store.values.set(PLAYER_ID_KEY, `${MAX_PLAYER_ID + 1}`); // pre-f32-fix build
  store.values.set(SESSION_ID_KEY, 'not-a-number');

  const identity = createMatchIdentity(null, store);

  assert.ok(identity.playerId >= 1 && identity.playerId <= MAX_PLAYER_ID);
  assert.ok(identity.sessionId >= 1 && identity.sessionId <= MAX_SESSION_ID);
  assert.equal(Number(store.values.get(PLAYER_ID_KEY)), identity.playerId);
  assert.equal(Number(store.values.get(SESSION_ID_KEY)), identity.sessionId);
});

test('storage write failures still yield a usable in-memory identity', () => {
  const store = new FakeStore();
  store.setItemFailure = new Error('quota exceeded');

  const identity = createMatchIdentity(null, store);

  assert.ok(identity.playerId >= 1 && identity.playerId <= MAX_PLAYER_ID);
  assert.ok(identity.sessionId >= 1 && identity.sessionId <= MAX_SESSION_ID);
  assert.equal(store.values.size, 0);
});

test('a missing store falls back to random ids in the valid ranges', () => {
  const identity = createMatchIdentity(null, null);

  assert.ok(Number.isInteger(identity.playerId));
  assert.ok(identity.playerId >= 1 && identity.playerId <= MAX_PLAYER_ID);
  assert.ok(Number.isInteger(identity.sessionId));
  assert.ok(identity.sessionId >= 1 && identity.sessionId <= MAX_SESSION_ID);
});
