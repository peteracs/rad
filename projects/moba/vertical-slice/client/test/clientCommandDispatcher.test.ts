import { strict as assert } from 'node:assert';
import test from 'node:test';
import { ClientCommandDispatcher } from '../src/app/clientCommandDispatcher.js';
import type { ClientInputTransport } from '../src/app/clientInputTransport.js';
import { AckDiagnostics } from '../src/netcode/ackDiagnostics.js';
import { INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS, NET_TICK_HZ } from '../src/netcode/constants.js';
import { ClientInputSequencer } from '../src/netcode/inputSequencer.js';
import {
  PREDICTION_INPUT_KIND_CAST,
  PREDICTION_INPUT_KIND_MOVE,
  PredictionBuffer,
} from '../src/netcode/predictionBuffer.js';

class RecordingInputTransport {
  readonly retransmitSchedules: number[] = [];
  readonly moveSends: {
    seq: number;
    targetTick: number;
    id: number;
    targetX: number;
    targetY: number;
  }[] = [];
  readonly castSends: {
    seq: number;
    targetTick: number;
    id: number;
    dirX: number;
    dirY: number;
    fireViewTick: number;
  }[] = [];

  scheduleRetransmit(now: number): void {
    this.retransmitSchedules.push(now);
  }

  sendFreshMoveOrder(seq: number, targetTick: number, id: number, targetX: number, targetY: number): void {
    this.moveSends.push({ seq, targetTick, id, targetX, targetY });
  }

  sendFreshCast(
    seq: number,
    targetTick: number,
    id: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
  ): void {
    this.castSends.push({ seq, targetTick, id, dirX, dirY, fireViewTick });
  }

  asTransport(): ClientInputTransport {
    return this as unknown as ClientInputTransport;
  }
}

function makeDispatcher() {
  const inputSequencer = new ClientInputSequencer();
  const ackDiagnostics = new AckDiagnostics(INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS);
  const prediction = new PredictionBuffer();
  const transport = new RecordingInputTransport();
  const dispatcher = new ClientCommandDispatcher(
    inputSequencer,
    ackDiagnostics,
    prediction,
    transport.asTransport(),
  );
  return { dispatcher, inputSequencer, ackDiagnostics, prediction, transport };
}

test('queueMove reserves a command, records the prediction input, and sends once', () => {
  const { dispatcher, prediction, transport } = makeDispatcher();

  const input = dispatcher.queueMove(10, 4.5, -2.25, 500);

  assert.equal(input.commandId, 1);
  assert.equal(input.clientSeq, 1);
  assert.equal(input.targetTick, 10 + INPUT_DELAY_TICKS);
  assert.equal(prediction.hasMoveInputAt(input.targetTick), true);
  assert.equal(prediction.inputKindAt(input.targetTick), PREDICTION_INPUT_KIND_MOVE);
  assert.equal(prediction.inputCommandIdAt(input.targetTick), 1);
  assert.equal(prediction.inputTargetXAt(input.targetTick), 4.5);
  assert.equal(prediction.inputTargetYAt(input.targetTick), -2.25);
  assert.deepEqual(transport.retransmitSchedules, [500]);
  assert.deepEqual(transport.moveSends, [
    { seq: 1, targetTick: 10 + INPUT_DELAY_TICKS, id: 1, targetX: 4.5, targetY: -2.25 },
  ]);
});

test('rapid orders at the same base tick get distinct commands on consecutive ticks', () => {
  const { dispatcher, prediction, transport } = makeDispatcher();

  const first = dispatcher.queueMove(10, 1, 1, 0);
  const firstTick = first.targetTick;
  const second = dispatcher.queueMove(10, 2, 2, 1);

  assert.equal(second.commandId, 2);
  assert.equal(second.clientSeq, 2);
  assert.equal(second.targetTick, firstTick + 1, 'target ticks stay strictly monotonic');
  // Both commands live in the ring on their own ticks; nothing was overwritten.
  assert.equal(prediction.inputCommandIdAt(firstTick), 1);
  assert.equal(prediction.inputCommandIdAt(firstTick + 1), 2);
  assert.equal(transport.moveSends.length, 2);
});

test('queueCast records a cast input and sends the cast datagram', () => {
  const { dispatcher, prediction, transport } = makeDispatcher();

  const input = dispatcher.queueCast(20, 0.6, 0.8, 19, 900);

  assert.equal(input.commandId, 1);
  assert.equal(input.targetTick, 20 + INPUT_DELAY_TICKS);
  assert.equal(prediction.hasInputAt(input.targetTick), true);
  assert.equal(prediction.inputKindAt(input.targetTick), PREDICTION_INPUT_KIND_CAST);
  assert.equal(prediction.hasMoveInputAt(input.targetTick), false, 'casts are not replayable moves');
  assert.equal(prediction.inputDirXAt(input.targetTick), 0.6);
  assert.equal(prediction.inputDirYAt(input.targetTick), 0.8);
  assert.equal(prediction.inputFireViewTickAt(input.targetTick), 19);
  assert.deepEqual(transport.retransmitSchedules, [900]);
  assert.deepEqual(transport.castSends, [
    { seq: 1, targetTick: 20 + INPUT_DELAY_TICKS, id: 1, dirX: 0.6, dirY: 0.8, fireViewTick: 19 },
  ]);
});

test('scheduling honors the ack-diagnostics recommended input delay', () => {
  const { dispatcher, ackDiagnostics } = makeDispatcher();

  // Degraded timing (1s round trip) pushes the recommendation to the maximum.
  ackDiagnostics.updateNetworkTiming(1000, 0, NET_TICK_HZ);
  assert.equal(ackDiagnostics.recommendedDelayTicks(), MAX_INPUT_DELAY_TICKS);

  const input = dispatcher.queueMove(10, 0, 0, 0);
  assert.equal(input.targetTick, 10 + MAX_INPUT_DELAY_TICKS);
});
