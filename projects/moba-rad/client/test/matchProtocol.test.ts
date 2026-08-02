import assert from 'node:assert/strict';
import test from 'node:test';
import {
  CAST_PACKET_BYTES,
  PACKET_KIND_ERROR,
  STATE_PACKET_BYTES,
  encodeCastPacket,
  encodeDisconnectPacket,
  encodeMoveOrderPacket,
  encodeSyncPacket,
  parseServerStatePacket,
} from '../src/transport/matchProtocol.js';
import {
  copyServerState,
  createServerStateBuffer,
} from '../src/transport/serverStateBuffer.js';

const identity = { sessionId: 9001, playerId: 3 };

test('encodes client input packets with sequencing and target ticks', () => {
  assert.deepEqual(
    Array.from(encodeMoveOrderPacket(identity, 7, 130, 42, 12.5, -8.25)),
    [
      77, 10, 2,
      7, 0, 0, 0,
      41, 35, 0, 0,
      3, 0, 0, 0,
      130, 0, 0, 0,
      42, 0, 0, 0,
      212, 48, 0, 0,
      198, 223, 255, 255,
    ],
  );
  assert.deepEqual(
    Array.from(encodeSyncPacket(identity, 8)),
    [77, 10, 1, 8, 0, 0, 0, 41, 35, 0, 0, 3, 0, 0, 0],
  );
  assert.deepEqual(
    Array.from(encodeDisconnectPacket(identity, 9)),
    [77, 10, 3, 9, 0, 0, 0, 41, 35, 0, 0, 3, 0, 0, 0],
  );
  assert.deepEqual(
    Array.from(encodeCastPacket(identity, 10, 133, 43, 1, 0, 125)),
    [
      77, 10, 6,
      10, 0, 0, 0,
      41, 35, 0, 0,
      3, 0, 0, 0,
      133, 0, 0, 0,
      43, 0, 0, 0,
      232, 3, 0, 0,
      0, 0, 0, 0,
      125, 0, 0, 0,
    ],
  );
  assert.equal(encodeCastPacket(identity, 10, 133, 43, 1, 0, 125).length, CAST_PACKET_BYTES);
});

test('parses authoritative avatar snapshots', () => {
  const statePacket = new Uint8Array([
    77, 10, 4,
    232, 3, 0, 0,
    130, 0, 0, 0,
    5, 0, 0, 0,
    41, 35, 0, 0,
    3, 0, 0, 0,
    7, 0, 0, 0,
    3, 0, 0, 0,
    42, 0, 0, 0,
    212, 48, 0, 0,
    198, 223, 255, 255,
    200, 50, 0, 0,
    216, 220, 255, 255,
    0,
    6,
    1,
    1,
    0,
    2, 8, 32, 1, 0, 1, 0, 0,
    2, 0, 0, 0,
    3, 0, 0, 0,
    4, 0, 0, 0,
    5, 0, 0, 0,
    7, 0, 0, 0,
    7, 0, 0, 0,
    3, 0, 0, 0,
    3, 0, 0, 0,
    42, 0, 0, 0,
    212, 48, 0, 0,
    198, 223, 255, 255,
    200, 50, 0, 0,
    216, 220, 255, 255,
    0,
    1,
  ]);
  assert.equal(statePacket.length, STATE_PACKET_BYTES);

  assert.deepEqual(parseServerStatePacket(statePacket), {
    ok: true,
    status: 'applied',
    correction_reason: 'applied',
    server_ms: 1000,
    server_tick: 130,
    server_seq: 5,
    session_id: 9001,
    player_id: 3,
    ack_client_seq: 7,
    ack_bits: 3,
    command_id: 42,
    avatar: {
      command_id: 42,
      model: 'clockwork_mage',
      player_id: 3,
      x: 12.5,
      y: -8.25,
      target_x: 13,
      target_y: -9,
      target_active: false,
    },
    peers: [],
    avatars: [{
      command_id: 42,
      model: 'clockwork_mage',
      player_id: 3,
      x: 12.5,
      y: -8.25,
      target_x: 13,
      target_y: -9,
      target_active: false,
    }],
    projectiles: [],
    projectile_impacts: [],
    authority: {
      peer_count: 2,
      max_peers: 8,
      input_queue_slots: 32,
      pending_move_inputs: 1,
      pending_cast_inputs: 0,
      peer_connected: true,
      late_inputs: 2,
      future_inputs: 3,
      duplicate_inputs: 4,
      overwritten_inputs: 5,
      last_client_seq: 7,
      last_applied_client_seq: 7,
      applied_ack_bits: 3,
    },
  });

  const reusableState = createServerStateBuffer();
  assert.equal(parseServerStatePacket(statePacket, reusableState), reusableState);
  const avatarRef = reusableState.avatar;
  const authorityRef = reusableState.authority;
  const rosterRef = reusableState.avatars[0];

  assert.equal(parseServerStatePacket(statePacket, reusableState), reusableState);
  assert.equal(reusableState.avatar, avatarRef);
  assert.equal(reusableState.authority, authorityRef);
  assert.equal(reusableState.avatars[0], rosterRef);

  const copiedState = createServerStateBuffer();
  assert.equal(copyServerState(reusableState, copiedState), copiedState);
  assert.deepEqual(copiedState, reusableState);
  assert.notEqual(copiedState.avatar, reusableState.avatar);
  assert.notEqual(copiedState.avatars[0], reusableState.avatars[0]);
});

test('parses roster snapshots for remote entity interpolation', () => {
  const statePacket = new Uint8Array([
    77, 10, 4,
    232, 3, 0, 0,
    130, 0, 0, 0,
    5, 0, 0, 0,
    41, 35, 0, 0,
    3, 0, 0, 0,
    7, 0, 0, 0,
    3, 0, 0, 0,
    42, 0, 0, 0,
    212, 48, 0, 0,
    198, 223, 255, 255,
    200, 50, 0, 0,
    216, 220, 255, 255,
    0,
    7,
    8,
    2,
    0,
    0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    3, 0, 0, 0,
    42, 0, 0, 0,
    212, 48, 0, 0,
    198, 223, 255, 255,
    200, 50, 0, 0,
    216, 220, 255, 255,
    0,
    1,
    4, 0, 0, 0,
    9, 0, 0, 0,
    32, 78, 0, 0,
    136, 19, 0, 0,
    240, 85, 0, 0,
    88, 27, 0, 0,
    1,
    1,
  ]);
  const state = parseServerStatePacket(statePacket);

  assert.equal(state?.avatars.length, 2);
  assert.equal(state?.avatars[1].player_id, 4);
  assert.equal(state?.avatars[1].x, 20);
  assert.equal(state?.avatars[1].target_active, true);
});

test('parses fixed-stride peer-table snapshots before avatar records', () => {
  const statePacket = new Uint8Array([
    77, 10, 4,
    232, 3, 0, 0,
    130, 0, 0, 0,
    5, 0, 0, 0,
    41, 35, 0, 0,
    3, 0, 0, 0,
    7, 0, 0, 0,
    3, 0, 0, 0,
    42, 0, 0, 0,
    212, 48, 0, 0,
    198, 223, 255, 255,
    200, 50, 0, 0,
    216, 220, 255, 255,
    0,
    7,
    8,
    0,
    0,
    1, 8, 32, 1, 0, 1, 0, 1,
    2, 0, 0, 0,
    3, 0, 0, 0,
    4, 0, 0, 0,
    5, 0, 0, 0,
    7, 0, 0, 0,
    7, 0, 0, 0,
    3, 0, 0, 0,
    3, 0, 0, 0,
    41, 35, 0, 0,
    7, 0, 0, 0,
    7, 0, 0, 0,
    7, 0, 0, 0,
    3, 0, 0, 0,
    1, 0, 1, 0,
    2, 0, 0, 0,
    3, 0, 0, 0,
    4, 0, 0, 0,
    5, 0, 0, 0,
  ]);
  const state = parseServerStatePacket(statePacket);

  assert.equal(state?.peers.length, 1);
  assert.equal(state?.avatars.length, 0);
  assert.equal(state?.peers[0].player_id, 3);
  assert.equal(state?.peers[0].session_id, 9001);
  assert.equal(state?.peers[0].received_client_seq, 7);
  assert.equal(state?.peers[0].applied_ack_bits, 3);
  assert.equal(state?.peers[0].pending_move_inputs, 1);
  assert.equal(state?.peers[0].connected, true);
  assert.equal(state?.peers[0].duplicate_inputs, 4);
});

test('parses projectile snapshots after avatar roster', () => {
  const statePacket = new Uint8Array([
    77, 10, 4,
    232, 3, 0, 0,
    130, 0, 0, 0,
    5, 0, 0, 0,
    41, 35, 0, 0,
    3, 0, 0, 0,
    7, 0, 0, 0,
    3, 0, 0, 0,
    42, 0, 0, 0,
    212, 48, 0, 0,
    198, 223, 255, 255,
    200, 50, 0, 0,
    216, 220, 255, 255,
    0,
    16,
    12,
    1,
    1,
    0, 0, 0, 0, 0, 0, 1, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    3, 0, 0, 0,
    42, 0, 0, 0,
    212, 48, 0, 0,
    198, 223, 255, 255,
    200, 50, 0, 0,
    216, 220, 255, 255,
    0,
    1,
    235, 198, 45, 0,
    3, 0, 0, 0,
    43, 0, 0, 0,
    32, 78, 0, 0,
    136, 19, 0, 0,
    192, 212, 1, 0,
    0, 0, 0, 0,
    133, 0, 0, 0,
    125, 0, 0, 0,
    77, 0, 0, 0,
    235, 198, 45, 0,
    3, 0, 0, 0,
    4, 0, 0, 0,
    32, 78, 0, 0,
    136, 19, 0, 0,
    1,
  ]);
  const state = parseServerStatePacket(statePacket);

  assert.equal(state?.status, 'cast');
  assert.equal(state?.correction_reason, 'cast');
  assert.equal(state?.projectiles.length, 1);
  assert.equal(state?.projectiles[0].projectile_id, 3_000_043);
  assert.equal(state?.projectiles[0].x, 20);
  assert.equal(state?.projectiles[0].velocity_x, 120);
  assert.equal(state?.projectile_impacts.length, 1);
  assert.equal(state?.projectile_impacts[0].event_id, 77);
  assert.equal(state?.projectile_impacts[0].reason, 'hit');
});

test('rejects malformed snapshots', () => {
  assert.equal(parseServerStatePacket(new Uint8Array([77, 10, 4])), null);
  assert.equal(parseServerStatePacket(new Uint8Array([77, 10, PACKET_KIND_ERROR, 10])), null);
});
