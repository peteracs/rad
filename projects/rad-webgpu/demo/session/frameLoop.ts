import { FixedStepClock } from './fixedStepClock.js';

export interface FrameLoopOptions {
  readonly stepSeconds?: number;
  readonly maxFrameSeconds?: number;
  readonly maxStepsPerFrame?: number;
  readonly onSimulationStep: (stepSeconds: number) => void;
  readonly onRender: () => void;
  readonly onTimeDropped?: (seconds: number) => void;
  readonly onError?: (error: unknown) => void;
}

/** Runs deterministic simulation steps independently from display cadence. */
export function startFrameLoop(options: FrameLoopOptions): () => void {
  const stepSeconds = options.stepSeconds ?? 1 / 60;
  const clock = new FixedStepClock({
    stepSeconds,
    maxFrameSeconds: options.maxFrameSeconds ?? 0.25,
    maxStepsPerFrame: options.maxStepsPerFrame ?? 8,
  });
  let previousTime = performance.now();
  let frameHandle = 0;
  let running = true;

  function stop(): void {
    if (!running) return;
    running = false;
    cancelAnimationFrame(frameHandle);
  }

  function frame(now: number): void {
    if (!running) return;
    try {
      const elapsedSeconds = Math.max(0, (now - previousTime) / 1_000);
      previousTime = now;
      const advance = clock.advance(elapsedSeconds);
      for (let index = 0; index < advance.steps; index += 1) {
        options.onSimulationStep(stepSeconds);
      }
      if (advance.droppedSeconds > 0) options.onTimeDropped?.(advance.droppedSeconds);
      options.onRender();
    } catch (error) {
      stop();
      options.onError?.(error);
      return;
    }
    if (running) frameHandle = requestAnimationFrame(frame);
  }

  frameHandle = requestAnimationFrame(frame);
  return stop;
}
