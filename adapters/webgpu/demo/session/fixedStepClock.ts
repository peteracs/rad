export interface FixedStepClockOptions {
  readonly stepSeconds: number;
  readonly maxFrameSeconds: number;
  readonly maxStepsPerFrame: number;
}

export interface FixedStepAdvance {
  readonly steps: number;
  readonly droppedSeconds: number;
}

/** Converts wall-clock frame time into a bounded deterministic step count. */
export class FixedStepClock {
  private accumulatorSeconds = 0;

  constructor(private readonly options: FixedStepClockOptions) {
    if (!Number.isFinite(options.stepSeconds) || options.stepSeconds <= 0) {
      throw new Error('demo.invalid_fixed_step');
    }
    if (!Number.isFinite(options.maxFrameSeconds) || options.maxFrameSeconds <= 0) {
      throw new Error('demo.invalid_max_frame_seconds');
    }
    if (!Number.isSafeInteger(options.maxStepsPerFrame) || options.maxStepsPerFrame <= 0) {
      throw new Error('demo.invalid_max_steps_per_frame');
    }
  }

  advance(elapsedSeconds: number): FixedStepAdvance {
    if (!Number.isFinite(elapsedSeconds) || elapsedSeconds < 0) {
      throw new Error('demo.invalid_elapsed_seconds');
    }
    const accepted = Math.min(elapsedSeconds, this.options.maxFrameSeconds);
    let droppedSeconds = elapsedSeconds - accepted;
    this.accumulatorSeconds += accepted;

    const available = Math.floor(this.accumulatorSeconds / this.options.stepSeconds);
    const steps = Math.min(available, this.options.maxStepsPerFrame);
    this.accumulatorSeconds -= steps * this.options.stepSeconds;
    if (available > steps) {
      const droppedSteps = available - steps;
      const droppedFromAccumulator = droppedSteps * this.options.stepSeconds;
      this.accumulatorSeconds -= droppedFromAccumulator;
      droppedSeconds += droppedFromAccumulator;
    }
    return Object.freeze({ steps, droppedSeconds });
  }
}
