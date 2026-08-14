import type { Page } from '@playwright/test';
import type { RadWebGpuBrowserHarness } from '../demo/testing/browserHarness.js';

type Harness = RadWebGpuBrowserHarness;
type HarnessMethod = keyof Harness;

export async function callHarness<K extends HarnessMethod>(
  page: Page,
  method: K,
): Promise<Awaited<ReturnType<Harness[K]>>> {
  return page.evaluate(async (methodName: HarnessMethod) => {
    const harness = (
      globalThis as typeof globalThis & {
        __radWebGpuDogfood?: Harness;
      }
    ).__radWebGpuDogfood;

    if (!harness) {
      throw new Error('RAD WebGPU browser harness is unavailable');
    }

    const operation = harness[methodName];

    if (typeof operation !== 'function') {
      throw new Error(
        `RAD WebGPU harness operation unavailable: ${String(methodName)}`,
      );
    }

    return await operation();
  }, method) as Promise<Awaited<ReturnType<Harness[K]>>>;
}
