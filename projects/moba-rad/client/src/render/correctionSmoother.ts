export interface CorrectionPoint {
  x: number;
  y: number;
}

export class CorrectionSmoother {
  private fromX = 0;
  private fromY = 0;
  private startMs = 0;
  private endMs = 0;
  private active = false;

  start(fromX: number, fromY: number, nowMs: number, durationMs: number): void {
    if (durationMs <= 0) {
      this.active = false;
      return;
    }

    this.fromX = fromX;
    this.fromY = fromY;
    this.startMs = nowMs;
    this.endMs = nowMs + durationMs;
    this.active = true;
  }

  write(targetX: number, targetY: number, nowMs: number, out: CorrectionPoint): boolean {
    if (!this.active) return false;

    if (nowMs >= this.endMs) {
      this.active = false;
      return false;
    }

    const t = smoothstep(clamp01((nowMs - this.startMs) / (this.endMs - this.startMs)));
    out.x = lerp(this.fromX, targetX, t);
    out.y = lerp(this.fromY, targetY, t);
    return true;
  }
}

function clamp01(value: number): number {
  if (value <= 0) return 0;
  if (value >= 1) return 1;
  return value;
}

function smoothstep(t: number): number {
  return t * t * (3 - 2 * t);
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
