import assert from 'node:assert/strict';
import test from 'node:test';
import { AvatarInterpolator } from '../src/render/avatarInterpolator.js';
import type { AvatarRenderState } from '../src/render/worldView.js';

function createTestAvatarState(): AvatarRenderState {
  return {
    model: 'test',
    x: 0,
    y: 0,
    targetX: 0,
    targetY: 0,
    targetActive: false,
    commandId: 0,
  };
}

test('interpolates visual avatar position between the two latest physics samples', () => {
  const previous = createTestAvatarState();
  const current = createTestAvatarState();
  const visual = createTestAvatarState();
  const first = createTestAvatarState();
  const second = createTestAvatarState();
  const interpolator = new AvatarInterpolator(previous, current);

  first.x = 0;
  first.y = 10;
  first.targetX = 5;
  first.targetY = 6;
  first.targetActive = true;
  first.commandId = 7;

  second.x = 8;
  second.y = 18;
  second.targetX = 12;
  second.targetY = 14;
  second.targetActive = false;
  second.commandId = 8;

  interpolator.pushSample(100, first);
  interpolator.pushSample(101, second);

  assert.equal(interpolator.writeVisualState(0.25, visual), true);
  assert.equal(visual.x, 2);
  assert.equal(visual.y, 12);
  assert.equal(visual.targetX, 12);
  assert.equal(visual.targetY, 14);
  assert.equal(visual.targetActive, false);
  assert.equal(visual.commandId, 8);
});

test('reuses caller-owned render states and clamps interpolation alpha', () => {
  const previous = createTestAvatarState();
  const current = createTestAvatarState();
  const visual = createTestAvatarState();
  const first = createTestAvatarState();
  const second = createTestAvatarState();
  const interpolator = new AvatarInterpolator(previous, current);

  first.x = -2;
  second.x = 6;
  interpolator.pushSample(1, first);
  interpolator.pushSample(2, second);

  assert.equal(interpolator.writeVisualState(2, visual), true);
  assert.equal(visual.x, 6);
  assert.equal(interpolator.writeVisualState(-1, visual), true);
  assert.equal(visual.x, -2);
});
