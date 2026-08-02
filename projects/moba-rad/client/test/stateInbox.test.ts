import { strict as assert } from 'node:assert';
import test from 'node:test';
import { ServerStateInbox } from '../src/transport/stateInbox.js';
import type { ServerState } from '../src/transport/serverState.js';

test('server state inbox drains the latest snapshot and clears stale backlog', () => {
  const inbox = new ServerStateInbox(4);
  inbox.push(stateWithSeq(10));
  inbox.push(stateWithSeq(11));
  inbox.push(stateWithSeq(12));

  assert.equal(inbox.size, 3);
  assert.equal(inbox.takeLatest()?.server_seq, 12);
  assert.equal(inbox.droppedCount, 2);
  assert.equal(inbox.size, 0);
  assert.equal(inbox.takeLatest(), null);
});

test('server state inbox is bounded and overwrites oldest snapshots first', () => {
  const inbox = new ServerStateInbox(2);
  inbox.push(stateWithSeq(1));
  inbox.push(stateWithSeq(2));
  inbox.push(stateWithSeq(3));

  assert.equal(inbox.size, 2);
  assert.equal(inbox.droppedCount, 1);
  assert.equal(inbox.takeLatest()?.server_seq, 3);
  assert.equal(inbox.droppedCount, 2);
  assert.equal(inbox.size, 0);
});

test('server state inbox can explicitly discard queued snapshots before sync waits', () => {
  const inbox = new ServerStateInbox(4);
  inbox.push(stateWithSeq(20));
  inbox.push(stateWithSeq(21));

  inbox.discardQueued();

  assert.equal(inbox.size, 0);
  assert.equal(inbox.droppedCount, 2);
  assert.equal(inbox.takeLatest(), null);
});

function stateWithSeq(serverSeq: number): ServerState {
  const avatar = {
    player_id: 1,
    model: 'clockwork_mage',
    x: 0,
    y: 0,
    target_x: 0,
    target_y: 0,
    target_active: false,
    command_id: 0,
  };
  return {
    ok: true,
    status: 'snapshot',
    correction_reason: 'snapshot',
    server_ms: serverSeq,
    server_tick: serverSeq,
    server_seq: serverSeq,
    session_id: 1,
    player_id: 1,
    ack_client_seq: 0,
    ack_bits: 0,
    command_id: 0,
    avatar,
    peers: [],
    avatars: [avatar],
    projectiles: [],
    projectile_impacts: [],
    authority: {
      peer_count: 1,
      max_peers: 8,
      input_queue_slots: 32,
      pending_move_inputs: 0,
      pending_cast_inputs: 0,
      peer_connected: true,
      late_inputs: 0,
      future_inputs: 0,
      duplicate_inputs: 0,
      overwritten_inputs: 0,
      last_client_seq: 0,
      last_applied_client_seq: 0,
      applied_ack_bits: 0,
    },
  };
}
