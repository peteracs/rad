import {
  HARD_CORRECTION_DISTANCE_SQ,
  RECONCILE_ERROR_EPSILON_SQ,
} from './constants.js';

export interface ReconciliationDecision {
  ignoreOlderCommand: boolean;
  shouldReconcile: boolean;
  targetMismatch: boolean;
  positionMismatch: boolean;
  smoothCorrection: boolean;
  hardCorrection: boolean;
  positionErrorSq: number;
  correctionDistance: number;
}

export function createReconciliationDecision(): ReconciliationDecision {
  return {
    ignoreOlderCommand: false,
    shouldReconcile: false,
    targetMismatch: false,
    positionMismatch: false,
    smoothCorrection: false,
    hardCorrection: false,
    positionErrorSq: Number.POSITIVE_INFINITY,
    correctionDistance: 0,
  };
}

export class ReconciliationPolicy {
  constructor(
    private readonly reconcileErrorEpsilonSq = RECONCILE_ERROR_EPSILON_SQ,
    private readonly hardCorrectionDistanceSq = HARD_CORRECTION_DISTANCE_SQ,
  ) {}

  decide(
    currentCommandIdValue: number,
    hasLocal: boolean,
    localCommandIdValue: number,
    localTargetActive: boolean,
    authorityCommandIdValue: number,
    authorityTargetActive: boolean,
    hasPrediction: boolean,
    positionErrorSqValue: number,
    out: ReconciliationDecision,
  ): ReconciliationDecision {
    const currentCommandId = Math.trunc(currentCommandIdValue);
    const localCommandId = Math.trunc(localCommandIdValue);
    const authorityCommandId = Math.trunc(authorityCommandIdValue);
    const usablePrediction = hasPrediction && Number.isFinite(positionErrorSqValue);
    const positionErrorSq = usablePrediction
      ? Math.max(0, positionErrorSqValue)
      : Number.POSITIVE_INFINITY;

    out.ignoreOlderCommand =
      hasLocal &&
      localTargetActive &&
      authorityCommandId < currentCommandId;
    out.targetMismatch =
      hasLocal &&
      authorityCommandId === localCommandId &&
      authorityTargetActive !== localTargetActive;
    out.positionMismatch =
      !usablePrediction ||
      positionErrorSq > this.reconcileErrorEpsilonSq;
    out.shouldReconcile =
      !out.ignoreOlderCommand &&
      (out.positionMismatch || out.targetMismatch);
    out.smoothCorrection =
      out.shouldReconcile &&
      hasLocal &&
      usablePrediction &&
      positionErrorSq > this.reconcileErrorEpsilonSq;
    out.hardCorrection =
      out.smoothCorrection &&
      positionErrorSq > this.hardCorrectionDistanceSq;
    out.positionErrorSq = positionErrorSq;
    out.correctionDistance = out.smoothCorrection ? Math.sqrt(positionErrorSq) : 0;
    return out;
  }
}
