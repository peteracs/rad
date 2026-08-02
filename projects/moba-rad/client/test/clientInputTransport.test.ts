import { strict as assert } from 'node:assert';
import test from 'node:test';
import { ClientInputTransport } from '../src/app/clientInputTransport.js';
import { ClientNetcodeTelemetry } from '../src/app/clientNetcodeTelemetry.js';
import { AckDiagnostics } from '../src/netcode/ackDiagnostics.js';
import {
  INPUT_DELAY_TICKS,
  INPUT_RETRANSMIT_INTERVAL_MS,
  MAX_INPUT_DELAY_TICKS,
} from '../src/netcode/constants.js';
import { ClientInputSequencer } from '../src/netcode/inputSequencer.js';
import { PredictionBuffer } from '../src/netcode/predictionBuffer.js';
import { createNetcodeDiagnosticsSnapshot } from '../src/netcode/runtimeDiagnostics.js';
import { FakeMatchTransport, flushAsync } from './appTestDoubles.js';

function makeInputTransport() {
  const transport = new FakeMatchTransport();
  const prediction = new PredictionBuffer();
  const inputSequencer = new ClientInputSequencer();
  const receiptAckBits = { lastReceiptAckBits: 0 };
  const ackDiagnostics = new AckDiagnostics(INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS);
  const telemetry = new ClientNetcodeTelemetry();
  const inputTransport = new ClientInputTransport(
    transport,
    prediction,
    inputSequencer,
    receiptAckBits,
    ackDiagnostics,
    telemetry,
  );
  return {
    inputTransport,
    transport,
    prediction,
    inputSequencer,
    receiptAckBits,
    ackDiagnostics,
    telemetry,
  };
}

function telemetrySnapshot(telemetry: ClientNetcodeTelemetry) {
  return telemetry.writeSnapshot(createNetcodeDiagnosticsSnapshot(), 0);
}

test('fresh sends forward the datagrams and count sent packets', () => {
  const { inputTransport, transport, inputSequencer } = makeInputTransport();

  inputTransport.sendFreshMoveOrder(1, 12, 1, 4, 5);
  inputTransport.sendFreshCast(2, 13, 2, 0.6, 0.8, 11);

  assert.deepEqual(transport.moveOrders, [
    { clientSeq: 1, targetTick: 12, commandId: 1, targetX: 4, targetY: 5 },
  ]);
  assert.deepEqual(transport.casts, [
    { clientSeq: 2, targetTick: 13, commandId: 2, dirX: 0.6, dirY: 0.8, fireViewTick: 11 },
  ]);
  assert.equal(inputSequencer.inputPacketsSent, 2);
});

test('unacked move retransmits once synced, paced by the retransmit interval', () => {
  const { inputTransport, transport, prediction, inputSequencer, telemetry } = makeInputTransport();
  prediction.recordMoveInput(30, 1, 1, 8, 9);

  inputTransport.maybeRetransmit(0, false, 10);
  assert.equal(transport.moveOrders.length, 0, 'nothing resends before authority sync');

  inputTransport.maybeRetransmit(0, true, 10);
  assert.deepEqual(transport.moveOrders, [
    { clientSeq: 1, targetTick: 30, commandId: 1, targetX: 8, targetY: 9 },
  ]);
  assert.equal(telemetrySnapshot(telemetry).inputResendPackets, 1);
  assert.equal(inputSequencer.inputPacketsSent, 1);

  inputTransport.maybeRetransmit(1, true, 10);
  assert.equal(transport.moveOrders.length, 1, 'interval gates the next resend');

  inputTransport.maybeRetransmit(INPUT_RETRANSMIT_INTERVAL_MS, true, 10);
  assert.equal(transport.moveOrders.length, 2, 'still-unacked input resends after the interval');
});

test('receipt-acked inputs are not retransmitted', () => {
  const { inputTransport, transport, prediction, ackDiagnostics, receiptAckBits } = makeInputTransport();
  prediction.recordMoveInput(30, 1, 1, 8, 9);

  ackDiagnostics.update(1, 0b1);
  receiptAckBits.lastReceiptAckBits = 0b1;
  inputTransport.maybeRetransmit(0, true, 10);

  assert.equal(transport.moveOrders.length, 0);
});

test('inputs at or before the server tick estimate are too old to resend', () => {
  const { inputTransport, transport, prediction } = makeInputTransport();
  prediction.recordMoveInput(30, 1, 1, 8, 9);

  inputTransport.maybeRetransmit(0, true, 30);
  assert.equal(transport.moveOrders.length, 0);

  inputTransport.maybeRetransmit(INPUT_RETRANSMIT_INTERVAL_MS, true, 29);
  assert.equal(transport.moveOrders.length, 1, 'tick 30 is still pending for server tick 29');
});

test('cast inputs retransmit through the cast datagram', () => {
  const { inputTransport, transport, prediction, telemetry } = makeInputTransport();
  prediction.recordCastInput(40, 3, 5, 0.6, -0.8, 38);

  inputTransport.maybeRetransmit(0, true, 20);

  assert.equal(transport.moveOrders.length, 0);
  assert.deepEqual(transport.casts, [
    { clientSeq: 3, targetTick: 40, commandId: 5, dirX: 0.6, dirY: -0.8, fireViewTick: 38 },
  ]);
  assert.equal(telemetrySnapshot(telemetry).inputResendPackets, 1);
});

test('closed transport sends nothing', () => {
  const { inputTransport, transport, prediction } = makeInputTransport();
  prediction.recordMoveInput(30, 1, 1, 8, 9);

  inputTransport.close();
  inputTransport.sendFreshMoveOrder(1, 12, 1, 4, 5);
  inputTransport.sendFreshCast(2, 13, 2, 1, 0, 11);
  inputTransport.maybeRetransmit(1000, true, 0);

  assert.equal(transport.moveOrders.length, 0);
  assert.equal(transport.casts.length, 0);
});

test('datagram send failures are routed to telemetry', async () => {
  const { inputTransport, transport, telemetry } = makeInputTransport();
  transport.sendFailure = new Error('datagram send failed');

  inputTransport.sendFreshMoveOrder(1, 12, 1, 4, 5);
  await flushAsync();

  const diag = telemetrySnapshot(telemetry);
  assert.equal(diag.transportFailures, 1);
  assert.equal(diag.lastTransportError, 'datagram send failed');
});
