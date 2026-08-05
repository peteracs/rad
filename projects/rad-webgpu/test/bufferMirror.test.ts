import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { GpuBufferMirror } from '../src/bufferMirror.js';

const usage = { COPY_DST: 8, STORAGE: 128 };
Object.assign(globalThis, { GPUBufferUsage: usage });

test('buffer mirror grows geometrically and destroys replaced buffers', () => {
  const created: FakeBuffer[] = [];
  const writes: number[] = [];
  const device = {
    queue: { writeBuffer: (_buffer: unknown, offset: number) => writes.push(offset) },
    createBuffer: ({ size }: { size: number }) => {
      const buffer = new FakeBuffer(size);
      created.push(buffer);
      return buffer;
    },
  } as unknown as GPUDevice;
  const mirror = new GpuBufferMirror(device, {
    label: 'test',
    usage: usage.STORAGE,
    maxBytes: 1024,
    minimumCapacityBytes: 16,
  });

  const first = mirror.upload(new Uint32Array(3));
  assert.equal(mirror.capacityBytes, 16);
  assert.equal(mirror.upload(new Uint32Array(4)), first);
  const second = mirror.upload(new Uint32Array(5));
  assert.notEqual(second, first);
  assert.equal(created[0]?.destroyed, true);
  assert.deepEqual(writes, [0, 0, 0]);
});

test('buffer mirror enforces byte and dirty-range bounds', () => {
  const device = {
    queue: { writeBuffer() {} },
    createBuffer: ({ size }: { size: number }) => new FakeBuffer(size),
  } as unknown as GPUDevice;
  const mirror = new GpuBufferMirror(device, {
    label: 'test',
    usage: usage.STORAGE,
    maxBytes: 16,
  });
  assert.throws(() => mirror.upload(new Uint32Array(5)), /buffer_limit_exceeded/);
  assert.throws(
    () => mirror.upload(new Uint32Array(2), [{ firstWord: 1, wordCount: 2 }]),
    /dirty_range_out_of_bounds/,
  );
  assert.throws(
    () => mirror.upload(new Uint32Array(2), [
      { firstWord: 1, wordCount: 1 },
      { firstWord: 0, wordCount: 1 },
    ]),
    /dirty_ranges_not_canonical/,
  );
  assert.throws(
    () => new GpuBufferMirror(device, { label: 'bad', usage: usage.STORAGE, maxBytes: 15 }),
    /invalid_buffer_limit/,
  );
  assert.throws(
    () => new GpuBufferMirror(device, {
      label: 'bad minimum',
      usage: usage.STORAGE,
      maxBytes: 16,
      minimumCapacityBytes: 17,
    }),
    /invalid_minimum_buffer_capacity/,
  );
});

class FakeBuffer {
  destroyed = false;

  constructor(readonly size: number) {}

  destroy(): void {
    this.destroyed = true;
  }
}
