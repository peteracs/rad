import { strict as assert } from 'node:assert';
import test from 'node:test';
import { ClientAuthorityApplier } from '../src/app/clientAuthorityApplier.js';
import { ClientNetcodeTelemetry } from '../src/app/clientNetcodeTelemetry.js';
import { ClientPredictionRunner } from '../src/app/clientPredictionRunner.js';
import { AckDiagnostics } from '../src/netcode/ackDiagnostics.js';
import { createAuthorityStateGateSnapshot } from '../src/netcode/authorityStateGate.js';
import {
  INPUT_DELAY_TICKS,
  MAX_INPUT_DELAY_TICKS,
  PREDICTION_LEAD_TICKS,
} from '../src/netcode/constants.js';
import { FixedTickClock } from '../src/netcode/fixedTickClock.js';
import {
  ClientInputSequencer,
  createClientInputReservation,
} from '../src/netcode/inputSequencer.js';
import { PredictionBuffer } from '../src/netcode/predictionBuffer.js';
import { createNetcodeDiagnosticsSnapshot } from '../src/netcode/runtimeDiagnostics.js';
import { createAvatarRenderState } from '../src/render/worldView.js';
import { FakeRadSession, FakeScene, makeServerState } from './appTestDoubles.js';

const SESSION_ID = 11;
const PLAYER_ID = 7;

// The applier announces hard corrections through a window event. Node has no
// window, so each harness installs a plain EventTarget with the same dispatch
// contract; the module resolves the global at dispatch time.
function makeApplier(session = new FakeRadSession(PLAYER_ID, 0, 0)) {
  const windowTarget = new EventTarget();
  (globalThis as { window?: EventTarget }).window = windowTarget;

  const scene = new FakeScene();
  const clock = new FixedTickClock();
  const prediction = new PredictionBuffer();
  const inputSequencer = new ClientInputSequencer();
  const ackDiagnostics = new AckDiagnostics(INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS);
  const telemetry = new ClientNetcodeTelemetry();
  const predictionRunner = new ClientPredictionRunner(
    session.asSession(),
    PLAYER_ID,
    scene.asScene(),
    prediction,
  );
  const applier = new ClientAuthorityApplier(
    SESSION_ID,
    PLAYER_ID,
    scene.asScene(),
    clock,
    prediction,
    inputSequencer,
    ackDiagnostics,
    predictionRunner,
    telemetry,
  );
  return {
    applier,
    windowTarget,
    scene,
    clock,
    prediction,
    inputSequencer,
    ackDiagnostics,
    telemetry,
    predictionRunner,
    session,
  };
}

function telemetrySnapshot(telemetry: ClientNetcodeTelemetry) {
  return telemetry.writeSnapshot(createNetcodeDiagnosticsSnapshot(), 0);
}

// Puts the local avatar mid-move under command 1: move to (5, 0) queued for
// tick 1, simulated up to `tick`, so position x === tick and target is active.
function startLocalMove(h: ReturnType<typeof makeApplier>, tick: number): void {
  h.inputSequencer.reserveInputCommand(0, 0, createClientInputReservation());
  h.prediction.recordMoveInput(1, 1, 1, 5, 0);
  h.predictionRunner.markActive();
  h.predictionRunner.advanceToTick(tick);
}

test('applier rejects snapshots for a different session without syncing', () => {
  const h = makeApplier();

  const state = makeServerState({ sessionId: 999, serverTick: 10, serverSeq: 1 });
  assert.equal(h.applier.apply(state), 0);

  assert.equal(h.applier.synced, false);
  assert.equal(h.clock.tick, 0);
  assert.equal(h.session.authorityStatesApplied.length, 0);
  const gate = h.applier.writeGateSnapshot(createAuthorityStateGateSnapshot());
  assert.equal(gate.rejectedStatePackets, 1);
  assert.equal(gate.acceptedStatePackets, 0);
});

test('applier drops duplicate and out-of-order snapshots after the first accept', () => {
  const h = makeApplier();

  const accepted = makeServerState({ serverTick: 10, serverSeq: 5, avatar: { x: 1, y: 1 } });
  assert.ok(h.applier.apply(accepted) > 0);
  assert.equal(h.session.authorityStatesApplied.length, 1);

  const duplicate = makeServerState({ serverTick: 10, serverSeq: 5, avatar: { x: 9, y: 9 } });
  assert.equal(h.applier.apply(duplicate), 0);
  const older = makeServerState({ serverTick: 9, serverSeq: 4, avatar: { x: 9, y: 9 } });
  assert.equal(h.applier.apply(older), 0);

  assert.equal(h.session.authorityStatesApplied.length, 1);
  assert.equal(h.applier.serverTickEstimate, 10);
  const gate = h.applier.writeGateSnapshot(createAuthorityStateGateSnapshot());
  assert.equal(gate.acceptedStatePackets, 1);
  assert.equal(gate.staleStatePackets, 2);
});

test('accepted snapshot syncs clock, acks, telemetry, ghost, and clears acked inputs', () => {
  const h = makeApplier();

  // Two inputs in the resend ring; the authority will report both applied.
  h.prediction.recordMoveInput(30, 1, 1, 1, 1);
  h.prediction.recordMoveInput(31, 2, 2, 2, 2);

  const state = makeServerState({
    serverTick: 20,
    serverSeq: 1,
    ackClientSeq: 3,
    ackBits: 0b111,
    avatar: { x: 2, y: 3, target_active: false },
    authority: {
      peer_count: 2,
      max_peers: 8,
      pending_move_inputs: 1,
      last_applied_client_seq: 3,
      applied_ack_bits: 0b111,
    },
  });
  state.avatars.push({
    player_id: 8,
    model: 'clockwork_mage',
    x: 4,
    y: 5,
    target_x: 0,
    target_y: 0,
    target_active: false,
    command_id: 0,
  });

  const renderNow = h.applier.apply(state);
  assert.ok(renderNow > 0, 'first snapshot has no prediction history and reconciles');

  assert.equal(h.applier.synced, true);
  assert.equal(h.applier.serverTickEstimate, 20);
  assert.equal(h.clock.tick, 20 + PREDICTION_LEAD_TICKS);
  assert.equal(h.applier.lastReceiptAckBits, 0b111);
  assert.equal(h.ackDiagnostics.highestAckSeq(), 3);
  assert.deepEqual(h.scene.ghostPositions, [{ x: 2, y: 3 }]);
  assert.equal(h.applier.authorityMayBeMoving, false);

  // Inputs the authority already applied are dropped from the resend ring.
  assert.equal(h.prediction.hasInputAt(30), false);
  assert.equal(h.prediction.hasInputAt(31), false);

  const diag = telemetrySnapshot(h.telemetry);
  assert.equal(diag.peerCount, 2);
  assert.equal(diag.maxPeers, 8);
  assert.equal(diag.pendingMoveInputs, 1);
  assert.equal(diag.avatarRecordCount, 1);
  assert.equal(diag.remoteAvatarCount, 1);
  assert.equal(diag.correctionCount, 1);
  assert.equal(diag.smoothedCorrectionCount, 0, 'no prediction history means no smoothing');

  // The authority state was applied and replayed up to the led clock tick.
  assert.deepEqual(h.session.authorityStatesApplied, [
    { x: 2, y: 3, targetX: 0, targetY: 0, targetActive: false, commandId: 0 },
  ]);
  const out = createAvatarRenderState();
  assert.equal(h.predictionRunner.writeLocalAvatarState(out), true);
  assert.equal(out.x, 2);
  assert.equal(out.y, 3);
});

test('snapshot echoing an older command is ignored while a local command is active', () => {
  const h = makeApplier();
  startLocalMove(h, 3); // command 1 active, predicted x=3

  // Authority still echoes pre-command idle state (command_id 0).
  const state = makeServerState({
    serverTick: 2,
    serverSeq: 1,
    avatar: { x: 0, y: 0, target_active: false, command_id: 0 },
  });
  assert.equal(h.applier.apply(state), 0);

  // The packet is accepted as authority data (sync, clock, tick estimate)...
  assert.equal(h.applier.synced, true);
  assert.equal(h.applier.serverTickEstimate, 2);
  assert.equal(h.clock.tick, 2 + PREDICTION_LEAD_TICKS);
  // ...but the local prediction is not rolled back to the stale echo.
  assert.equal(h.session.authorityStatesApplied.length, 0);
  const out = createAvatarRenderState();
  h.predictionRunner.writeLocalAvatarState(out);
  assert.equal(out.x, 3);
  assert.equal(out.targetActive, true);
  assert.equal(telemetrySnapshot(h.telemetry).correctionCount, 0);
});

test('snapshot matching the prediction does not reconcile or blend', () => {
  const h = makeApplier();
  startLocalMove(h, 2); // predicted x=2 at tick 2

  const state = makeServerState({
    serverTick: 2,
    serverSeq: 1,
    avatar: { x: 2, y: 0, target_x: 5, target_y: 0, target_active: true, command_id: 1 },
  });
  assert.equal(h.applier.apply(state), 0);

  assert.equal(h.applier.synced, true);
  assert.equal(h.applier.authorityMayBeMoving, true);
  assert.equal(h.session.authorityStatesApplied.length, 0);
  assert.equal(h.scene.correctionBlendTimes.length, 0);
  assert.equal(telemetrySnapshot(h.telemetry).correctionCount, 0);
});

test('diverged snapshot smooth-corrects, replays, and converges to authority', () => {
  const h = makeApplier();
  const hardCorrections: number[] = [];
  h.windowTarget.addEventListener('moba-rad-hard-correction', (event) => {
    hardCorrections.push((event as CustomEvent<{ distance: number }>).detail.distance);
  });
  startLocalMove(h, 2); // predicted x=2 at tick 2

  // Authority saw slightly less progress: error 0.2 (> epsilon, < hard cutoff).
  const state = makeServerState({
    serverTick: 2,
    serverSeq: 1,
    avatar: { x: 1.8, y: 0, target_x: 5, target_y: 0, target_active: true, command_id: 1 },
  });
  const renderNow = h.applier.apply(state);
  assert.ok(renderNow > 0);

  assert.equal(h.scene.correctionBlendTimes.length, 1);
  assert.equal(hardCorrections.length, 0);
  const diag = telemetrySnapshot(h.telemetry);
  assert.equal(diag.correctionCount, 1);
  assert.equal(diag.smoothedCorrectionCount, 1);
  assert.ok(Math.abs(diag.maxCorrectionDistance - 0.2) < 1e-9);

  // Replay integrates ticks 3..6 from the authority position 1.8 toward (5,0):
  // 2.8 -> 3.8 -> 4.8 -> arrival at 5.
  assert.equal(h.session.authorityStatesApplied.length, 1);
  assert.equal(h.clock.tick, 2 + PREDICTION_LEAD_TICKS);
  const out = createAvatarRenderState();
  h.predictionRunner.writeLocalAvatarState(out);
  assert.equal(out.x, 5);
  assert.equal(out.targetActive, false);
  assert.equal(h.prediction.hasPositionAt(h.clock.tick), true);
});

test('large divergence dispatches the hard-correction window event', () => {
  const h = makeApplier();
  const hardCorrections: number[] = [];
  h.windowTarget.addEventListener('moba-rad-hard-correction', (event) => {
    hardCorrections.push((event as CustomEvent<{ distance: number }>).detail.distance);
  });
  startLocalMove(h, 2); // predicted x=2 at tick 2

  // Error 1.2 exceeds the 0.5 hard-correction cutoff.
  const state = makeServerState({
    serverTick: 2,
    serverSeq: 1,
    avatar: { x: 0.8, y: 0, target_x: 5, target_y: 0, target_active: true, command_id: 1 },
  });
  assert.ok(h.applier.apply(state) > 0);

  assert.equal(hardCorrections.length, 1);
  assert.ok(Math.abs(hardCorrections[0] - 1.2) < 1e-9);
  assert.equal(h.scene.correctionBlendTimes.length, 1);
  const diag = telemetrySnapshot(h.telemetry);
  assert.equal(diag.correctionCount, 1);
  assert.equal(diag.smoothedCorrectionCount, 1);
});

test('authority-may-be-moving clears only while the local simulation is idle', () => {
  const h = makeApplier();

  h.applier.markAuthorityMayBeMoving();
  h.applier.clearAuthorityMayBeMovingIfIdle(true);
  assert.equal(h.applier.authorityMayBeMoving, true);
  h.applier.clearAuthorityMayBeMovingIfIdle(false);
  assert.equal(h.applier.authorityMayBeMoving, false);
});
