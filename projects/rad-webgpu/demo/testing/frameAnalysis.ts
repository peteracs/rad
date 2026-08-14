import type { AvatarFrameReadback } from '../../src/presentation/avatarRenderer.js';

const CHANGED_PIXEL_THRESHOLD = 60;

export function countChangedPixels(
  readback: AvatarFrameReadback,
): number {
  const { pixels, width, height, bytesPerRow } = readback;

  const clear = expectedClearBytes(readback);

  let changedPixels = 0;

  for (let y = 0; y < height; y += 1) {
    const rowOffset = y * bytesPerRow;

    for (let x = 0; x < width; x += 1) {
      const offset = rowOffset + x * 4;

      const distance =
        Math.abs((pixels[offset] ?? 0) - clear[0])
        + Math.abs((pixels[offset + 1] ?? 0) - clear[1])
        + Math.abs((pixels[offset + 2] ?? 0) - clear[2]);

      if (distance > CHANGED_PIXEL_THRESHOLD) {
        changedPixels += 1;
      }
    }
  }

  return changedPixels;
}

function expectedClearBytes(
  readback: AvatarFrameReadback,
): readonly [number, number, number] {
  const encode = readback.format.endsWith('-srgb') ? linearToSrgb : identity;
  const rgba = [
    byte(encode(readback.clearColor.r)),
    byte(encode(readback.clearColor.g)),
    byte(encode(readback.clearColor.b)),
  ] as const;
  return readback.format.startsWith('bgra8')
    ? [rgba[2], rgba[1], rgba[0]]
    : rgba;
}

function linearToSrgb(value: number): number {
  return value <= 0.003_130_8
    ? value * 12.92
    : 1.055 * value ** (1 / 2.4) - 0.055;
}

function identity(value: number): number {
  return value;
}

function byte(value: number): number {
  return Math.round(Math.max(0, Math.min(1, value)) * 255);
}
