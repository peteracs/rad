import type { AvatarRenderTargetSnapshot } from './avatarRenderTarget.js';

export interface NormalizedRgbaColor {
  readonly r: number;
  readonly g: number;
  readonly b: number;
  readonly a: number;
}

export interface AvatarFrameReadback {
  readonly width: number;
  readonly height: number;
  readonly bytesPerRow: number;
  readonly format: GPUTextureFormat;
  readonly clearColor: NormalizedRgbaColor;
  readonly pixels: Uint8Array;
}

/** Copies an already-rendered target into one bounded CPU-owned pixel array. */
export async function readTexturePixels(
  device: GPUDevice,
  target: AvatarRenderTargetSnapshot,
  clearColor: NormalizedRgbaColor,
  maxBytes: number,
): Promise<AvatarFrameReadback> {
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
    throw new Error('webgpu.invalid_readback_limit');
  }
  if (!target.format.startsWith('rgba8') && !target.format.startsWith('bgra8')) {
    throw new Error(`webgpu.unsupported_readback_format:${target.format}`);
  }

  const bytesPerRow = alignTo(checkedProduct(target.width, 4, 'readback_row'), 256);
  const byteLength = checkedProduct(bytesPerRow, target.height, 'readback_size');
  if (byteLength > maxBytes || byteLength > Number(device.limits.maxBufferSize)) {
    throw new Error('webgpu.readback_limit_exceeded');
  }

  const buffer = device.createBuffer({
    label: 'RAD avatar frame readback',
    size: byteLength,
    usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
  });
  let mapped = false;
  try {
    const encoder = device.createCommandEncoder({ label: 'RAD avatar readback copy' });
    encoder.copyTextureToBuffer(
      { texture: target.texture },
      { buffer, bytesPerRow, rowsPerImage: target.height },
      { width: target.width, height: target.height, depthOrArrayLayers: 1 },
    );
    device.queue.submit([encoder.finish()]);
    await buffer.mapAsync(GPUMapMode.READ);
    mapped = true;
    return Object.freeze({
      width: target.width,
      height: target.height,
      bytesPerRow,
      format: target.format,
      clearColor,
      pixels: new Uint8Array(buffer.getMappedRange()).slice(),
    });
  } finally {
    if (mapped) buffer.unmap();
    buffer.destroy();
  }
}

function checkedProduct(left: number, right: number, name: string): number {
  const product = left * right;
  if (!Number.isSafeInteger(product) || product <= 0) {
    throw new Error(`webgpu.invalid_${name}`);
  }
  return product;
}

function alignTo(value: number, alignment: number): number {
  return Math.ceil(value / alignment) * alignment;
}
