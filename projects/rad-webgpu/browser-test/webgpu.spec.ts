import { expect, test } from '@playwright/test';

test('renders pixels across resize, session restart, and device recovery', async ({ page }) => {
  await page.goto('/');
  const status = page.locator('#status');
  await expect(status).toHaveAttribute('data-kind', 'ok', { timeout: 30_000 });
  const canvas = page.locator('#viewport');
  const initialPixels = await capturePresentation(page);
  expect(initialPixels.recordCount).toBeGreaterThan(0);
  expect(initialPixels.changedPixels).toBeGreaterThan(200);
  const ready = await snapshot(page);
  expect(ready.errors).toEqual([]);

  const initial = ready;
  await canvas.evaluate((element) => { element.style.width = '520px'; });
  await expect.poll(async () => (await snapshot(page)).canvasWidth).not.toBe(initial.canvasWidth);
  const resizedPixels = await capturePresentation(page);
  expect(resizedPixels.width).not.toBe(initialPixels.width);
  expect(resizedPixels.changedPixels).toBeGreaterThan(200);

  await page.evaluate(() => globalThis.__radWebGpuDogfood?.restart());
  await expect.poll(async () => BigInt((await snapshot(page)).streamId)).toBeGreaterThan(
    BigInt(initial.streamId),
  );
  await expect(status).toHaveAttribute('data-kind', 'ok');
  expect((await capturePresentation(page)).changedPixels).toBeGreaterThan(200);
  expect((await snapshot(page)).errors).toEqual([]);

  const beforeLoss = await snapshot(page);
  await page.evaluate(() => globalThis.__radWebGpuDogfood?.loseDevice());
  await expect.poll(async () => (await snapshot(page)).deviceEpoch, { timeout: 30_000 }).toBeGreaterThan(
    beforeLoss.deviceEpoch,
  );
  await expect(status).toHaveAttribute('data-kind', 'ok');
  expect((await capturePresentation(page)).changedPixels).toBeGreaterThan(200);
  const recovered = await snapshot(page);
  expect(recovered.errors.length).toBeGreaterThanOrEqual(1);
  expect(recovered.errors.every((error) => error.startsWith('webgpu.device_lost:'))).toBe(true);
});

async function snapshot(page: import('@playwright/test').Page) {
  const value = await page.evaluate(() => globalThis.__radWebGpuDogfood?.snapshot());
  if (!value) throw new Error('RAD WebGPU dogfood harness is unavailable');
  return value;
}

async function capturePresentation(page: import('@playwright/test').Page) {
  try {
    const proof = await page.evaluate(() => globalThis.__radWebGpuDogfood?.capture());
    if (!proof) throw new Error('RAD WebGPU pixel proof is unavailable');
    return proof;
  } catch (error) {
    const state = await snapshot(page);
    throw new Error(`RAD WebGPU pixel readback failed: ${JSON.stringify(state)}`, { cause: error });
  }
}
