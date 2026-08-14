import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { FixedStepClock } from '../demo/session/fixedStepClock.js';

test('fixed-step clock decouples simulation cadence and bounds catch-up', () => {
  const clock = new FixedStepClock({
    stepSeconds: 0.01,
    maxFrameSeconds: 0.05,
    maxStepsPerFrame: 3,
  });
  assert.deepEqual(clock.advance(0.005), { steps: 0, droppedSeconds: 0 });
  assert.deepEqual(clock.advance(0.015), { steps: 2, droppedSeconds: 0 });
  assert.deepEqual(clock.advance(0.1), { steps: 3, droppedSeconds: 0.07 });
});

test('fixed-step clock rejects invalid profiles and elapsed time', () => {
  assert.throws(
    () => new FixedStepClock({ stepSeconds: 0, maxFrameSeconds: 1, maxStepsPerFrame: 1 }),
    /invalid_fixed_step/,
  );
  const clock = new FixedStepClock({
    stepSeconds: 0.01,
    maxFrameSeconds: 1,
    maxStepsPerFrame: 1,
  });
  assert.throws(() => clock.advance(-1), /invalid_elapsed_seconds/);
});
