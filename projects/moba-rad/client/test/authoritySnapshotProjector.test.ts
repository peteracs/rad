import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  AuthoritySnapshotProjector,
  createAuthoritySnapshotVisualStats,
} from '../src/app/authoritySnapshotProjector.js';
import type {
  ServerAvatarState,
  ServerProjectileImpactState,
} from '../src/transport/serverState.js';
import { FakeScene, makeServerState } from './appTestDoubles.js';

const LOCAL_PLAYER_ID = 7;

function remoteAvatar(playerId: number, overrides: Partial<ServerAvatarState> = {}): ServerAvatarState {
  return {
    player_id: playerId,
    model: 'clockwork_mage',
    x: 0,
    y: 0,
    target_x: 0,
    target_y: 0,
    target_active: false,
    command_id: 0,
    ...overrides,
  };
}

function impact(eventId: number, overrides: Partial<ServerProjectileImpactState> = {}): ServerProjectileImpactState {
  return {
    event_id: eventId,
    projectile_id: 1,
    owner_id: LOCAL_PLAYER_ID,
    target_id: 0,
    x: 0,
    y: 0,
    reason: 'hit',
    ...overrides,
  };
}

test('projector counts records, skips the local player, and places the authority ghost', () => {
  const scene = new FakeScene();
  const projector = new AuthoritySnapshotProjector(scene.asScene(), LOCAL_PLAYER_ID);

  const state = makeServerState({ avatar: { x: 10, y: -4 } });
  state.avatars.push(
    remoteAvatar(LOCAL_PLAYER_ID, { x: 1, y: 2 }),
    remoteAvatar(8, { x: -1, y: -2, command_id: 9 }),
    remoteAvatar(9, { x: 6, y: 7, target_active: true, command_id: 10 }),
  );

  const stats = projector.apply(state, 42, createAuthoritySnapshotVisualStats());

  assert.equal(stats.avatarRecordCount, 3);
  assert.equal(stats.remoteAvatarCount, 2);
  assert.equal(stats.projectileRecordCount, 0);
  assert.equal(stats.projectileImpactRecordCount, 0);
  assert.deepEqual(scene.ghostPositions, [{ x: 10, y: -4 }]);
  assert.equal(scene.remoteSnapshotBegins, 1);
  assert.equal(scene.remoteSnapshotEnds, 1);
  // Values must survive the projector's scratch reuse: each remote avatar
  // arrives with its own data, not the last one written.
  assert.deepEqual(
    scene.remoteAvatarSamples.map((sample) => ({
      playerId: sample.playerId,
      tick: sample.tick,
      x: sample.state.x,
      commandId: sample.state.commandId,
      targetActive: sample.state.targetActive,
    })),
    [
      { playerId: 8, tick: 42, x: -1, commandId: 9, targetActive: false },
      { playerId: 9, tick: 42, x: 6, commandId: 10, targetActive: true },
    ],
  );
});

test('projector forwards projectile snapshots inside a begin/end bracket', () => {
  const scene = new FakeScene();
  const projector = new AuthoritySnapshotProjector(scene.asScene(), LOCAL_PLAYER_ID);

  const state = makeServerState();
  state.projectiles.push(
    {
      projectile_id: 100,
      owner_id: LOCAL_PLAYER_ID,
      command_id: 1,
      x: 5,
      y: 6,
      velocity_x: 7,
      velocity_y: 8,
      spawn_tick: 40,
      fire_view_tick: 39,
    },
    {
      projectile_id: 101,
      owner_id: 8,
      command_id: 2,
      x: -5,
      y: -6,
      velocity_x: -7,
      velocity_y: -8,
      spawn_tick: 41,
      fire_view_tick: 40,
    },
  );

  const stats = projector.apply(state, 50, createAuthoritySnapshotVisualStats());

  assert.equal(stats.projectileRecordCount, 2);
  assert.equal(scene.projectileSnapshotBegins, 1);
  assert.equal(scene.projectileSnapshotEnds, 1);
  assert.deepEqual(scene.projectileSamples, [
    { projectileId: 100, x: 5, y: 6, velocityX: 7, velocityY: 8 },
    { projectileId: 101, x: -5, y: -6, velocityX: -7, velocityY: -8 },
  ]);
});

test('projector spawns each impact once across duplicated and repeated snapshots', () => {
  const scene = new FakeScene();
  const projector = new AuthoritySnapshotProjector(scene.asScene(), LOCAL_PLAYER_ID);

  const first = makeServerState({ serverSeq: 1 });
  first.projectile_impacts.push(impact(1), impact(2));
  projector.apply(first, 10, createAuthoritySnapshotVisualStats());

  // The authority keeps recent impacts in later snapshots; the same event id
  // (even repeated within one packet) must not double-spawn effects.
  const second = makeServerState({ serverSeq: 2 });
  second.projectile_impacts.push(impact(2), impact(3), impact(2));
  const stats = projector.apply(second, 11, createAuthoritySnapshotVisualStats());

  assert.equal(stats.projectileImpactRecordCount, 3);
  assert.deepEqual(
    scene.impactSpawns.map((spawn) => spawn.eventId),
    [1, 2, 3],
  );
});

test('projector never spawns the reserved zero impact id', () => {
  const scene = new FakeScene();
  const projector = new AuthoritySnapshotProjector(scene.asScene(), LOCAL_PLAYER_ID);

  const state = makeServerState();
  state.projectile_impacts.push(impact(0));
  const stats = projector.apply(state, 10, createAuthoritySnapshotVisualStats());

  assert.equal(stats.projectileImpactRecordCount, 1);
  assert.equal(scene.impactSpawns.length, 0);
});

test('projector re-spawns an impact once its id is evicted from the dedupe ring', () => {
  const scene = new FakeScene();
  const projector = new AuthoritySnapshotProjector(scene.asScene(), LOCAL_PLAYER_ID, 2);

  const first = makeServerState({ serverSeq: 1 });
  first.projectile_impacts.push(impact(1), impact(2));
  projector.apply(first, 10, createAuthoritySnapshotVisualStats());

  const second = makeServerState({ serverSeq: 2 });
  second.projectile_impacts.push(impact(3));
  projector.apply(second, 11, createAuthoritySnapshotVisualStats());

  // Ring size 2 evicted event 1, so its reappearance is treated as new.
  const third = makeServerState({ serverSeq: 3 });
  third.projectile_impacts.push(impact(1));
  projector.apply(third, 12, createAuthoritySnapshotVisualStats());

  assert.deepEqual(
    scene.impactSpawns.map((spawn) => spawn.eventId),
    [1, 2, 3, 1],
  );
});
