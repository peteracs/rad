import { expect, test } from '@playwright/test';
import { callHarness } from './harness.js';

test('renders pixels across resize, session restart, and device recovery', async ({ page }) => {
  await page.goto('/');
  const status = page.locator('#status');
  await expect(status).toHaveAttribute('data-kind', 'ok', { timeout: 30_000 });
  const canvas = page.locator('#viewport');
  const initialPixels = await capturePresentation(page);
  expect(initialPixels.recordCount).toBeGreaterThan(0);
  expect(initialPixels.changedPixels).toBeGreaterThan(200);
  const ready = await callHarness(page, 'snapshot');
  expect(ready.errors).toEqual([]);

  const initial = ready;
  await canvas.evaluate((element) => { element.style.width = '520px'; });
  await expect.poll(async () => (await callHarness(page, 'snapshot')).canvasWidth).not.toBe(initial.canvasWidth);
  const resizedPixels = await capturePresentation(page);
  expect(resizedPixels.width).not.toBe(initialPixels.width);
  expect(resizedPixels.changedPixels).toBeGreaterThan(200);

  await callHarness(page, 'restart');
  await expect.poll(async () => BigInt((await callHarness(page, 'snapshot')).streamId)).toBeGreaterThan(
    BigInt(initial.streamId),
  );
  await expect(status).toHaveAttribute('data-kind', 'ok');
  expect((await capturePresentation(page)).changedPixels).toBeGreaterThan(200);
  expect((await callHarness(page, 'snapshot')).errors).toEqual([]);

  const beforeLoss = await callHarness(page, 'snapshot');
  await callHarness(page, 'loseDevice');
  await expect.poll(async () => (await callHarness(page, 'snapshot')).deviceEpoch, { timeout: 30_000 }).toBeGreaterThan(
    beforeLoss.deviceEpoch,
  );
  await expect(status).toHaveAttribute('data-kind', 'ok');
  expect((await capturePresentation(page)).changedPixels).toBeGreaterThan(200);
  const recovered = await callHarness(page, 'snapshot');
  expect(recovered.errors.length).toBeGreaterThanOrEqual(1);
  expect(recovered.errors.every((error: string) => error.startsWith('webgpu.device_lost:'))).toBe(true);
});

async function capturePresentation(page: import('@playwright/test').Page) {
  try {
    return await callHarness(page, 'capture');
  } catch (error) {
    const state = await callHarness(page, 'snapshot');
    throw new Error(`RAD WebGPU pixel readback failed: ${JSON.stringify(state)}`, { cause: error });
  }
}
