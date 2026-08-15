import { PredictionBuffer, type PredictionInputSnapshot } from './predictionBuffer';

export interface PredictedMoveTarget {
  moveOrder(playerId: number, commandId: number, targetX: number, targetY: number): void;
  tickFixed(): void;
}

function createPredictionInputScratch(): PredictionInputSnapshot {
  return {
    kind: 0,
    tick: 0,
    clientSeq: 0,
    commandId: 0,
    targetX: 0,
    targetY: 0,
    dirX: 0,
    dirY: 0,
    fireViewTick: 0,
  };
}

export class PredictedMoveApplier {
  private readonly scratch = createPredictionInputScratch();

  constructor(private readonly prediction: PredictionBuffer) {}

  applyDueMovesAtOrBefore(
    tickValue: number,
    target: PredictedMoveTarget,
    playerId: number,
  ): number {
    if (!Number.isFinite(tickValue)) return 0;
    const tick = Math.trunc(tickValue);
    let applied = 0;
    while (this.prediction.writeNextUnappliedMoveAtOrBefore(tick, this.scratch)) {
      target.moveOrder(
        playerId,
        this.scratch.commandId,
        this.scratch.targetX,
        this.scratch.targetY,
      );
      applied += 1;
    }
    return applied;
  }

  replayWindow(
    startTickValue: number,
    endTickValue: number,
    target: PredictedMoveTarget,
    playerId: number,
  ): number {
    if (!Number.isFinite(startTickValue) || !Number.isFinite(endTickValue)) return 0;
    const startTick = Math.trunc(startTickValue);
    const endTick = Math.trunc(endTickValue);
    if (endTick < startTick) return 0;

    this.prediction.markMoveInputsForReplay(startTick, endTick);
    let applied = 0;
    for (let tick = startTick; tick <= endTick; tick += 1) {
      applied += this.applyDueMovesAtOrBefore(tick, target, playerId);
      target.tickFixed();
    }
    return applied;
  }
}
