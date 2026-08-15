import { MAX_CLIENT_CATCHUP_TICKS } from '../netcode/constants.js';
import { PredictedMoveApplier } from '../netcode/predictedMoveApplier.js';
import type { PredictionBuffer } from '../netcode/predictionBuffer';
import type { RadGameSession, RadWorld } from '../radHost';
import type { AvatarRenderState } from '../render/worldView';
import {
  createAvatarRenderState,
  writeControlledAvatarState,
} from '../render/worldView.js';
import type { MobaRadScene } from '../scene';
import type { ServerState } from '../transport/serverState';

export class ClientPredictionRunner {
  private readonly predictedMoveApplier: PredictedMoveApplier;
  private readonly avatarState = createAvatarRenderState();
  private worldState: RadWorld;
  private simulatedTick = 0;
  private localSimulationActive = false;
  private missingAvatarWarned = false;

  constructor(
    private readonly session: RadGameSession,
    private readonly playerId: number,
    private readonly scene: MobaRadScene,
    private readonly prediction: PredictionBuffer,
  ) {
    this.predictedMoveApplier = new PredictedMoveApplier(prediction);
    this.worldState = session.snapshot();
  }

  get world(): RadWorld {
    return this.worldState;
  }

  get active(): boolean {
    return this.localSimulationActive;
  }

  markActive(): void {
    this.localSimulationActive = true;
  }

  writeLocalAvatarState(out: AvatarRenderState): boolean {
    return writeControlledAvatarState(this.worldState, this.playerId, out);
  }

  advanceToTick(clockTick: number): void {
    if (this.simulatedTick >= clockTick) return;

    const gap = clockTick - this.simulatedTick;
    if (!this.localSimulationActive || gap > MAX_CLIENT_CATCHUP_TICKS) {
      // Idle (nothing to integrate) or a large hard-resync jump: advance the
      // frontier without churning the ring. The client stops polling the
      // authority while idle, so the unrecorded span is never reconciled
      // against; a large jump is corrected by the next snapshot reconcile.
      this.simulatedTick = clockTick;
      return;
    }

    while (this.simulatedTick < clockTick) {
      const tick = this.simulatedTick + 1;
      this.simulatedTick = tick;
      this.applyLocalInputForTick(tick);
      if (this.localSimulationActive) {
        this.session.tickFixed();
        this.recordWorldSampleForTick(tick);
      }
    }
  }

  refreshSceneSample(tick: number): void {
    this.recordWorldSampleForTick(tick);
  }

  applyAuthoritativeStateAndReplay(state: ServerState, startTick: number, endTick: number): void {
    this.session.applyAuthoritativeState(state);
    this.predictedMoveApplier.replayWindow(
      startTick,
      endTick,
      this.session,
      this.playerId,
    );
    this.simulatedTick = endTick;
    this.recordWorldSampleForTick(endTick);
  }

  private applyLocalInputForTick(tick: number): void {
    const applied = this.predictedMoveApplier.applyDueMovesAtOrBefore(
      tick,
      this.session,
      this.playerId,
    );
    if (applied > 0) {
      this.localSimulationActive = true;
    }
  }

  private recordWorldSampleForTick(tick: number): void {
    this.worldState = this.session.refresh();
    const hasLocalState = writeControlledAvatarState(this.worldState, this.playerId, this.avatarState);
    if (hasLocalState) {
      this.prediction.recordPosition(tick, this.avatarState.x, this.avatarState.y);
      this.localSimulationActive =
        this.avatarState.targetActive ||
        this.prediction.hasUnappliedMoveAtOrBefore(tick) ||
        this.prediction.hasPendingMoveAtOrAfter(tick + 1);
    } else {
      this.warnMissingControlledAvatar();
    }
    this.scene.applyAvatarState(hasLocalState ? this.avatarState : null, tick);
  }

  // The local champion is rendered only from the controlled avatar found here.
  // If it is missing (player_id mismatch between the client identity and the
  // seeded/authoritative world, or an empty render-buffer world) every sample is
  // null and the champion freezes at spawn even though authority keeps moving.
  // Surface it once with the player ids that ARE present so the mismatch is
  // diagnosable instead of silent.
  private warnMissingControlledAvatar(): void {
    if (this.missingAvatarWarned) return;
    this.missingAvatarWarned = true;
    const presentPlayerIds: number[] = [];
    for (let i = 0; i < this.worldState.entities.length; i += 1) {
      const components = this.worldState.entities[i].components;
      for (let c = 0; c < components.length; c += 1) {
        if (components[c].type !== 'PlayerControlled') continue;
        const id = Number(components[c].fields.player_id);
        if (Number.isFinite(id)) presentPlayerIds.push(id);
      }
    }
    // eslint-disable-next-line no-console
    console.error(
      `[moba-rad] controlled avatar for player_id=${this.playerId} not found in the ` +
        `RAD world (present player ids: [${presentPlayerIds.join(', ')}]). ` +
        'The local champion cannot be predicted/rendered until this matches.',
    );
  }
}
