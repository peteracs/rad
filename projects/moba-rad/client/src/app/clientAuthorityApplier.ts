import {
  createAckDiagnosticsSnapshot,
  type AckDiagnostics,
} from '../netcode/ackDiagnostics.js';
import {
  AuthorityStateGate,
  type AuthorityStateGateSnapshot,
} from '../netcode/authorityStateGate.js';
import { PREDICTION_LEAD_TICKS } from '../netcode/constants.js';
import type { FixedTickClock } from '../netcode/fixedTickClock';
import type { ClientInputSequencer } from '../netcode/inputSequencer';
import type { PredictionBuffer } from '../netcode/predictionBuffer';
import {
  createReconciliationDecision,
  ReconciliationPolicy,
} from '../netcode/reconciliationPolicy.js';
import { createAvatarRenderState } from '../render/worldView.js';
import type { MobaRadScene } from '../scene';
import type { ServerState } from '../transport/serverState';
import {
  AuthoritySnapshotProjector,
  createAuthoritySnapshotVisualStats,
} from './authoritySnapshotProjector.js';
import type { ClientNetcodeTelemetry } from './clientNetcodeTelemetry';
import type { ClientPredictionRunner } from './clientPredictionRunner';

export class ClientAuthorityApplier {
  private readonly gate: AuthorityStateGate;
  private readonly projector: AuthoritySnapshotProjector;
  private readonly visualStats = createAuthoritySnapshotVisualStats();
  private readonly reconciliationPolicy = new ReconciliationPolicy();
  private readonly reconciliationDecision = createReconciliationDecision();
  private readonly ackUpdateScratch = createAckDiagnosticsSnapshot();
  private readonly localStateScratch = createAvatarRenderState();
  private authoritySynced = false;
  private serverTick = 0;
  private moving = false;

  constructor(
    sessionId: number,
    playerId: number,
    private readonly scene: MobaRadScene,
    private readonly clock: FixedTickClock,
    private readonly prediction: PredictionBuffer,
    private readonly inputSequencer: ClientInputSequencer,
    private readonly ackDiagnostics: AckDiagnostics,
    private readonly predictionRunner: ClientPredictionRunner,
    private readonly telemetry: ClientNetcodeTelemetry,
  ) {
    this.gate = new AuthorityStateGate(sessionId, playerId);
    this.projector = new AuthoritySnapshotProjector(scene, playerId);
  }

  get synced(): boolean {
    return this.authoritySynced;
  }

  get serverTickEstimate(): number {
    return this.serverTick;
  }

  get authorityMayBeMoving(): boolean {
    return this.moving;
  }

  get lastReceiptAckBits(): number {
    return this.gate.lastReceiptAckBits;
  }

  markAuthorityMayBeMoving(): void {
    this.moving = true;
  }

  clearAuthorityMayBeMovingIfIdle(localSimulationActive: boolean): void {
    if (!localSimulationActive) this.moving = false;
  }

  writeGateSnapshot<T extends AuthorityStateGateSnapshot>(out: T): T {
    return this.gate.writeSnapshot(out);
  }

  apply(state: ServerState): number {
    if (!this.gate.accept(state, this.ackDiagnostics.highestAckSeq())) return 0;
    this.ackDiagnostics.update(state.ack_client_seq, state.ack_bits, this.ackUpdateScratch);

    const acceptedServerTick = Math.trunc(state.server_tick);
    this.authoritySynced = true;
    this.serverTick = Math.max(this.serverTick, acceptedServerTick);
    const visualStats = this.projector.apply(
      state,
      acceptedServerTick,
      this.visualStats,
    );
    this.telemetry.applyAuthoritySnapshot(state, visualStats);
    this.clock.setTick(acceptedServerTick + PREDICTION_LEAD_TICKS);
    this.prediction.clearAppliedInputs(
      state.authority.last_applied_client_seq,
      state.authority.applied_ack_bits,
    );

    const hasLocal = this.predictionRunner.writeLocalAvatarState(this.localStateScratch);
    const hasPrediction = this.prediction.hasPositionAt(acceptedServerTick);
    const positionErrorSq = hasPrediction
      ? this.prediction.positionErrorSq(acceptedServerTick, state.avatar.x, state.avatar.y)
      : Number.POSITIVE_INFINITY;
    const reconciliation = this.reconciliationPolicy.decide(
      this.inputSequencer.currentCommandId,
      hasLocal,
      this.localStateScratch.commandId,
      this.localStateScratch.targetActive,
      state.avatar.command_id,
      state.avatar.target_active,
      hasPrediction,
      positionErrorSq,
      this.reconciliationDecision,
    );

    this.moving = state.avatar.target_active;
    if (reconciliation.ignoreOlderCommand || !reconciliation.shouldReconcile) return 0;

    const correctionNow = performance.now();
    if (reconciliation.smoothCorrection) {
      const correctionDistance = reconciliation.correctionDistance;
      this.telemetry.noteCorrection(correctionDistance, true);
      this.scene.beginLocalCorrectionBlend(correctionNow);
      if (reconciliation.hardCorrection) {
        window.dispatchEvent(new CustomEvent('moba-rad-hard-correction', {
          detail: { distance: correctionDistance },
        }));
      }
    } else {
      this.telemetry.noteCorrection(0, false);
    }
    this.predictionRunner.applyAuthoritativeStateAndReplay(
      state,
      acceptedServerTick + 1,
      this.clock.tick,
    );
    return correctionNow;
  }
}
