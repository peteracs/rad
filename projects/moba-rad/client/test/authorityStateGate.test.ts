import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  AuthorityStateGate,
  createAuthorityStateGateSnapshot,
  type AuthorityStateGateInput,
} from '../src/netcode/authorityStateGate.js';

type GateInputOverrides = Partial<Omit<AuthorityStateGateInput, 'avatar'>> & {
  avatar?: Partial<AuthorityStateGateInput['avatar']>;
};

test('authority state gate accepts fresh matching snapshots and records receipt ack bits', () => {
  const gate = new AuthorityStateGate(100, 7);
  const state = createGateInput({
    session_id: 100,
    player_id: 7,
    server_seq: 10,
    ack_client_seq: 5,
    ack_bits: 0b10101,
    status: 'snapshot',
    correction_reason: 'applied',
  });

  assert.equal(gate.accept(state, 4), true);
  assert.equal(gate.lastServerSeq, 10);
  assert.equal(gate.lastReceiptAckBits, 0b10101);

  const snapshot = gate.writeSnapshot(createAuthorityStateGateSnapshot());
  assert.equal(snapshot.statePacketsReceived, 1);
  assert.equal(snapshot.acceptedStatePackets, 1);
  assert.equal(snapshot.staleStatePackets, 0);
  assert.equal(snapshot.rejectedStatePackets, 0);
  assert.equal(snapshot.lastAuthorityStatus, 'snapshot');
  assert.equal(snapshot.lastCorrectionReason, 'applied');
});

test('authority state gate rejects wrong owner and stale server sequences separately', () => {
  const gate = new AuthorityStateGate(100, 7);

  assert.equal(gate.accept(createGateInput({ session_id: 101, server_seq: 1 }), 0), false);
  assert.equal(gate.accept(createGateInput({ session_id: 100, player_id: 7, server_seq: 2 }), 0), true);
  assert.equal(gate.accept(createGateInput({ session_id: 100, player_id: 7, server_seq: 2 }), 0), false);
  assert.equal(gate.accept(createGateInput({ session_id: 100, player_id: 7, server_seq: 1 }), 0), false);

  const snapshot = gate.writeSnapshot(createAuthorityStateGateSnapshot());
  assert.equal(snapshot.statePacketsReceived, 4);
  assert.equal(snapshot.acceptedStatePackets, 1);
  assert.equal(snapshot.rejectedStatePackets, 1);
  assert.equal(snapshot.staleStatePackets, 2);
  assert.equal(snapshot.lastServerSeq, 2);
});

test('authority state gate rejects invalid sequence fields before ack diagnostics can be poisoned', () => {
  const gate = new AuthorityStateGate(100, 7);

  assert.equal(gate.accept(createGateInput({ server_seq: Number.NaN }), 0), false);
  assert.equal(gate.accept(createGateInput({ server_seq: 1.5 }), 0), false);
  assert.equal(gate.accept(createGateInput({ ack_client_seq: Number.POSITIVE_INFINITY }), 0), false);
  assert.equal(gate.accept(createGateInput({ ack_bits: Number.NaN }), 0), false);
  assert.equal(gate.accept(createGateInput({ ack_bits: -1 }), 0), false);
  assert.equal(gate.accept(createGateInput({ server_seq: 0x1_0000_0000 }), 0), false);
  assert.equal(gate.accept(createGateInput({ ack_bits: 0x1_0000_0000 }), 0), false);

  const snapshot = gate.writeSnapshot(createAuthorityStateGateSnapshot());
  assert.equal(snapshot.statePacketsReceived, 7);
  assert.equal(snapshot.acceptedStatePackets, 0);
  assert.equal(snapshot.rejectedStatePackets, 7);
  assert.equal(gate.lastServerSeq, 0);
  assert.equal(gate.lastReceiptAckBits, 0);
});

test('authority state gate rejects invalid authoritative tick or avatar fields before rollback', () => {
  const gate = new AuthorityStateGate(100, 7);

  assert.equal(gate.accept(createGateInput({ server_tick: Number.NaN }), 0), false);
  assert.equal(gate.accept(createGateInput({ server_tick: -1 }), 0), false);
  assert.equal(gate.accept(createGateInput({
    avatar: {
      x: Number.POSITIVE_INFINITY,
    },
  }), 0), false);
  assert.equal(gate.accept(createGateInput({
    avatar: {
      target_x: Number.NaN,
    },
  }), 0), false);
  assert.equal(gate.accept(createGateInput({
    avatar: {
      command_id: 1.5,
    },
  }), 0), false);
  assert.equal(gate.accept(createGateInput({
    avatar: {
      command_id: -1,
    },
  }), 0), false);
  assert.equal(gate.accept(createGateInput({
    avatar: {
      target_active: 'yes' as unknown as boolean,
    },
  }), 0), false);

  const snapshot = gate.writeSnapshot(createAuthorityStateGateSnapshot());
  assert.equal(snapshot.statePacketsReceived, 7);
  assert.equal(snapshot.acceptedStatePackets, 0);
  assert.equal(snapshot.rejectedStatePackets, 7);
  assert.equal(snapshot.lastServerSeq, 0);
});

test('authority state gate keeps resend ack bits when a snapshot has an older receipt ack', () => {
  const gate = new AuthorityStateGate(100, 7);

  assert.equal(gate.accept(createGateInput({
    server_seq: 1,
    ack_client_seq: 10,
    ack_bits: 0xffff,
  }), 9), true);
  assert.equal(gate.lastReceiptAckBits, 0xffff);

  assert.equal(gate.accept(createGateInput({
    server_seq: 2,
    ack_client_seq: 8,
    ack_bits: 0,
  }), 10), true);
  assert.equal(gate.lastReceiptAckBits, 0xffff);
});

function createGateInput(overrides: GateInputOverrides = {}): AuthorityStateGateInput {
  const { avatar: avatarOverrides, ...rootOverrides } = overrides;
  return {
    ok: true,
    session_id: 100,
    player_id: 7,
    server_tick: 0,
    server_seq: 1,
    ack_client_seq: 0,
    ack_bits: 0,
    status: 'snapshot',
    correction_reason: 'snapshot',
    ...rootOverrides,
    avatar: {
      x: 0,
      y: 0,
      target_x: 0,
      target_y: 0,
      target_active: false,
      command_id: 0,
      ...avatarOverrides,
    },
  };
}
