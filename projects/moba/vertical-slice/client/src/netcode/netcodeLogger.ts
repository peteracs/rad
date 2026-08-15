import { NET_TICK_HZ } from './constants.js';
import {
  createNetcodeDiagnosticsSnapshot,
  type NetcodeDiagnosticsSnapshot,
} from './runtimeDiagnostics.js';

const INTERVAL_TICKS = NET_TICK_HZ;

export interface NetcodeDiagnosticsSource {
  writeNetcodeDiagnostics(out: NetcodeDiagnosticsSnapshot): NetcodeDiagnosticsSnapshot;
}

export interface NetcodeLoggerSink {
  write(line: string): void;
}

export interface NetcodeLoggerOptions {
  enabled?: boolean;
  sink?: NetcodeLoggerSink;
}

const CONSOLE_SINK: NetcodeLoggerSink = {
  write(line: string): void {
    console.info(line);
  },
};

export class NetcodeLogger {
  private readonly snapshot = createNetcodeDiagnosticsSnapshot();
  private readonly sink: NetcodeLoggerSink;
  private readonly enabled: boolean;
  private started = false;
  private closed = false;
  private startedAtMs = 0;
  private lastTick = 0;
  private lastCorrections = 0;
  private lastLateInputs = 0;
  private lastInspectedPackets = 0;
  private lastMissingPackets = 0;
  private lastStaleSnapshots = 0;
  private lastRejectedInputs = 0;
  private intervalTicks = 0;
  private intervalCorrections = 0;
  private intervalLateInputs = 0;
  private totalTicks = 0;
  private totalCorrections = 0;
  private totalLateInputs = 0;
  private totalRejectedInputs = 0;
  private totalInspectedPackets = 0;
  private totalMissingPackets = 0;
  private totalStaleSnapshots = 0;
  private pingWeightedSum = 0;
  private jitterWeightedSum = 0;
  private pingWeightTicks = 0;
  private maxActiveAvatars = 0;
  private maxAvatarCapacity = 0;
  private maxActiveProjectiles = 0;
  private maxProjectileCapacity = 0;

  constructor(
    private readonly source: NetcodeDiagnosticsSource,
    options: NetcodeLoggerOptions = {},
  ) {
    this.enabled = options.enabled === true;
    this.sink = options.sink ?? CONSOLE_SINK;
  }

  sample(nowMs: number): void {
    if (!this.enabled || this.closed) return;
    const snapshot = this.source.writeNetcodeDiagnostics(this.snapshot);
    if (!this.started) {
      this.start(nowMs, snapshot);
      return;
    }

    const tickDelta = Math.max(0, snapshot.localTick - this.lastTick);
    const correctionDelta = Math.max(0, snapshot.correctionCount - this.lastCorrections);
    const lateInputDelta = Math.max(0, snapshot.lateInputs - this.lastLateInputs);
    const inspectedDelta = Math.max(0, snapshot.inspectedPackets - this.lastInspectedPackets);
    const missingDelta = Math.max(0, snapshot.missingPackets - this.lastMissingPackets);
    const staleDelta = Math.max(0, snapshot.staleStatePackets - this.lastStaleSnapshots);
    const rejectedDelta = Math.max(0, snapshot.rejectedStatePackets - this.lastRejectedInputs);

    this.intervalTicks += tickDelta;
    this.intervalCorrections += correctionDelta;
    this.intervalLateInputs += lateInputDelta;
    this.totalTicks += tickDelta;
    this.totalCorrections += correctionDelta;
    this.totalLateInputs += lateInputDelta;
    this.totalRejectedInputs += rejectedDelta;
    this.totalInspectedPackets += inspectedDelta;
    this.totalMissingPackets += missingDelta;
    this.totalStaleSnapshots += staleDelta;

    if (tickDelta > 0) {
      this.pingWeightedSum += snapshot.roundTripMs * tickDelta;
      this.jitterWeightedSum += snapshot.jitterMs * tickDelta;
      this.pingWeightTicks += tickDelta;
    }

    this.maxActiveAvatars = Math.max(this.maxActiveAvatars, snapshot.remoteAvatarPoolActive);
    this.maxAvatarCapacity = Math.max(
      this.maxAvatarCapacity,
      snapshot.remoteAvatarPoolActive + snapshot.remoteAvatarPoolIdle,
    );
    this.maxActiveProjectiles = Math.max(this.maxActiveProjectiles, snapshot.projectilePoolActive);
    this.maxProjectileCapacity = Math.max(
      this.maxProjectileCapacity,
      snapshot.projectilePoolActive + snapshot.projectilePoolIdle,
    );
    this.recordBaselines(snapshot);

    while (this.intervalTicks >= INTERVAL_TICKS) {
      this.writeIntervalLine(snapshot);
      this.intervalTicks -= INTERVAL_TICKS;
      this.intervalCorrections = 0;
      this.intervalLateInputs = 0;
    }
  }

  close(nowMs: number): void {
    if (!this.enabled || this.closed) return;
    if (!this.started) {
      this.start(nowMs, this.source.writeNetcodeDiagnostics(this.snapshot));
    } else {
      this.sample(nowMs);
    }
    this.closed = true;
    this.writeSummary(nowMs, this.snapshot);
  }

  private start(nowMs: number, snapshot: NetcodeDiagnosticsSnapshot): void {
    this.started = true;
    this.startedAtMs = nowMs;
    this.lastTick = snapshot.localTick;
    this.recordBaselines(snapshot);
  }

  private recordBaselines(snapshot: NetcodeDiagnosticsSnapshot): void {
    this.lastTick = snapshot.localTick;
    this.lastCorrections = snapshot.correctionCount;
    this.lastLateInputs = snapshot.lateInputs;
    this.lastInspectedPackets = snapshot.inspectedPackets;
    this.lastMissingPackets = snapshot.missingPackets;
    this.lastStaleSnapshots = snapshot.staleStatePackets;
    this.lastRejectedInputs = snapshot.rejectedStatePackets;
  }

  private writeIntervalLine(snapshot: NetcodeDiagnosticsSnapshot): void {
    const reconciledTicks = Math.min(this.intervalCorrections, INTERVAL_TICKS);
    const correctionRatio = ratio(reconciledTicks, INTERVAL_TICKS);
    const correctionEvents =
      this.intervalCorrections > INTERVAL_TICKS ? ` | CorrectionEvents: ${this.intervalCorrections}` : '';
    this.sink.write(
      `[${formatElapsed(this.totalTicks)}]`
        + ` Ticks: ${INTERVAL_TICKS}`
        + ` | Ping: ${snapshot.roundTripMs.toFixed(0)}ms (jit: ${snapshot.jitterMs.toFixed(0)}ms)`
        + ` | Loss: ${(snapshot.lossRatio * 100).toFixed(1)}%`
        + ` | Reconciles: ${reconciledTicks}/${INTERVAL_TICKS} (${correctionRatio})`
        + correctionEvents
        + ` | LateInputs: ${this.intervalLateInputs}`
        + ` | Meshes: A_act:${snapshot.remoteAvatarPoolActive}/${avatarCapacity(snapshot)}`
        + ` P_act:${snapshot.projectilePoolActive}/${projectileCapacity(snapshot)}`,
    );
  }

  private writeSummary(nowMs: number, snapshot: NetcodeDiagnosticsSnapshot): void {
    const durationSeconds = Math.max(0, (nowMs - this.startedAtMs) / 1000);
    const avgPing = this.pingWeightTicks > 0 ? this.pingWeightedSum / this.pingWeightTicks : snapshot.roundTripMs;
    const avgJitter = this.pingWeightTicks > 0 ? this.jitterWeightedSum / this.pingWeightTicks : snapshot.jitterMs;
    const correctionRate = ratio(this.totalCorrections, Math.max(1, this.totalTicks));
    const rejectedRate = ratio(this.totalRejectedInputs, Math.max(1, snapshot.inputPacketsSent));
    const packetLoss = ratio(this.totalMissingPackets, Math.max(1, this.totalInspectedPackets));

    this.sink.write([
      '================ NETCODE REPORT ================',
      `Duration:          ${durationSeconds.toFixed(1)} seconds (${this.totalTicks} ticks)`,
      `Avg Ping / Jitter: ${avgPing.toFixed(1)}ms / ${avgJitter.toFixed(1)}ms`,
      `Total Packet Loss: ${packetLoss} (${this.totalMissingPackets} / ${this.totalInspectedPackets} packets)`,
      '',
      'PREDICTION STABILITY:',
      `- Predicted Ticks: ${this.totalTicks}`,
      `- Total Corrections: ${this.totalCorrections} (${correctionRate} error rate)`,
      `- Max Correction Dist: ${snapshot.maxCorrectionDistance.toFixed(2)} units`,
      '',
      'INPUT QUEUE HEALTH (SERVER):',
      `- Total Inputs Sent: ${snapshot.inputPacketsSent}`,
      `- Stale Snapshots:   ${this.totalStaleSnapshots}`,
      `- Rejected Inputs:   ${this.totalRejectedInputs} (${rejectedRate} arrived too late / invalid)`,
      `- Late Inputs:       ${this.totalLateInputs}`,
      '',
      'RESOURCE POOLING:',
      `- Max Active Avatars: ${this.maxActiveAvatars} (pool capacity: ${this.maxAvatarCapacity})`,
      `- Max Active Projs:   ${this.maxActiveProjectiles} (pool capacity: ${this.maxProjectileCapacity})`,
      '================================================',
    ].join('\n'));
  }
}

function ratio(numerator: number, denominator: number): string {
  if (denominator <= 0) return '0.0%';
  return `${((numerator / denominator) * 100).toFixed(1)}%`;
}

function avatarCapacity(snapshot: NetcodeDiagnosticsSnapshot): number {
  return snapshot.remoteAvatarPoolActive + snapshot.remoteAvatarPoolIdle;
}

function projectileCapacity(snapshot: NetcodeDiagnosticsSnapshot): number {
  return snapshot.projectilePoolActive + snapshot.projectilePoolIdle;
}

function formatElapsed(totalTicks: number): string {
  const totalSeconds = Math.floor(totalTicks / NET_TICK_HZ);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${pad2(minutes)}:${pad2(seconds)}`;
}

function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}
