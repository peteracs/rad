import type { DemoSession } from '../session/demoSession.js';

export type RadWebGpuBrowserHarness = Pick<
  DemoSession,
  'capture' | 'loseDevice' | 'restart' | 'snapshot'
>;

export function installBrowserHarness(
  session: DemoSession,
): () => void {
  const harness: RadWebGpuBrowserHarness = Object.freeze({
    capture: () => session.capture(),
    loseDevice: () => session.loseDevice(),
    restart: () => session.restart(),
    snapshot: () => session.snapshot(),
  });

  globalThis.__radWebGpuDogfood = harness;

  return () => {
    if (globalThis.__radWebGpuDogfood === harness) {
      globalThis.__radWebGpuDogfood = undefined;
    }
  };
}
