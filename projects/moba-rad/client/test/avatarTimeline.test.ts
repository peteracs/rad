import assert from 'node:assert/strict';
import test from 'node:test';
import { AvatarTimeline } from '../src/render/avatarTimeline.js';
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

function createTimeline(): AvatarTimeline {
  return new AvatarTimeline([
    createTestAvatarState(),
    createTestAvatarState(),
    createTestAvatarState(),
    createTestAvatarState(),
  ]);
}

test('interpolates remote avatar state at a delayed render tick', () => {
  const timeline = createTimeline();
  const visual = createTestAvatarState();
  const first = createTestAvatarState();
  const second = createTestAvatarState();

  first.x = 10;
  first.y = -2;
  second.x = 18;
  second.y = 6;
  second.targetX = 40;
  second.targetActive = true;
  second.commandId = 3;

  timeline.pushSample(100, first);
  timeline.pushSample(108, second);

  assert.equal(timeline.writeVisualStateAt(104, visual), true);
  assert.equal(visual.x, 14);
  assert.equal(visual.y, 2);
  assert.equal(visual.targetX, 40);
  assert.equal(visual.targetActive, true);
  assert.equal(visual.commandId, 3);
});

test('handles out-of-order samples without allocating sorted snapshots', () => {
  const timeline = createTimeline();
  const visual = createTestAvatarState();
  const late = createTestAvatarState();
  const early = createTestAvatarState();

  late.x = 30;
  early.x = 10;
  timeline.pushSample(120, late);
  timeline.pushSample(100, early);

  assert.equal(timeline.writeVisualStateAt(110, visual), true);
  assert.equal(visual.x, 20);
  assert.equal(timeline.latestSampleTick(), 120);
});
