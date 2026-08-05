import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import type { RadPresentationRuntime } from '../src/source.js';
import { WasmAvatarPresentationSource } from '../src/source.js';
import { runtimeFeatures } from './fixtures.js';

const descriptor = runtimeFeatures();

test('source reacquires its view after refresh grows WASM memory', () => {
  let memory = new WebAssembly.Memory({ initial: 1 });
  let pointer = 0;
  const runtime: RadPresentationRuntime = {
    runtime_features: () => JSON.stringify(descriptor),
    session_render_buffer_refresh_bounded(maxRecords: number, maxEntitiesScanned: number) {
      assert.equal(maxRecords, 4);
      assert.equal(maxEntitiesScanned, 16);
      memory.grow(1);
      pointer = 64;
      new Uint32Array(memory.buffer, pointer, 16).set([
        0x50444152, 3, 12, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
      ]);
    },
    session_render_buffer_ptr: () => pointer,
    session_render_buffer_u32_len: () => 16,
  };

  const source = new WasmAvatarPresentationSource(runtime, memory);
  const packet = source.refresh();
  assert.equal(packet.header.frame, 3n);
  assert.equal(packet.header.streamId, 5n);
  assert.equal(packet.words.buffer, memory.buffer);
});

test('source rejects a packet range outside WASM memory', () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const runtime: RadPresentationRuntime = {
    runtime_features: () => JSON.stringify(descriptor),
    session_render_buffer_refresh_bounded() {},
    session_render_buffer_ptr: () => memory.buffer.byteLength - 4,
    session_render_buffer_u32_len: () => 16,
  };
  const source = new WasmAvatarPresentationSource(runtime, memory);
  assert.throws(() => source.refresh(), /packet_outside_wasm_memory/);
});
