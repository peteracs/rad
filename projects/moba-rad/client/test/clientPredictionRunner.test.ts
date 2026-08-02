import { strict as assert } from 'node:assert';
import test from 'node:test';
import { ClientPredictionRunner } from '../src/app/clientPredictionRunner.js';
import { MAX_CLIENT_CATCHUP_TICKS } from '../src/netcode/constants.js';
import { PredictionBuffer } from '../src/netcode/predictionBuffer.js';
import { createAvatarRenderState } from '../src/render/worldView.js';
import { FakeRadSession, FakeScene, makeServerState } from './appTestDoubles.js';

const PLAYER_ID = 7;

function makeRunner(session: FakeRadSession) {
  const scene = new FakeScene();
  const prediction = new PredictionBuffer();
  const runner = new ClientPredictionRunner(
    session.asSession(),
    PLAYER_ID,
    scene.asScene(),
    prediction,
  );
  return { runner, scene, prediction };
}

test('prediction runner simulates queued moves tick by tick and goes idle at the target', () => {
  const session = new FakeRadSession(PLAYER_ID, 0, 0);
  const { runner, scene, prediction } = makeRunner(session);

  prediction.recordMoveInput(2, 1, 1, 3, 0);
  runner.markActive();
  runner.advanceToTick(5);

  assert.deepEqual(session.moveOrders, [
    { playerId: PLAYER_ID, commandId: 1, targetX: 3, targetY: 0 },
  ]);
  // Ticks 1-4 simulate; the runner stops integrating once the target is
  // reached, so tick 5 advances the frontier without a fixed tick.
  assert.equal(session.tickFixedCalls, 4);
  assert.deepEqual(
    scene.avatarSamples.map((sample) => ({ tick: sample.tick, x: sample.state?.x })),
    [
      { tick: 1, x: 0 },
      { tick: 2, x: 1 },
      { tick: 3, x: 2 },
      { tick: 4, x: 3 },
    ],
  );
  assert.equal(runner.active, false);
  assert.equal(prediction.hasPositionAt(4), true);
  assert.equal(prediction.hasPositionAt(5), false);

  const out = createAvatarRenderState();
  assert.equal(runner.writeLocalAvatarState(out), true);
  assert.equal(out.x, 3);
  assert.equal(out.y, 0);
  assert.equal(out.targetActive, false);
});

test('prediction runner jumps the frontier while idle instead of simulating the gap', () => {
  const session = new FakeRadSession(PLAYER_ID, 0, 0);
  const { runner, scene, prediction } = makeRunner(session);

  runner.advanceToTick(6);
  assert.equal(session.tickFixedCalls, 0);
  assert.equal(scene.avatarSamples.length, 0);

  // The frontier really moved: activating afterwards only simulates the new span.
  prediction.recordMoveInput(7, 1, 1, 5, 0);
  runner.markActive();
  runner.advanceToTick(8);

  assert.equal(session.tickFixedCalls, 2);
  assert.deepEqual(
    scene.avatarSamples.map((sample) => ({ tick: sample.tick, x: sample.state?.x })),
    [
      { tick: 7, x: 1 },
      { tick: 8, x: 2 },
    ],
  );
});

test('prediction runner hard-jumps gaps beyond the catch-up budget without ticking', () => {
  const session = new FakeRadSession(PLAYER_ID, 0, 0);
  const { runner, scene, prediction } = makeRunner(session);

  prediction.recordMoveInput(1, 1, 1, 50, 0);
  runner.markActive();
  runner.advanceToTick(MAX_CLIENT_CATCHUP_TICKS + 2);

  assert.equal(session.tickFixedCalls, 0);
  assert.equal(scene.avatarSamples.length, 0);

  // One more tick resumes normal simulation from the jumped frontier.
  runner.advanceToTick(MAX_CLIENT_CATCHUP_TICKS + 3);
  assert.equal(session.tickFixedCalls, 1);
  assert.deepEqual(
    scene.avatarSamples.map((sample) => sample.tick),
    [MAX_CLIENT_CATCHUP_TICKS + 3],
  );
});

test('authoritative apply and replay converges onto the corrected authority state', () => {
  const session = new FakeRadSession(PLAYER_ID, 0, 0);
  const { runner, scene, prediction } = makeRunner(session);

  prediction.recordMoveInput(3, 1, 1, 5, 0);
  runner.markActive();
  runner.advanceToTick(6);
  const beforeReplayTicks = session.tickFixedCalls;
  assert.equal(beforeReplayTicks, 6);

  const out = createAvatarRenderState();
  runner.writeLocalAvatarState(out);
  assert.equal(out.x, 4, 'client predicted 4 units of travel by tick 6');

  // Authority disagrees about tick 4: the avatar is at x=1.5, still moving.
  const state = makeServerState({
    serverTick: 4,
    avatar: { x: 1.5, y: 0, target_x: 5, target_y: 0, target_active: true, command_id: 1 },
  });
  runner.applyAuthoritativeStateAndReplay(state, 5, 8);

  assert.deepEqual(session.authorityStatesApplied, [
    { x: 1.5, y: 0, targetX: 5, targetY: 0, targetActive: true, commandId: 1 },
  ]);
  // Replay integrates ticks 5-8 on top of the authority state: 1.5 -> 2.5 ->
  // 3.5 -> 4.5 -> arrival at 5.
  assert.equal(session.tickFixedCalls, beforeReplayTicks + 4);
  runner.writeLocalAvatarState(out);
  assert.equal(out.x, 5);
  assert.equal(out.targetActive, false);

  const lastSample = scene.avatarSamples[scene.avatarSamples.length - 1];
  assert.equal(lastSample.tick, 8);
  assert.equal(lastSample.state?.x, 5);
  assert.equal(prediction.hasPositionAt(8), true);
  assert.equal(prediction.positionErrorSq(8, 5, 0), 0);

  // The replay window is the new simulation frontier; re-advancing to it is a no-op.
  runner.advanceToTick(8);
  assert.equal(session.tickFixedCalls, beforeReplayTicks + 4);
});

test('prediction runner warns once and emits null samples when the controlled avatar is missing', () => {
  const session = new FakeRadSession(99, 0, 0);
  const { runner, scene, prediction } = makeRunner(session);

  const errors: string[] = [];
  const originalError = console.error;
  console.error = (...args: unknown[]) => {
    errors.push(args.map(String).join(' '));
  };
  try {
    prediction.recordMoveInput(1, 1, 1, 4, 0);
    runner.markActive();
    runner.advanceToTick(2);
  } finally {
    console.error = originalError;
  }

  assert.equal(runner.writeLocalAvatarState(createAvatarRenderState()), false);
  assert.deepEqual(scene.avatarSamples.map((sample) => sample.state), [null, null]);

  const warnings = errors.filter((message) =>
    message.includes(`controlled avatar for player_id=${PLAYER_ID}`),
  );
  assert.equal(warnings.length, 1, 'missing-avatar diagnostic fires exactly once');
  assert.ok(warnings[0].includes('present player ids: [99]'));
});

test('refreshSceneSample records the current world state without simulating', () => {
  const session = new FakeRadSession(PLAYER_ID, 2, 3);
  const { runner, scene, prediction } = makeRunner(session);

  runner.refreshSceneSample(9);

  assert.equal(session.tickFixedCalls, 0);
  assert.deepEqual(
    scene.avatarSamples.map((sample) => ({
      tick: sample.tick,
      x: sample.state?.x,
      y: sample.state?.y,
    })),
    [{ tick: 9, x: 2, y: 3 }],
  );
  assert.equal(prediction.hasPositionAt(9), true);
});
