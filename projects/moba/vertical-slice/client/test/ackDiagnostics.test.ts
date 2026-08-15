import assert from 'node:assert/strict';
import test from 'node:test';
import { AckDiagnostics } from '../src/netcode/ackDiagnostics.js';

test('keeps base input delay when ack window is clean', () => {
  const diagnostics = new AckDiagnostics(2, 6);
  const snapshot = diagnostics.update(4, 0b1111);

  assert.equal(snapshot.highestAck, 4);
  assert.equal(snapshot.missingPackets, 0);
  assert.equal(snapshot.recommendedInputDelayTicks, 2);
});

test('raises recommended input delay as packet loss increases', () => {
  const diagnostics = new AckDiagnostics(2, 6);
  diagnostics.update(8, 0b1111_1111);
  const snapshot = diagnostics.update(16, 0b1010_1010);

  assert.equal(snapshot.highestAck, 16);
  assert.equal(snapshot.missingPackets, 4);
  assert.equal(snapshot.recommendedInputDelayTicks, 6);
});

test('ignores stale ack snapshots', () => {
  const diagnostics = new AckDiagnostics(2, 6);
  diagnostics.update(10, 0b1111_1111_11);
  const snapshot = diagnostics.update(9, 0);

  assert.equal(snapshot.highestAck, 10);
  assert.equal(snapshot.missingPackets, 0);
});

test('does not count out-of-window sequence ids as inspected', () => {
  const diagnostics = new AckDiagnostics(2, 6);
  const snapshot = diagnostics.update(64, 0xffff_ffff);

  assert.equal(snapshot.highestAck, 64);
  assert.equal(snapshot.inspectedPackets, 32);
  assert.equal(snapshot.lossRatio, 0);
});

test('raises input delay from RTT and jitter timing', () => {
  const diagnostics = new AckDiagnostics(2, 24);
  const snapshot = diagnostics.updateNetworkTiming(120, 15, 128);

  assert.equal(snapshot.recommendedInputDelayTicks, 12);
});

test('keeps localhost timing responsive', () => {
  const diagnostics = new AckDiagnostics(2, 24);
  const snapshot = diagnostics.updateNetworkTiming(16, 0, 128);

  assert.equal(snapshot.recommendedInputDelayTicks, 4);
});

test('clamps timing delay to the configured max', () => {
  const diagnostics = new AckDiagnostics(2, 24);
  const snapshot = diagnostics.updateNetworkTiming(500, 100, 128);

  assert.equal(snapshot.recommendedInputDelayTicks, 24);
});

test('uses the larger delay from loss or network timing', () => {
  const diagnostics = new AckDiagnostics(2, 6);
  diagnostics.updateNetworkTiming(16, 0, 128);
  const snapshot = diagnostics.update(16, 0b1010_1010);

  assert.equal(snapshot.recommendedInputDelayTicks, 6);
});
