import type { AvatarRenderState } from './worldView';

export class AvatarTimeline {
  private readonly ticks: Int32Array;
  private sampleCount = 0;
  private writeCursor = 0;
  private latestTick = 0;

  constructor(private readonly samples: AvatarRenderState[]) {
    if (samples.length < 2) {
      throw new Error('AvatarTimeline requires at least two sample slots');
    }
    this.ticks = new Int32Array(samples.length);
  }

  reset(): void {
    this.sampleCount = 0;
    this.writeCursor = 0;
    this.latestTick = 0;
  }

  pushSample(tick: number, state: AvatarRenderState): void {
    const sampleTick = Math.trunc(tick);
    for (let i = 0; i < this.sampleCount; i += 1) {
      if (this.ticks[i] === sampleTick) {
        copyAvatarState(this.samples[i], state);
        if (sampleTick > this.latestTick) this.latestTick = sampleTick;
        return;
      }
    }

    const slot = this.writeCursor;
    this.ticks[slot] = sampleTick;
    copyAvatarState(this.samples[slot], state);
    this.writeCursor = (this.writeCursor + 1) % this.samples.length;
    if (this.sampleCount < this.samples.length) this.sampleCount += 1;
    if (this.sampleCount === 1 || sampleTick > this.latestTick) this.latestTick = sampleTick;
  }

  writeVisualStateAt(renderTick: number, out: AvatarRenderState): boolean {
    if (this.sampleCount <= 0) return false;

    let lowerIndex = -1;
    let upperIndex = -1;
    let lowerTick = -2147483648;
    let upperTick = 2147483647;

    for (let i = 0; i < this.sampleCount; i += 1) {
      const tick = this.ticks[i];
      if (tick <= renderTick && tick >= lowerTick) {
        lowerTick = tick;
        lowerIndex = i;
      }
      if (tick >= renderTick && tick <= upperTick) {
        upperTick = tick;
        upperIndex = i;
      }
    }

    if (lowerIndex < 0 && upperIndex < 0) return false;
    if (lowerIndex < 0) {
      copyAvatarState(out, this.samples[upperIndex]);
      return true;
    }
    if (upperIndex < 0 || lowerIndex === upperIndex || upperTick === lowerTick) {
      copyAvatarState(out, this.samples[lowerIndex]);
      return true;
    }

    const previous = this.samples[lowerIndex];
    const current = this.samples[upperIndex];
    const alpha = Math.min(1, Math.max(0, (renderTick - lowerTick) / (upperTick - lowerTick)));
    out.model = current.model;
    out.x = lerp(previous.x, current.x, alpha);
    out.y = lerp(previous.y, current.y, alpha);
    out.targetX = current.targetX;
    out.targetY = current.targetY;
    out.targetActive = current.targetActive;
    out.commandId = current.commandId;
    return true;
  }

  latestSampleTick(): number {
    return this.latestTick;
  }
}

function copyAvatarState(out: AvatarRenderState, state: AvatarRenderState): void {
  out.model = state.model;
  out.x = state.x;
  out.y = state.y;
  out.targetX = state.targetX;
  out.targetY = state.targetY;
  out.targetActive = state.targetActive;
  out.commandId = state.commandId;
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
