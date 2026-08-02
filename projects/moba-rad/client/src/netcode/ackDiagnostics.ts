export interface AckDiagnosticsSnapshot {
  highestAck: number;
  inspectedPackets: number;
  missingPackets: number;
  lossRatio: number;
  recommendedInputDelayTicks: number;
}

export function createAckDiagnosticsSnapshot(): AckDiagnosticsSnapshot {
  return {
    highestAck: 0,
    inspectedPackets: 0,
    missingPackets: 0,
    lossRatio: 0,
    recommendedInputDelayTicks: 0,
  };
}

const ACK_WINDOW_BITS = 32;
const DEFAULT_TIMING_SAFETY_TICKS = 2;

export class AckDiagnostics {
  private highestAck = 0;
  private inspectedPackets = 0;
  private missingPackets = 0;
  private lossInputDelayTicks: number;
  private timingInputDelayTicks: number;
  private recommendedInputDelayTicks: number;

  constructor(
    private readonly baseInputDelayTicks: number,
    private readonly maxInputDelayTicks: number,
    private readonly timingSafetyTicks = DEFAULT_TIMING_SAFETY_TICKS,
  ) {
    this.lossInputDelayTicks = baseInputDelayTicks;
    this.timingInputDelayTicks = baseInputDelayTicks;
    this.recommendedInputDelayTicks = baseInputDelayTicks;
  }

  update(ackClientSeq: number, ackBits: number, out = createAckDiagnosticsSnapshot()): AckDiagnosticsSnapshot {
    const ack = Math.trunc(ackClientSeq);
    if (ack <= this.highestAck) return this.writeSnapshot(out);

    const bits = Math.trunc(ackBits) >>> 0;
    const window = Math.min(ACK_WINDOW_BITS, ack);

    let inspected = 0;
    let missing = 0;
    for (let offset = 0; offset < window; offset += 1) {
      const seq = ack - offset;
      if (seq <= this.highestAck) break;
      inspected += 1;
      if (((bits >>> offset) & 1) === 0) missing += 1;
    }

    this.inspectedPackets += inspected;
    this.missingPackets += missing;
    this.highestAck = ack;
    this.lossInputDelayTicks = this.delayForLoss(this.lossRatio());
    this.refreshRecommendedDelay();
    return this.writeSnapshot(out);
  }

  updateNetworkTiming(
    roundTripMs: number,
    jitterMs: number,
    tickHz: number,
    out = createAckDiagnosticsSnapshot(),
  ): AckDiagnosticsSnapshot {
    this.timingInputDelayTicks = this.delayForNetworkTiming(roundTripMs, jitterMs, tickHz);
    this.refreshRecommendedDelay();
    return this.writeSnapshot(out);
  }

  recommendedDelayTicks(): number {
    return this.recommendedInputDelayTicks;
  }

  highestAckSeq(): number {
    return this.highestAck;
  }

  writeSnapshot(out: AckDiagnosticsSnapshot): AckDiagnosticsSnapshot {
    out.highestAck = this.highestAck;
    out.inspectedPackets = this.inspectedPackets;
    out.missingPackets = this.missingPackets;
    out.lossRatio = this.lossRatio();
    out.recommendedInputDelayTicks = this.recommendedInputDelayTicks;
    return out;
  }

  snapshot(): AckDiagnosticsSnapshot {
    return this.writeSnapshot(createAckDiagnosticsSnapshot());
  }

  private lossRatio(): number {
    if (this.inspectedPackets <= 0) return 0;
    return this.missingPackets / this.inspectedPackets;
  }

  private delayForLoss(lossRatio: number): number {
    if (lossRatio >= 0.12) return this.maxInputDelayTicks;
    if (lossRatio >= 0.06) return Math.min(this.maxInputDelayTicks, this.baseInputDelayTicks + 2);
    if (lossRatio >= 0.02) return Math.min(this.maxInputDelayTicks, this.baseInputDelayTicks + 1);
    return this.baseInputDelayTicks;
  }

  private delayForNetworkTiming(roundTripMs: number, jitterMs: number, tickHz: number): number {
    if (!Number.isFinite(roundTripMs) || !Number.isFinite(jitterMs) || !Number.isFinite(tickHz) || tickHz <= 0) {
      return this.baseInputDelayTicks;
    }

    const tickMs = 1000 / tickHz;
    const oneWayTicks = Math.ceil(Math.max(0, roundTripMs) / (2 * tickMs));
    const jitterTicks = Math.ceil(Math.max(0, jitterMs) / tickMs);
    return Math.min(
      this.maxInputDelayTicks,
      Math.max(this.baseInputDelayTicks, oneWayTicks + jitterTicks + this.timingSafetyTicks),
    );
  }

  private refreshRecommendedDelay(): void {
    this.recommendedInputDelayTicks = Math.min(
      this.maxInputDelayTicks,
      Math.max(this.baseInputDelayTicks, this.lossInputDelayTicks, this.timingInputDelayTicks),
    );
  }
}
