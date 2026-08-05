import type { WordRange } from './contract.js';

export interface GpuBufferMirrorOptions {
  readonly label: string;
  readonly usage: GPUBufferUsageFlags;
  readonly maxBytes: number;
  readonly minimumCapacityBytes?: number;
}

/** A bounded, geometrically growing GPU buffer with explicit replacement cleanup. */
export class GpuBufferMirror {
  private bufferValue: GPUBuffer | null = null;
  private capacityBytesValue = 0;

  constructor(
    private readonly device: GPUDevice,
    private readonly options: GpuBufferMirrorOptions,
  ) {
    if (
      !Number.isSafeInteger(options.maxBytes)
      || options.maxBytes < 4
      || options.maxBytes % 4 !== 0
    ) {
      throw new Error('webgpu.invalid_buffer_limit');
    }
    const minimum = options.minimumCapacityBytes;
    if (
      minimum !== undefined
      && (!Number.isSafeInteger(minimum) || minimum <= 0 || minimum > options.maxBytes)
    ) {
      throw new Error('webgpu.invalid_minimum_buffer_capacity');
    }
  }

  get buffer(): GPUBuffer | null {
    return this.bufferValue;
  }

  get capacityBytes(): number {
    return this.capacityBytesValue;
  }

  upload(words: Uint32Array, ranges?: readonly WordRange[]): GPUBuffer {
    if (ranges) validateRanges(ranges, words.length);
    const byteLength = words.byteLength;
    const previous = this.bufferValue;
    const buffer = this.ensureCapacity(Math.max(byteLength, 4));
    if (byteLength === 0) return buffer;

    if (!ranges || buffer !== previous) {
      this.device.queue.writeBuffer(buffer, 0, words);
      return buffer;
    }
    for (const range of ranges) {
      if (range.wordCount === 0) continue;
      const slice = words.subarray(range.firstWord, range.firstWord + range.wordCount);
      this.device.queue.writeBuffer(
        buffer,
        range.firstWord * Uint32Array.BYTES_PER_ELEMENT,
        slice,
      );
    }
    return buffer;
  }

  destroy(): void {
    this.bufferValue?.destroy();
    this.bufferValue = null;
    this.capacityBytesValue = 0;
  }

  private ensureCapacity(requiredBytes: number): GPUBuffer {
    if (requiredBytes > this.options.maxBytes) throw new Error('webgpu.buffer_limit_exceeded');
    if (this.bufferValue && this.capacityBytesValue >= requiredBytes) return this.bufferValue;

    const minimum = Math.max(this.options.minimumCapacityBytes ?? 256, requiredBytes, 4);
    const capacity = Math.min(nextPowerOfTwo(minimum), this.options.maxBytes);
    if (capacity < requiredBytes) throw new Error('webgpu.buffer_limit_exceeded');

    const replacement = this.device.createBuffer({
      label: this.options.label,
      size: alignTo4(capacity),
      usage: this.options.usage | GPUBufferUsage.COPY_DST,
    });
    this.bufferValue?.destroy();
    this.bufferValue = replacement;
    this.capacityBytesValue = alignTo4(capacity);
    return replacement;
  }
}

function validateRanges(ranges: readonly WordRange[], totalWords: number): void {
  let previousEnd = 0;
  for (const range of ranges) {
    validateRange(range, totalWords);
    if (range.firstWord < previousEnd) throw new Error('webgpu.dirty_ranges_not_canonical');
    previousEnd = range.firstWord + range.wordCount;
  }
}

function validateRange(range: WordRange, totalWords: number): void {
  if (!Number.isSafeInteger(range.firstWord) || range.firstWord < 0) {
    throw new Error('webgpu.invalid_dirty_range');
  }
  if (!Number.isSafeInteger(range.wordCount) || range.wordCount < 0) {
    throw new Error('webgpu.invalid_dirty_range');
  }
  if (range.firstWord + range.wordCount > totalWords) {
    throw new Error('webgpu.dirty_range_out_of_bounds');
  }
}

function nextPowerOfTwo(value: number): number {
  let result = 1;
  while (result < value) result *= 2;
  return result;
}

function alignTo4(value: number): number {
  return Math.ceil(value / 4) * 4;
}
