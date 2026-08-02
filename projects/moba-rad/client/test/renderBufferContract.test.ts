import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import {
  RENDER_BUFFER_HEADER_F32,
  RENDER_BUFFER_STRIDE_F32,
  isRenderableEntityId,
} from '../src/render/renderBufferContract.js';

// Regression: the locally-seeded champion is always the first-allocated RAD
// entity (id 0). A `<= 0` guard dropped it, so it froze at spawn while the
// server ghost rendered. Id 0 MUST be renderable.
test('entity id 0 (the locally seeded champion) is renderable', () => {
  assert.equal(isRenderableEntityId(0), true);
});

test('positive entity ids are renderable', () => {
  assert.equal(isRenderableEntityId(1), true);
  assert.equal(isRenderableEntityId(4096), true);
});

test('malformed entity ids are rejected', () => {
  assert.equal(isRenderableEntityId(-1), false);
  assert.equal(isRenderableEntityId(Number.NaN), false);
  assert.equal(isRenderableEntityId(Number.POSITIVE_INFINITY), false);
});

// Decoding a buffer whose single record is entity 0 must surface that record,
// mirroring applyRenderBuffer's loop guard.
test('a render buffer whose only record is entity 0 yields one renderable avatar', () => {
  const count = 1;
  const buffer = new Float32Array(RENDER_BUFFER_HEADER_F32 + count * RENDER_BUFFER_STRIDE_F32);
  buffer[0] = 1; // version
  buffer[1] = RENDER_BUFFER_STRIDE_F32;
  buffer[2] = count;
  // record: entity_id=0, player_id=1, x=10, y=20, ...
  buffer[RENDER_BUFFER_HEADER_F32 + 0] = 0;
  buffer[RENDER_BUFFER_HEADER_F32 + 1] = 1;

  const surfaced: number[] = [];
  for (let i = 0; i < count; i += 1) {
    const offset = RENDER_BUFFER_HEADER_F32 + i * RENDER_BUFFER_STRIDE_F32;
    const entityId = Math.trunc(buffer[offset] ?? -1);
    if (!isRenderableEntityId(entityId)) continue;
    surfaced.push(entityId);
  }

  assert.deepEqual(surfaced, [0]);
});
