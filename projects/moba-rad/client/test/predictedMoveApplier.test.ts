import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  PredictedMoveApplier,
  type PredictedMoveTarget,
} from '../src/netcode/predictedMoveApplier.js';
import { PredictionBuffer } from '../src/netcode/predictionBuffer.js';

class FakeMoveTarget implements PredictedMoveTarget {
  readonly commandIds: number[] = [];
  readonly targetXs: number[] = [];
  readonly targetYs: number[] = [];
  ticks = 0;

  moveOrder(_playerId: number, commandId: number, targetX: number, targetY: number): void {
    this.commandIds.push(commandId);
    this.targetXs.push(targetX);
    this.targetYs.push(targetY);
  }

  tickFixed(): void {
    this.ticks += 1;
  }
}

test('predicted move applier re-arms locally applied moves before rollback replay', () => {
  const prediction = new PredictionBuffer();
  const applier = new PredictedMoveApplier(prediction);
  const target = new FakeMoveTarget();

  prediction.recordMoveInput(10, 1, 700, 12.5, -3.25);
  assert.equal(applier.applyDueMovesAtOrBefore(10, target, 1), 1);
  assert.equal(applier.applyDueMovesAtOrBefore(10, target, 1), 0);

  assert.equal(applier.replayWindow(10, 12, target, 1), 1);
  assert.deepEqual(target.commandIds, [700, 700]);
  assert.deepEqual(target.targetXs, [12.5, 12.5]);
  assert.deepEqual(target.targetYs, [-3.25, -3.25]);
  assert.equal(target.ticks, 3);
});

test('predicted move applier does not replay cast records as local movement', () => {
  const prediction = new PredictionBuffer();
  const applier = new PredictedMoveApplier(prediction);
  const target = new FakeMoveTarget();

  prediction.recordMoveInput(20, 1, 800, 4, 5);
  prediction.recordCastInput(21, 2, 801, 1, 0, 19);

  assert.equal(applier.replayWindow(20, 22, target, 1), 1);
  assert.deepEqual(target.commandIds, [800]);
  assert.equal(target.ticks, 3);
});

test('predicted move applier rejects invalid tick windows without draining inputs', () => {
  const prediction = new PredictionBuffer();
  const applier = new PredictedMoveApplier(prediction);
  const target = new FakeMoveTarget();

  prediction.recordMoveInput(30, 1, 900, 6, 7);

  assert.equal(applier.applyDueMovesAtOrBefore(Number.NaN, target, 1), 0);
  assert.equal(applier.replayWindow(30, Number.POSITIVE_INFINITY, target, 1), 0);
  assert.equal(applier.applyDueMovesAtOrBefore(30, target, 1), 1);
  assert.deepEqual(target.commandIds, [900]);
});
