import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  ClientInputSequencer,
  createClientInputReservation,
} from '../src/netcode/inputSequencer.js';

test('client input sequencer reserves monotonic command ids, client seqs, and target ticks', () => {
  const sequencer = new ClientInputSequencer();
  const out = createClientInputReservation();

  sequencer.reserveInputCommand(100, 2, out);
  assert.equal(out.commandId, 1);
  assert.equal(out.clientSeq, 1);
  assert.equal(out.targetTick, 102);
  assert.equal(out.predictionLeadTicks, 2);

  sequencer.reserveInputCommand(100, 2, out);
  assert.equal(out.commandId, 2);
  assert.equal(out.clientSeq, 2);
  assert.equal(out.targetTick, 103);
  assert.equal(out.predictionLeadTicks, 3);
});

test('client input sequencer refuses silent seq wrap and counts only sent input packets', () => {
  const sequencer = new ClientInputSequencer(3);
  const out = createClientInputReservation();

  sequencer.reserveInputCommand(10, 1, out);
  assert.equal(out.clientSeq, 1);
  sequencer.reserveInputCommand(11, 1, out);
  assert.equal(out.clientSeq, 2);
  assert.equal(sequencer.reserveClientSeq(), 3);
  assert.throws(
    () => sequencer.reserveClientSeq(),
    /Client sequence exhausted/,
    'live sessions must not silently wrap seq ids the RAD ACK window cannot prove',
  );
  assert.equal(sequencer.inputPacketsSent, 0);

  sequencer.noteInputPacketSent();
  sequencer.noteInputPacketSent();
  assert.equal(sequencer.inputPacketsSent, 2);
});
