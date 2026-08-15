import type { AvatarRenderState } from './worldView';

export class AvatarInterpolator {
  private currentTick = 0;
  private hasSample = false;
  private readonly previous: AvatarRenderState;
  private readonly current: AvatarRenderState;

  constructor(
    previous: AvatarRenderState,
    current: AvatarRenderState,
  ) {
    this.previous = previous;
    this.current = current;
  }

  reset(): void {
    this.currentTick = 0;
    this.hasSample = false;
  }

  pushSample(tick: number, state: AvatarRenderState): void {
    const sampleTick = Math.trunc(tick);
    if (!this.hasSample) {
      copyAvatarState(this.previous, state);
      copyAvatarState(this.current, state);
      this.currentTick = sampleTick;
      this.hasSample = true;
      return;
    }

    if (sampleTick <= this.currentTick) {
      copyAvatarState(this.current, state);
      this.currentTick = sampleTick;
      return;
    }

    copyAvatarState(this.previous, this.current);
    copyAvatarState(this.current, state);
    this.currentTick = sampleTick;
  }

  writeVisualState(alpha: number, out: AvatarRenderState): boolean {
    if (!this.hasSample) return false;

    const t = Math.min(1, Math.max(0, alpha));
    out.model = this.current.model;
    out.x = lerp(this.previous.x, this.current.x, t);
    out.y = lerp(this.previous.y, this.current.y, t);
    out.targetX = this.current.targetX;
    out.targetY = this.current.targetY;
    out.targetActive = this.current.targetActive;
    out.commandId = this.current.commandId;
    return true;
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
