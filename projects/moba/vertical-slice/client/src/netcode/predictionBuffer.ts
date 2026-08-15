import { PREDICTION_RING_SIZE } from './constants.js';

const RING_MASK = PREDICTION_RING_SIZE - 1;
const ACK_WINDOW_BITS = 32;

export const PREDICTION_INPUT_KIND_MOVE = 1;
export const PREDICTION_INPUT_KIND_CAST = 2;

export interface PredictionInputSnapshot {
  kind: number;
  tick: number;
  clientSeq: number;
  commandId: number;
  targetX: number;
  targetY: number;
  dirX: number;
  dirY: number;
  fireViewTick: number;
}

export class PredictionBuffer {
  private readonly inputActive = new Uint8Array(PREDICTION_RING_SIZE);
  private readonly inputKinds = new Uint8Array(PREDICTION_RING_SIZE);
  private readonly inputAppliedLocally = new Uint8Array(PREDICTION_RING_SIZE);
  private readonly inputTicks = new Int32Array(PREDICTION_RING_SIZE);
  private readonly inputClientSeqs = new Int32Array(PREDICTION_RING_SIZE);
  private readonly inputCommandIds = new Int32Array(PREDICTION_RING_SIZE);
  private readonly inputTargetXs = new Float64Array(PREDICTION_RING_SIZE);
  private readonly inputTargetYs = new Float64Array(PREDICTION_RING_SIZE);
  private readonly inputDirXs = new Float64Array(PREDICTION_RING_SIZE);
  private readonly inputDirYs = new Float64Array(PREDICTION_RING_SIZE);
  private readonly inputFireViewTicks = new Int32Array(PREDICTION_RING_SIZE);

  private readonly positionActive = new Uint8Array(PREDICTION_RING_SIZE);
  private readonly positionTicks = new Int32Array(PREDICTION_RING_SIZE);
  private readonly positionXs = new Float64Array(PREDICTION_RING_SIZE);
  private readonly positionYs = new Float64Array(PREDICTION_RING_SIZE);

  recordMoveInput(
    targetTick: number,
    clientSeq: number,
    commandId: number,
    targetX: number,
    targetY: number,
  ): void {
    const tick = Math.trunc(targetTick);
    const slot = tick & RING_MASK;
    this.inputActive[slot] = 1;
    this.inputKinds[slot] = PREDICTION_INPUT_KIND_MOVE;
    this.inputAppliedLocally[slot] = 0;
    this.inputTicks[slot] = tick;
    this.inputClientSeqs[slot] = Math.trunc(clientSeq);
    this.inputCommandIds[slot] = Math.trunc(commandId);
    this.inputTargetXs[slot] = targetX;
    this.inputTargetYs[slot] = targetY;
    this.inputDirXs[slot] = 0;
    this.inputDirYs[slot] = 0;
    this.inputFireViewTicks[slot] = 0;
  }

  recordCastInput(
    targetTick: number,
    clientSeq: number,
    commandId: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
  ): void {
    const tick = Math.trunc(targetTick);
    const slot = tick & RING_MASK;
    this.inputActive[slot] = 1;
    this.inputKinds[slot] = PREDICTION_INPUT_KIND_CAST;
    this.inputAppliedLocally[slot] = 1;
    this.inputTicks[slot] = tick;
    this.inputClientSeqs[slot] = Math.trunc(clientSeq);
    this.inputCommandIds[slot] = Math.trunc(commandId);
    this.inputTargetXs[slot] = 0;
    this.inputTargetYs[slot] = 0;
    this.inputDirXs[slot] = dirX;
    this.inputDirYs[slot] = dirY;
    this.inputFireViewTicks[slot] = Math.trunc(fireViewTick);
  }

  hasInputAt(tick: number): boolean {
    const t = Math.trunc(tick);
    const slot = t & RING_MASK;
    return this.inputActive[slot] === 1 && this.inputTicks[slot] === t;
  }

  hasMoveInputAt(tick: number): boolean {
    const t = Math.trunc(tick);
    const slot = t & RING_MASK;
    return this.inputActive[slot] === 1
      && this.inputKinds[slot] === PREDICTION_INPUT_KIND_MOVE
      && this.inputTicks[slot] === t;
  }

  inputKindAt(tick: number): number {
    return this.inputKinds[Math.trunc(tick) & RING_MASK];
  }

  inputCommandIdAt(tick: number): number {
    return this.inputCommandIds[Math.trunc(tick) & RING_MASK];
  }

  inputTargetXAt(tick: number): number {
    return this.inputTargetXs[Math.trunc(tick) & RING_MASK];
  }

  inputTargetYAt(tick: number): number {
    return this.inputTargetYs[Math.trunc(tick) & RING_MASK];
  }

  inputDirXAt(tick: number): number {
    return this.inputDirXs[Math.trunc(tick) & RING_MASK];
  }

  inputDirYAt(tick: number): number {
    return this.inputDirYs[Math.trunc(tick) & RING_MASK];
  }

  inputFireViewTickAt(tick: number): number {
    return this.inputFireViewTicks[Math.trunc(tick) & RING_MASK];
  }

  hasPendingMoveAtOrAfter(tick: number): boolean {
    const minTick = Math.trunc(tick);
    for (let i = 0; i < PREDICTION_RING_SIZE; i += 1) {
      if (
        this.inputActive[i] === 1
        && this.inputKinds[i] === PREDICTION_INPUT_KIND_MOVE
        && this.inputTicks[i] >= minTick
      ) {
        return true;
      }
    }
    return false;
  }

  hasUnappliedMoveAtOrBefore(tick: number): boolean {
    const maxTick = Math.trunc(tick);
    for (let i = 0; i < PREDICTION_RING_SIZE; i += 1) {
      if (
        this.inputActive[i] === 1
        && this.inputAppliedLocally[i] === 0
        && this.inputKinds[i] === PREDICTION_INPUT_KIND_MOVE
        && this.inputTicks[i] <= maxTick
      ) {
        return true;
      }
    }
    return false;
  }

  writeNextUnappliedMoveAtOrBefore(tick: number, out: PredictionInputSnapshot): boolean {
    const maxTick = Math.trunc(tick);
    let bestSlot = -1;
    let bestTick = 2147483647;
    let bestSeq = 2147483647;

    for (let i = 0; i < PREDICTION_RING_SIZE; i += 1) {
      if (this.inputActive[i] !== 1) continue;
      if (this.inputAppliedLocally[i] !== 0) continue;
      if (this.inputKinds[i] !== PREDICTION_INPUT_KIND_MOVE) continue;
      const inputTick = this.inputTicks[i];
      if (inputTick > maxTick) continue;
      const inputSeq = this.inputClientSeqs[i];
      if (inputTick > bestTick) continue;
      if (inputTick === bestTick && inputSeq >= bestSeq) continue;
      bestTick = inputTick;
      bestSeq = inputSeq;
      bestSlot = i;
    }

    if (bestSlot < 0) return false;
    this.writeInputSnapshot(bestSlot, out);
    this.inputAppliedLocally[bestSlot] = 1;
    return true;
  }

  markMoveInputsForReplay(startTick: number, endTick: number): void {
    const start = Math.trunc(startTick);
    const end = Math.trunc(endTick);
    if (end < start) return;

    for (let i = 0; i < PREDICTION_RING_SIZE; i += 1) {
      if (this.inputActive[i] !== 1) continue;
      if (this.inputKinds[i] !== PREDICTION_INPUT_KIND_MOVE) continue;
      const inputTick = this.inputTicks[i];
      if (inputTick >= start && inputTick <= end) {
        this.inputAppliedLocally[i] = 0;
      }
    }
  }

  clearAppliedInputs(appliedClientSeq: number, appliedAckBits: number): void {
    const applied = Math.trunc(appliedClientSeq);
    const bits = Math.trunc(appliedAckBits) >>> 0;
    if (applied <= 0) return;
    for (let i = 0; i < PREDICTION_RING_SIZE; i += 1) {
      if (this.inputActive[i] !== 1) continue;
      const seq = this.inputClientSeqs[i];
      if (isAckedByWindow(seq, applied, bits) || applied - seq >= ACK_WINDOW_BITS) {
        this.inputActive[i] = 0;
        this.inputAppliedLocally[i] = 0;
      }
    }
  }

  writeOldestUnackedInputAfter(
    ackClientSeq: number,
    ackBits: number,
    minTargetTick: number,
    out: PredictionInputSnapshot,
  ): boolean {
    const ack = Math.trunc(ackClientSeq);
    const bits = Math.trunc(ackBits) >>> 0;
    const minTick = Math.trunc(minTargetTick);
    let bestSlot = -1;
    let bestSeq = 2147483647;

    for (let i = 0; i < PREDICTION_RING_SIZE; i += 1) {
      if (this.inputActive[i] !== 1) continue;
      const seq = this.inputClientSeqs[i];
      if (isAckedByWindow(seq, ack, bits) || seq >= bestSeq) continue;
      if (this.inputTicks[i] < minTick) continue;
      bestSeq = seq;
      bestSlot = i;
    }

    if (bestSlot < 0) return false;
    this.writeInputSnapshot(bestSlot, out);
    return true;
  }

  recordPosition(tick: number, x: number, y: number): void {
    const t = Math.trunc(tick);
    const slot = t & RING_MASK;
    this.positionActive[slot] = 1;
    this.positionTicks[slot] = t;
    this.positionXs[slot] = x;
    this.positionYs[slot] = y;
  }

  hasPositionAt(tick: number): boolean {
    const t = Math.trunc(tick);
    const slot = t & RING_MASK;
    return this.positionActive[slot] === 1 && this.positionTicks[slot] === t;
  }

  positionErrorSq(tick: number, x: number, y: number): number {
    const slot = Math.trunc(tick) & RING_MASK;
    const dx = this.positionXs[slot] - x;
    const dy = this.positionYs[slot] - y;
    return dx * dx + dy * dy;
  }

  private writeInputSnapshot(slot: number, out: PredictionInputSnapshot): void {
    out.tick = this.inputTicks[slot];
    out.clientSeq = this.inputClientSeqs[slot];
    out.commandId = this.inputCommandIds[slot];
    out.targetX = this.inputTargetXs[slot];
    out.targetY = this.inputTargetYs[slot];
    out.kind = this.inputKinds[slot];
    out.dirX = this.inputDirXs[slot];
    out.dirY = this.inputDirYs[slot];
    out.fireViewTick = this.inputFireViewTicks[slot];
  }
}

function isAckedByWindow(seq: number, ack: number, ackBits: number): boolean {
  if (seq <= 0 || seq > ack) return false;
  const offset = ack - seq;
  if (offset < 0 || offset >= ACK_WINDOW_BITS) return false;
  return ((ackBits >>> offset) & 1) === 1;
}
