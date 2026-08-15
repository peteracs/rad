import { strict as assert } from 'node:assert';
import test from 'node:test';
import { NetcodeLogger } from '../src/netcode/netcodeLogger.js';
import {
  createNetcodeDiagnosticsSnapshot,
  type NetcodeDiagnosticsSnapshot,
} from '../src/netcode/runtimeDiagnostics.js';

test('netcode logger emits one interval line per 128 simulated ticks', () => {
  const source = new FakeDiagnosticsSource();
  const sink = new ArraySink();
  const logger = new NetcodeLogger(source, { enabled: true, sink });

  logger.sample(0);
  source.advanceTicks(127);
  source.snapshot.correctionCount = 3;
  source.snapshot.lateInputs = 2;
  logger.sample(1000);

  assert.equal(sink.lines.length, 0);

  source.advanceTicks(1);
  logger.sample(1010);

  assert.equal(sink.lines.length, 1);
  assert.match(sink.lines[0], /\[00:01\] Ticks: 128/);
  assert.match(sink.lines[0], /Reconciles: 3\/128 \(2\.3%\)/);
  assert.match(sink.lines[0], /LateInputs: 2/);
});

test('netcode logger caps per-tick reconcile rate while preserving correction event count', () => {
  const source = new FakeDiagnosticsSource();
  const sink = new ArraySink();
  const logger = new NetcodeLogger(source, { enabled: true, sink });

  logger.sample(0);
  source.advanceTicks(128);
  source.snapshot.correctionCount = 130;
  logger.sample(1000);

  assert.equal(sink.lines.length, 1);
  assert.match(sink.lines[0], /Reconciles: 128\/128 \(100\.0%\)/);
  assert.match(sink.lines[0], /CorrectionEvents: 130/);
});

test('netcode logger writes a teardown summary with totals and pool peaks', () => {
  const source = new FakeDiagnosticsSource();
  const sink = new ArraySink();
  const logger = new NetcodeLogger(source, { enabled: true, sink });

  logger.sample(0);
  source.advanceTicks(256);
  source.snapshot.roundTripMs = 8;
  source.snapshot.jitterMs = 3;
  source.snapshot.inspectedPackets = 100;
  source.snapshot.missingPackets = 5;
  source.snapshot.correctionCount = 4;
  source.snapshot.maxCorrectionDistance = 0.75;
  source.snapshot.inputPacketsSent = 42;
  source.snapshot.rejectedStatePackets = 1;
  source.snapshot.staleStatePackets = 6;
  source.snapshot.remoteAvatarPoolActive = 3;
  source.snapshot.remoteAvatarPoolIdle = 61;
  source.snapshot.projectilePoolActive = 2;
  source.snapshot.projectilePoolIdle = 94;
  logger.sample(2000);
  logger.close(2200);

  const summary = sink.lines[sink.lines.length - 1];
  assert.match(summary, /NETCODE REPORT/);
  assert.match(summary, /Duration:\s+2\.2 seconds \(256 ticks\)/);
  assert.match(summary, /Total Packet Loss: 5\.0% \(5 \/ 100 packets\)/);
  assert.match(summary, /Total Corrections: 4 \(1\.6% error rate\)/);
  assert.match(summary, /Max Correction Dist: 0\.75 units/);
  assert.match(summary, /Total Inputs Sent: 42/);
  assert.match(summary, /Max Active Avatars: 3 \(pool capacity: 64\)/);
  assert.match(summary, /Max Active Projs:\s+2 \(pool capacity: 96\)/);
});

class FakeDiagnosticsSource {
  readonly snapshot = createNetcodeDiagnosticsSnapshot();

  constructor() {
    this.snapshot.remoteAvatarPoolIdle = 64;
    this.snapshot.projectilePoolIdle = 96;
  }

  advanceTicks(delta: number): void {
    this.snapshot.localTick += delta;
  }

  writeNetcodeDiagnostics(out: NetcodeDiagnosticsSnapshot): NetcodeDiagnosticsSnapshot {
    Object.assign(out, this.snapshot);
    return out;
  }
}

class ArraySink {
  readonly lines: string[] = [];

  write(line: string): void {
    this.lines.push(line);
  }
}
