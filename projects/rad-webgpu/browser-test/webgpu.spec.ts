import { expect, test } from '@playwright/test';
import { PNG } from 'pngjs';

test('renders pixels across resize, session restart, and device recovery', async ({ page }) => {
  await page.goto('/');
  const status = page.locator('#status');
  await expect(status).toHaveAttribute('data-kind', 'ok', { timeout: 30_000 });
  const canvas = page.locator('#viewport');
  await expectVisibleAvatarPixels(canvas);

  const initial = await snapshot(page);
  expect(initial.errors).toEqual([]);
  await canvas.evaluate((element) => { element.style.width = '520px'; });
  await expect.poll(async () => (await snapshot(page)).canvasWidth).not.toBe(initial.canvasWidth);
  await expectVisibleAvatarPixels(canvas);

  await page.evaluate(() => globalThis.__radWebGpuDogfood?.restart());
  await expect.poll(async () => BigInt((await snapshot(page)).streamId)).toBeGreaterThan(
    BigInt(initial.streamId),
  );
  await expect(status).toHaveAttribute('data-kind', 'ok');
  await expectVisibleAvatarPixels(canvas);
  expect((await snapshot(page)).errors).toEqual([]);

  const beforeLoss = await snapshot(page);
  await page.evaluate(() => globalThis.__radWebGpuDogfood?.loseDevice());
  await expect.poll(async () => (await snapshot(page)).deviceEpoch, { timeout: 30_000 }).toBeGreaterThan(
    beforeLoss.deviceEpoch,
  );
  await expect(status).toHaveAttribute('data-kind', 'ok');
  await expectVisibleAvatarPixels(canvas);
  const recovered = await snapshot(page);
  expect(recovered.errors.length).toBeGreaterThanOrEqual(1);
  expect(recovered.errors.every((error) => error.startsWith('webgpu.device_lost:'))).toBe(true);
});

async function expectVisibleAvatarPixels(canvas: import('@playwright/test').Locator): Promise<void> {
  const png = PNG.sync.read(await canvas.screenshot());
  const reference = pixelAt(png, 8, 8);
  let changed = 0;
  for (let y = 8; y < png.height - 8; y += 1) {
    for (let x = 8; x < png.width - 8; x += 1) {
      const pixel = pixelAt(png, x, y);
      const distance = Math.abs(pixel[0] - reference[0])
        + Math.abs(pixel[1] - reference[1])
        + Math.abs(pixel[2] - reference[2]);
      if (distance > 60) changed += 1;
    }
  }
  expect(changed).toBeGreaterThan(200);
}

function pixelAt(png: PNG, x: number, y: number): readonly [number, number, number] {
  const offset = (y * png.width + x) * 4;
  return [png.data[offset] ?? 0, png.data[offset + 1] ?? 0, png.data[offset + 2] ?? 0];
}

async function snapshot(page: import('@playwright/test').Page) {
  const value = await page.evaluate(() => globalThis.__radWebGpuDogfood?.snapshot());
  if (!value) throw new Error('RAD WebGPU dogfood harness is unavailable');
  return value;
}
