import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  createReconciliationDecision,
  ReconciliationPolicy,
} from '../src/netcode/reconciliationPolicy.js';

test('reconciliation policy ignores older authority echoes while local command is active', () => {
  const policy = new ReconciliationPolicy();
  const decision = policy.decide(
    12,
    true,
    12,
    true,
    11,
    false,
    true,
    100,
    createReconciliationDecision(),
  );

  assert.equal(decision.ignoreOlderCommand, true);
  assert.equal(decision.shouldReconcile, false);
  assert.equal(decision.smoothCorrection, false);
});

test('reconciliation policy accepts matching predicted authority state without replay', () => {
  const policy = new ReconciliationPolicy();
  const decision = policy.decide(
    12,
    true,
    12,
    false,
    12,
    false,
    true,
    0.0001,
    createReconciliationDecision(),
  );

  assert.equal(decision.ignoreOlderCommand, false);
  assert.equal(decision.positionMismatch, false);
  assert.equal(decision.targetMismatch, false);
  assert.equal(decision.shouldReconcile, false);
});

test('reconciliation policy replays when prediction history is missing', () => {
  const policy = new ReconciliationPolicy();
  const decision = policy.decide(
    12,
    true,
    12,
    false,
    12,
    false,
    false,
    Number.POSITIVE_INFINITY,
    createReconciliationDecision(),
  );

  assert.equal(decision.positionMismatch, true);
  assert.equal(decision.shouldReconcile, true);
  assert.equal(decision.smoothCorrection, false);
});

test('reconciliation policy replays target-active mismatch for the same command', () => {
  const policy = new ReconciliationPolicy();
  const decision = policy.decide(
    12,
    true,
    12,
    true,
    12,
    false,
    true,
    0,
    createReconciliationDecision(),
  );

  assert.equal(decision.targetMismatch, true);
  assert.equal(decision.shouldReconcile, true);
  assert.equal(decision.smoothCorrection, false);
});

test('reconciliation policy distinguishes soft and hard position corrections', () => {
  const policy = new ReconciliationPolicy();
  const out = createReconciliationDecision();

  const soft = policy.decide(12, true, 12, false, 12, false, true, 0.2 * 0.2, out);
  assert.equal(soft.shouldReconcile, true);
  assert.equal(soft.smoothCorrection, true);
  assert.equal(soft.hardCorrection, false);
  assert.equal(soft.correctionDistance, 0.2);

  const hard = policy.decide(12, true, 12, false, 12, false, true, 0.75 * 0.75, out);
  assert.equal(hard.shouldReconcile, true);
  assert.equal(hard.smoothCorrection, true);
  assert.equal(hard.hardCorrection, true);
  assert.equal(hard.correctionDistance, 0.75);
});

test('reconciliation policy treats non-finite prediction error as missing history', () => {
  const policy = new ReconciliationPolicy();
  const decision = policy.decide(
    12,
    true,
    12,
    false,
    12,
    false,
    true,
    Number.NaN,
    createReconciliationDecision(),
  );

  assert.equal(decision.positionMismatch, true);
  assert.equal(decision.shouldReconcile, true);
  assert.equal(decision.smoothCorrection, false);
});
