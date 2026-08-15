import type { AckDiagnostics, AckDiagnosticsSnapshot } from '../netcode/ackDiagnostics';
import type { NetcodeDiagnosticsSnapshot } from '../netcode/runtimeDiagnostics';
import type { ServerState } from '../transport/serverState';
import type { AuthoritySnapshotVisualStats } from './authoritySnapshotProjector';

export class ClientNetcodeTelemetry {
  private roundTripMs = 0;
  private jitterMs = 0;
  private lastRoundTripMs = 0;
  private correctionCount = 0;
  private maxCorrectionDistance = 0;
  private correctionRateSampleCount = 0;
  private correctionRateSampleMs = 0;
  private reconciliationRatePerSecond = 0;
  private smoothedCorrectionCount = 0;
  private inputResendPackets = 0;
  private transportFailures = 0;
  private lastTransportError = 'none';
  private authorityTimeouts = 0;
  private authorityStateRequests = 0;
  private peerCount = 0;
  private peerRecordCount = 0;
  private maxPeers = 0;
  private inputQueueSlots = 0;
  private pendingMoveInputs = 0;
  private pendingCastInputs = 0;
  private peerConnected = false;
  private lateInputs = 0;
  private futureInputs = 0;
  private duplicateInputs = 0;
  private overwrittenInputs = 0;
  private lastAuthorityClientSeq = 0;
  private lastAuthorityAppliedSeq = 0;
  private lastAuthorityAppliedAckBits = 0;
  private avatarRecordCount = 0;
  private remoteAvatarCount = 0;
  private projectileRecordCount = 0;
  private projectileImpactRecordCount = 0;

  noteInputResendPacket(): void {
    this.inputResendPackets += 1;
  }

  noteAuthorityStateRequest(): void {
    this.authorityStateRequests += 1;
  }

  noteTransportFailure(error: unknown): void {
    this.transportFailures += 1;
    this.lastTransportError = error instanceof Error ? error.message : String(error);
    if (this.lastTransportError.includes('Timed out')) {
      this.authorityTimeouts += 1;
    }
  }

  noteAuthorityRoundTrip(
    startedMs: number,
    finishedMs: number,
    ackDiagnostics: AckDiagnostics,
    tickHz: number,
    ackOut: AckDiagnosticsSnapshot,
  ): void {
    const sample = Math.max(0, finishedMs - startedMs);
    if (this.roundTripMs <= 0) {
      this.roundTripMs = sample;
      this.lastRoundTripMs = sample;
      ackDiagnostics.updateNetworkTiming(this.roundTripMs, this.jitterMs, tickHz, ackOut);
      return;
    }

    const delta = Math.abs(sample - this.lastRoundTripMs);
    this.roundTripMs = this.roundTripMs * 0.85 + sample * 0.15;
    this.jitterMs = this.jitterMs * 0.85 + delta * 0.15;
    this.lastRoundTripMs = sample;
    ackDiagnostics.updateNetworkTiming(this.roundTripMs, this.jitterMs, tickHz, ackOut);
  }

  noteCorrection(correctionDistance: number, smoothed: boolean): void {
    this.correctionCount += 1;
    if (!smoothed) return;
    this.maxCorrectionDistance = Math.max(this.maxCorrectionDistance, correctionDistance);
    this.smoothedCorrectionCount += 1;
  }

  applyAuthoritySnapshot(state: ServerState, visualStats: AuthoritySnapshotVisualStats): void {
    this.peerCount = state.authority.peer_count;
    this.peerRecordCount = state.peers.length;
    this.maxPeers = state.authority.max_peers;
    this.inputQueueSlots = state.authority.input_queue_slots;
    this.pendingMoveInputs = state.authority.pending_move_inputs;
    this.pendingCastInputs = state.authority.pending_cast_inputs;
    this.peerConnected = state.authority.peer_connected;
    this.lateInputs = state.authority.late_inputs;
    this.futureInputs = state.authority.future_inputs;
    this.duplicateInputs = state.authority.duplicate_inputs;
    this.overwrittenInputs = state.authority.overwritten_inputs;
    this.lastAuthorityClientSeq = state.authority.last_client_seq;
    this.lastAuthorityAppliedSeq = state.authority.last_applied_client_seq;
    this.lastAuthorityAppliedAckBits = Math.trunc(state.authority.applied_ack_bits) >>> 0;
    this.avatarRecordCount = visualStats.avatarRecordCount;
    this.remoteAvatarCount = visualStats.remoteAvatarCount;
    this.projectileRecordCount = visualStats.projectileRecordCount;
    this.projectileImpactRecordCount = visualStats.projectileImpactRecordCount;
  }

  writeSnapshot<T extends NetcodeDiagnosticsSnapshot>(out: T, nowMs: number): T {
    this.updateReconciliationRate(nowMs);
    out.roundTripMs = this.roundTripMs;
    out.jitterMs = this.jitterMs;
    out.correctionCount = this.correctionCount;
    out.maxCorrectionDistance = this.maxCorrectionDistance;
    out.smoothedCorrectionCount = this.smoothedCorrectionCount;
    out.reconciliationRatePerSecond = this.reconciliationRatePerSecond;
    out.inputResendPackets = this.inputResendPackets;
    out.transportFailures = this.transportFailures;
    out.lastTransportError = this.lastTransportError;
    out.authorityTimeouts = this.authorityTimeouts;
    out.authorityStateRequests = this.authorityStateRequests;
    out.peerCount = this.peerCount;
    out.peerRecordCount = this.peerRecordCount;
    out.maxPeers = this.maxPeers;
    out.inputQueueSlots = this.inputQueueSlots;
    out.pendingMoveInputs = this.pendingMoveInputs;
    out.pendingCastInputs = this.pendingCastInputs;
    out.peerConnected = this.peerConnected;
    out.lateInputs = this.lateInputs;
    out.futureInputs = this.futureInputs;
    out.duplicateInputs = this.duplicateInputs;
    out.overwrittenInputs = this.overwrittenInputs;
    out.lastAuthorityClientSeq = this.lastAuthorityClientSeq;
    out.lastAuthorityAppliedSeq = this.lastAuthorityAppliedSeq;
    out.lastAuthorityAppliedAckBits = this.lastAuthorityAppliedAckBits;
    out.avatarRecordCount = this.avatarRecordCount;
    out.remoteAvatarCount = this.remoteAvatarCount;
    out.projectileRecordCount = this.projectileRecordCount;
    out.projectileImpactRecordCount = this.projectileImpactRecordCount;
    return out;
  }

  private updateReconciliationRate(nowMs: number): void {
    if (this.correctionRateSampleMs <= 0) {
      this.correctionRateSampleMs = nowMs;
      this.correctionRateSampleCount = this.correctionCount;
      return;
    }

    const elapsed = nowMs - this.correctionRateSampleMs;
    if (elapsed < 1000) return;

    this.reconciliationRatePerSecond =
      ((this.correctionCount - this.correctionRateSampleCount) * 1000) / elapsed;
    this.correctionRateSampleMs = nowMs;
    this.correctionRateSampleCount = this.correctionCount;
  }
}
