import { strict as assert } from 'node:assert';
import test from 'node:test';
import { createAuthoritySnapshotVisualStats } from '../src/app/authoritySnapshotProjector.js';
import { ClientNetcodeTelemetry } from '../src/app/clientNetcodeTelemetry.js';
import {
  AckDiagnostics,
  createAckDiagnosticsSnapshot,
} from '../src/netcode/ackDiagnostics.js';
import {
  INPUT_DELAY_TICKS,
  MAX_INPUT_DELAY_TICKS,
  NET_TICK_HZ,
} from '../src/netcode/constants.js';
import { createNetcodeDiagnosticsSnapshot } from '../src/netcode/runtimeDiagnostics.js';
import { makeServerState } from './appTestDoubles.js';

function snapshot(telemetry: ClientNetcodeTelemetry, nowMs = 0) {
  return telemetry.writeSnapshot(createNetcodeDiagnosticsSnapshot(), nowMs);
}

test('telemetry counts resends, requests, and classified transport failures', () => {
  const telemetry = new ClientNetcodeTelemetry();

  telemetry.noteInputResendPacket();
  telemetry.noteInputResendPacket();
  telemetry.noteAuthorityStateRequest();
  telemetry.noteTransportFailure(new Error('Timed out waiting for state'));
  telemetry.noteTransportFailure('wire exploded');

  const diag = snapshot(telemetry);
  assert.equal(diag.inputResendPackets, 2);
  assert.equal(diag.authorityStateRequests, 1);
  assert.equal(diag.transportFailures, 2);
  assert.equal(diag.lastTransportError, 'wire exploded', 'non-Error reasons are stringified');
  assert.equal(diag.authorityTimeouts, 1, 'only timeout messages count as timeouts');
});

test('round-trip sampling seeds then smooths, and feeds the ack diagnostics', () => {
  const telemetry = new ClientNetcodeTelemetry();
  const ackDiagnostics = new AckDiagnostics(INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS);
  const ackScratch = createAckDiagnosticsSnapshot();

  telemetry.noteAuthorityRoundTrip(0, 100, ackDiagnostics, NET_TICK_HZ, ackScratch);
  let diag = snapshot(telemetry);
  assert.equal(diag.roundTripMs, 100, 'first sample seeds the estimate');
  assert.equal(diag.jitterMs, 0);

  telemetry.noteAuthorityRoundTrip(0, 200, ackDiagnostics, NET_TICK_HZ, ackScratch);
  diag = snapshot(telemetry);
  assert.ok(Math.abs(diag.roundTripMs - 115) < 1e-9, '0.85/0.15 EWMA over samples');
  assert.ok(Math.abs(diag.jitterMs - 15) < 1e-9, 'jitter tracks sample-to-sample delta');

  assert.ok(
    ackDiagnostics.recommendedDelayTicks() > INPUT_DELAY_TICKS,
    '100ms+ round trips raise the recommended input delay',
  );
});

test('corrections track totals, smoothed count, and the worst distance', () => {
  const telemetry = new ClientNetcodeTelemetry();

  telemetry.noteCorrection(0, false);
  telemetry.noteCorrection(0.5, true);
  telemetry.noteCorrection(0.3, true);

  const diag = snapshot(telemetry);
  assert.equal(diag.correctionCount, 3);
  assert.equal(diag.smoothedCorrectionCount, 2);
  assert.equal(diag.maxCorrectionDistance, 0.5);
});

test('authority snapshots copy server telemetry and visual stats verbatim', () => {
  const telemetry = new ClientNetcodeTelemetry();

  const state = makeServerState({
    authority: {
      peer_count: 3,
      max_peers: 8,
      input_queue_slots: 64,
      pending_move_inputs: 2,
      pending_cast_inputs: 1,
      peer_connected: true,
      late_inputs: 4,
      future_inputs: 5,
      duplicate_inputs: 6,
      overwritten_inputs: 7,
      last_client_seq: 9,
      last_applied_client_seq: 8,
      applied_ack_bits: 0xffffffff,
    },
  });
  state.peers.push({
    player_id: 8,
    session_id: 12,
    last_client_seq: 1,
    received_client_seq: 1,
    last_applied_client_seq: 1,
    applied_ack_bits: 1,
    pending_move_inputs: 0,
    pending_cast_inputs: 0,
    connected: true,
    late_inputs: 0,
    future_inputs: 0,
    duplicate_inputs: 0,
    overwritten_inputs: 0,
  });
  const stats = createAuthoritySnapshotVisualStats();
  stats.avatarRecordCount = 4;
  stats.remoteAvatarCount = 3;
  stats.projectileRecordCount = 2;
  stats.projectileImpactRecordCount = 1;

  telemetry.applyAuthoritySnapshot(state, stats);

  const diag = snapshot(telemetry);
  assert.equal(diag.peerCount, 3);
  assert.equal(diag.peerRecordCount, 1);
  assert.equal(diag.maxPeers, 8);
  assert.equal(diag.inputQueueSlots, 64);
  assert.equal(diag.pendingMoveInputs, 2);
  assert.equal(diag.pendingCastInputs, 1);
  assert.equal(diag.peerConnected, true);
  assert.equal(diag.lateInputs, 4);
  assert.equal(diag.futureInputs, 5);
  assert.equal(diag.duplicateInputs, 6);
  assert.equal(diag.overwrittenInputs, 7);
  assert.equal(diag.lastAuthorityClientSeq, 9);
  assert.equal(diag.lastAuthorityAppliedSeq, 8);
  assert.equal(diag.lastAuthorityAppliedAckBits, 0xffffffff);
  assert.equal(diag.avatarRecordCount, 4);
  assert.equal(diag.remoteAvatarCount, 3);
  assert.equal(diag.projectileRecordCount, 2);
  assert.equal(diag.projectileImpactRecordCount, 1);
});

test('reconciliation rate averages corrections over one-second windows', () => {
  const telemetry = new ClientNetcodeTelemetry();

  snapshot(telemetry, 1000); // seed the sampling window
  telemetry.noteCorrection(0, false);
  telemetry.noteCorrection(0, false);
  telemetry.noteCorrection(0, false);

  assert.equal(snapshot(telemetry, 1500).reconciliationRatePerSecond, 0, 'window still open');
  assert.equal(snapshot(telemetry, 2000).reconciliationRatePerSecond, 3);
  assert.equal(snapshot(telemetry, 3000).reconciliationRatePerSecond, 0, 'quiet window decays to zero');
});
