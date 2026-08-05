import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import type { RadPresentationRuntime } from '../src/source.js';
import { WasmAvatarPresentationSource } from '../src/source.js';

const descriptor = {
  presentation: {
    avatar_instances: {
      magic: 7,
      version: 2,
      header_words: 8,
      record_words: 12,
      supported_flags: 0,
      default_max_records: 2,
      hard_max_records: 4,
      default_max_entities_scanned: 8,
      hard_max_entities_scanned: 16,
      model_names: ['', 'clockwork_mage'],
      fields: {
        entity_slot: 0,
        entity_generation: 1,
        player_id: 2,
        x: 3,
        y: 4,
        target_x: 5,
        target_y: 6,
        target_active: 7,
        command_id_low: 8,
        command_id_high: 9,
        model_id: 10,
        reserved: 11,
      },
    },
  },
};

test('source reacquires its view after refresh grows WASM memory', () => {
  let memory = new WebAssembly.Memory({ initial: 1 });
  let pointer = 0;
  const runtime: RadPresentationRuntime = {
    runtime_features: () => JSON.stringify(descriptor),
    session_render_buffer_refresh_bounded(maxRecords: number, maxEntitiesScanned: number) {
      assert.equal(maxRecords, 2);
      assert.equal(maxEntitiesScanned, 8);
      memory.grow(1);
      pointer = 64;
      new Uint32Array(memory.buffer, pointer, 8).set([7, 2, 12, 0, 3, 0, 0, 0]);
    },
    session_render_buffer_ptr: () => pointer,
    session_render_buffer_u32_len: () => 8,
  };

  const source = new WasmAvatarPresentationSource(runtime, memory);
  const packet = source.refresh();
  assert.equal(packet.header.frame, 3n);
  assert.equal(packet.words.buffer, memory.buffer);
});

test('source rejects a packet range outside WASM memory', () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const runtime: RadPresentationRuntime = {
    runtime_features: () => JSON.stringify(descriptor),
    session_render_buffer_refresh_bounded() {},
    session_render_buffer_ptr: () => memory.buffer.byteLength - 4,
    session_render_buffer_u32_len: () => 8,
  };
  const source = new WasmAvatarPresentationSource(runtime, memory);
  assert.throws(() => source.refresh(), /packet_outside_wasm_memory/);
});
