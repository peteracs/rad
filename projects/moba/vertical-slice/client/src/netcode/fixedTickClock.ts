import { FIXED_DT, MAX_CLIENT_CATCHUP_TICKS } from './constants.js';

export class FixedTickClock {
  private accumulator = 0;
  private lastNow = 0;
  private currentTick = 0;

  get tick(): number {
    return this.currentTick;
  }

  get interpolationAlpha(): number {
    return Math.min(1, Math.max(0, this.accumulator / FIXED_DT));
  }

  reset(now: number): void {
    this.lastNow = now;
    this.accumulator = 0;
  }

  setTick(tick: number): void {
    const nextTick = Math.trunc(tick);
    if (nextTick <= this.currentTick) return;
    this.currentTick = nextTick;
    this.accumulator = 0;
  }

  consume(now: number): number {
    if (this.lastNow <= 0) {
      this.reset(now);
      return 0;
    }

    const frameDt = Math.min(0.25, Math.max(0, (now - this.lastNow) / 1000));
    this.lastNow = now;
    this.accumulator += frameDt;

    let ticks = 0;
    while (this.accumulator >= FIXED_DT && ticks < MAX_CLIENT_CATCHUP_TICKS) {
      this.accumulator -= FIXED_DT;
      ticks += 1;
    }

    if (ticks === MAX_CLIENT_CATCHUP_TICKS && this.accumulator >= FIXED_DT) {
      this.accumulator = 0;
    }

    return ticks;
  }

  advanceOne(): number {
    this.currentTick += 1;
    return this.currentTick;
  }
}
