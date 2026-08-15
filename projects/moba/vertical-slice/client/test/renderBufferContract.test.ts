import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import {
  type AvatarPresentationDescriptor,
  isRenderableEntityId,
  parseAvatarDescriptor,
  parseAvatarPacket,
  signedI64AsSafeNumber,
} from '../src/render/renderBufferContract.js';

const descriptor: AvatarPresentationDescriptor = parseAvatarDescriptor({
  presentation: {
    avatar_instances: {
      magic: 0x50444152,
      version: 3,
      header_words: 16,
      record_words: 12,
      supported_flags: 0,
      default_max_records: 128,
      hard_max_records: 1024,
      default_max_entities_scanned: 4096,
      hard_max_entities_scanned: 8192,
      model_names: ['', 'clockwork_mage'],
      packet_kinds: { full: 0, delta: 1 },
      header_fields: {
        magic: 0,
        version: 1,
        record_words: 2,
        count: 3,
        stream_id_low: 4,
        stream_id_high: 5,
        sequence_low: 6,
        sequence_high: 7,
        frame_low: 8,
        frame_high: 9,
        packet_kind: 10,
        base_sequence_low: 11,
        base_sequence_high: 12,
        flags: 13,
        reserved_0: 14,
        reserved_1: 15,
      },
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
});

test('entity zero remains a valid render identity', () => {
  assert.equal(isRenderableEntityId(0), true);
  assert.equal(isRenderableEntityId(1), true);
});

test('malformed entity ids are rejected', () => {
  assert.equal(isRenderableEntityId(-1), false);
  assert.equal(isRenderableEntityId(Number.NaN), false);
  assert.equal(isRenderableEntityId(0x1_0000_0000), false);
});

test('an exact packet whose only record is entity zero is accepted', () => {
  const buffer = new Uint32Array(descriptor.headerWords + descriptor.recordWords);
  buffer.set([
    descriptor.magic, descriptor.version, descriptor.recordWords, 1,
    1, 0, 0, 0, 17, 0, descriptor.packetKinds.full, 0, 0, 0, 0, 0,
  ]);
  buffer[descriptor.headerWords + descriptor.fields.entity_slot] = 0;
  buffer[descriptor.headerWords + descriptor.fields.entity_generation] = 3;
  buffer[descriptor.headerWords + descriptor.fields.player_id] = 1;

  const header = parseAvatarPacket(buffer, descriptor);
  assert.equal(header.frame, 17n);
  assert.equal(header.count, 1);
});

test('packet parser rejects trailing and stale representations', () => {
  const valid = new Uint32Array(descriptor.headerWords);
  valid.set([descriptor.magic, descriptor.version, descriptor.recordWords, 0]);
  valid[descriptor.headerFields.stream_id_low] = 1;
  assert.equal(parseAvatarPacket(valid, descriptor).count, 0);

  const stale = valid.slice();
  stale[1] -= 1;
  assert.throws(() => parseAvatarPacket(stale, descriptor), /packet_version_mismatch/);

  const trailing = new Uint32Array(descriptor.headerWords + 1);
  trailing.set(valid);
  assert.throws(() => parseAvatarPacket(trailing, descriptor), /packet_length_mismatch/);
});

test('signed command IDs stay exact until JavaScript safe-number boundary', () => {
  assert.equal(signedI64AsSafeNumber(0xffff_ffff, 0xffff_ffff), -1);
  assert.equal(signedI64AsSafeNumber(123, 0), 123);
  assert.equal(signedI64AsSafeNumber(0, 0x0020_0000), null);
});
