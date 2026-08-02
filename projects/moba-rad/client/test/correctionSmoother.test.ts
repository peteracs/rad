import assert from 'node:assert/strict';
import test from 'node:test';
import { CorrectionSmoother } from '../src/render/correctionSmoother.js';

test('blends visual correction from the rendered position toward authority', () => {
  const smoother = new CorrectionSmoother();
  const out = { x: 0, y: 0 };

  smoother.start(10, -4, 1000, 100);

  assert.equal(smoother.write(20, 6, 1000, out), true);
  assert.equal(out.x, 10);
  assert.equal(out.y, -4);

  assert.equal(smoother.write(20, 6, 1050, out), true);
  assert.equal(out.x, 15);
  assert.equal(out.y, 1);

  assert.equal(smoother.write(20, 6, 1100, out), false);
});
