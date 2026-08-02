import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  PREDICTION_INPUT_KIND_CAST,
  PREDICTION_INPUT_KIND_MOVE,
  PredictionBuffer,
  type PredictionInputSnapshot,
} from '../src/netcode/predictionBuffer.js';
import { FIXED_DT, NET_TICK_HZ, PREDICTION_RING_SIZE } from '../src/netcode/constants.js';

// Worst-case round trip the chaos harness can emulate: +120ms base latency with
// +/-15ms jitter on EACH direction, plus a little authority tick processing.
// That lands around 270-300ms; we audit against the upper bound.
const WORST_CASE_RTT_MS = 300;
const WORST_CASE_RTT_TICKS = Math.ceil((WORST_CASE_RTT_MS / 1000) * NET_TICK_HZ);

test('prediction ring is a power of two so the bit-mask slot index is exact', () => {
  assert.equal(
    PREDICTION_RING_SIZE & (PREDICTION_RING_SIZE - 1),
    0,
    'PREDICTION_RING_SIZE must be a power of two for the & RING_MASK indexing to be correct',
  );
});

test('prediction history comfortably outlasts the worst-case chaos RTT', () => {
  // A snapshot delayed by the worst-case RTT must still land on its original,
  // un-lapped slot. We require generous headroom (>= 2x) so jitter spikes and
  // catch-up bursts never push a still-relevant tick out of the buffer.
  assert.ok(
    PREDICTION_RING_SIZE >= WORST_CASE_RTT_TICKS * 2,
    `ring of ${PREDICTION_RING_SIZE} must be >= 2x worst-case RTT (${WORST_CASE_RTT_TICKS} ticks)`,
  );
  const historySeconds = PREDICTION_RING_SIZE * FIXED_DT;
  assert.ok(
    historySeconds >= 1.0,
    `ring holds ${historySeconds.toFixed(3)}s of history; need >= 1.0s above the RTT budget`,
  );
});

test('a snapshot delayed by the worst-case RTT still reconciles cleanly', () => {
  const buffer = new PredictionBuffer();
  const castTick = 1000;
  buffer.recordMoveInput(castTick, 1, 7, 12.5, -4.0);
  buffer.recordPosition(castTick, 12.5, -4.0);

  // The client keeps predicting forward the whole time the snapshot is in flight.
  for (let tick = castTick + 1; tick <= castTick + WORST_CASE_RTT_TICKS; tick += 1) {
    buffer.recordPosition(tick, tick * 0.1, tick * 0.2);
  }

  // The late authoritative snapshot for castTick finally arrives.
  assert.ok(buffer.hasPositionAt(castTick), 'the delayed tick must still be in the prediction history');
  assert.ok(buffer.hasInputAt(castTick), 'the delayed input must still be available to replay');
  assert.ok(
    buffer.positionErrorSq(castTick, 12.5, -4.0) < 1e-9,
    'a matching authoritative position reconciles with ~zero error (no snap)',
  );
});

test('a tick lapped out of the ring reports stale instead of mis-reconciling', () => {
  const buffer = new PredictionBuffer();
  const oldTick = 1000;
  buffer.recordPosition(oldTick, 5, 5);

  // Lap the ring exactly once: the same slot, a different tick.
  buffer.recordPosition(oldTick + PREDICTION_RING_SIZE, 99, 99);

  assert.ok(
    !buffer.hasPositionAt(oldTick),
    'a lapped tick must NOT report a false positive that would reconcile against the wrong slot',
  );
  assert.ok(buffer.hasPositionAt(oldTick + PREDICTION_RING_SIZE), 'the current occupant of the slot is valid');
});

test('records and reads predicted inputs by tick', () => {
  const buffer = new PredictionBuffer();
  buffer.recordMoveInput(42, 9, 3, 1.5, 2.5);
  assert.ok(buffer.hasInputAt(42));
  assert.ok(buffer.hasMoveInputAt(42));
  assert.equal(buffer.inputKindAt(42), PREDICTION_INPUT_KIND_MOVE);
  assert.equal(buffer.inputCommandIdAt(42), 3);
  assert.equal(buffer.inputTargetXAt(42), 1.5);
  assert.equal(buffer.inputTargetYAt(42), 2.5);
  assert.ok(!buffer.hasInputAt(43), 'an empty tick must not report an input');
});

test('overdue move inputs apply once when authority snapshots skip their exact tick', () => {
  const buffer = new PredictionBuffer();
  const out = createInputScratch();

  buffer.recordMoveInput(42, 9, 3, 1.5, 2.5);

  assert.ok(!buffer.writeNextUnappliedMoveAtOrBefore(41, out), 'future input must wait');
  assert.ok(buffer.hasUnappliedMoveAtOrBefore(45), 'skipped target tick must remain due');
  assert.ok(buffer.writeNextUnappliedMoveAtOrBefore(45, out), 'overdue move must apply');
  assert.equal(out.tick, 42);
  assert.equal(out.clientSeq, 9);
  assert.equal(out.commandId, 3);
  assert.equal(out.targetX, 1.5);
  assert.equal(out.targetY, 2.5);
  assert.ok(!buffer.writeNextUnappliedMoveAtOrBefore(45, out), 'same input must not apply twice');
});

test('rollback replay can re-arm locally applied moves inside the replay window', () => {
  const buffer = new PredictionBuffer();
  const out = createInputScratch();

  buffer.recordMoveInput(50, 10, 4, 7, 8);
  assert.ok(buffer.writeNextUnappliedMoveAtOrBefore(50, out));
  assert.ok(!buffer.writeNextUnappliedMoveAtOrBefore(50, out));

  buffer.markMoveInputsForReplay(49, 51);

  assert.ok(buffer.writeNextUnappliedMoveAtOrBefore(50, out));
  assert.equal(out.tick, 50);
  assert.equal(out.commandId, 4);
});

test('records cast inputs for ack-window resend without applying them as movement', () => {
  const buffer = new PredictionBuffer();
  buffer.recordCastInput(50, 12, 300, 0.6, 0.8, 44);

  assert.ok(buffer.hasInputAt(50));
  assert.ok(!buffer.hasMoveInputAt(50), 'cast input history must not replay as a move order');
  assert.equal(buffer.inputKindAt(50), PREDICTION_INPUT_KIND_CAST);
  assert.equal(buffer.inputDirXAt(50), 0.6);
  assert.equal(buffer.inputDirYAt(50), 0.8);
  assert.equal(buffer.inputFireViewTickAt(50), 44);
});

test('receipt ack window drives resend without clearing unapplied rollback history', () => {
  const buffer = new PredictionBuffer();
  buffer.recordMoveInput(10, 1, 100, 0, 0);
  buffer.recordMoveInput(11, 2, 101, 0, 0);
  buffer.recordMoveInput(12, 3, 102, 0, 0);

  // Ack watermark seq 3, bitfield reports seq3 (bit0) and seq1 (bit2) received,
  // but seq2 (bit1) was lost. This mirrors the server receipt ack_bits semantics.
  const ack = 3;
  const bits = 0b101;

  const out = createInputScratch();
  assert.ok(
    buffer.writeOldestUnackedInputAfter(ack, bits, 0, out),
    'the lost-but-unacked input must still be available to resend',
  );
  assert.equal(out.clientSeq, 2, 'seq 1 and 3 are acked; the oldest unacked is the lost seq 2');
  assert.equal(out.commandId, 101);

  assert.ok(buffer.hasInputAt(10), 'receipt ack alone must not delete replay history before apply');
  assert.ok(buffer.hasInputAt(12), 'received future input stays replayable until the authoritative tick applies it');
  buffer.clearAppliedInputs(1, 1);
  assert.ok(!buffer.hasInputAt(10), 'applied progress clears rollback history through the applied client seq');
  assert.ok(buffer.hasInputAt(12), 'unapplied received input remains available for prediction replay');
});

test('receipt ack window resends lost casts from the same bounded input ring', () => {
  const buffer = new PredictionBuffer();
  buffer.recordMoveInput(30, 1, 210, 4, 5);
  buffer.recordCastInput(31, 2, 211, 1, 0, 29);

  const out = createInputScratch();
  assert.ok(
    buffer.writeOldestUnackedInputAfter(2, 0b10, 0, out),
    'acked move plus lost cast must surface the cast for resend',
  );
  assert.equal(out.kind, PREDICTION_INPUT_KIND_CAST);
  assert.equal(out.clientSeq, 2);
  assert.equal(out.commandId, 211);
  assert.equal(out.dirX, 1);
  assert.equal(out.dirY, 0);
  assert.equal(out.fireViewTick, 29);
});

test('applied ack window does not clear lower un-applied inputs just because the high watermark advanced', () => {
  const buffer = new PredictionBuffer();
  buffer.recordMoveInput(20, 10, 200, 1, 1);
  buffer.recordMoveInput(21, 11, 201, 2, 2);

  buffer.clearAppliedInputs(11, 0b001);

  assert.ok(buffer.hasInputAt(20), 'seq10 is below the applied watermark but its bit is absent');
  assert.ok(!buffer.hasInputAt(21), 'seq11 is applied and should be retired from rollback history');
});

function createInputScratch(): PredictionInputSnapshot {
  return {
    kind: 0,
    tick: 0,
    clientSeq: 0,
    commandId: 0,
    targetX: 0,
    targetY: 0,
    dirX: 0,
    dirY: 0,
    fireViewTick: 0,
  };
}
