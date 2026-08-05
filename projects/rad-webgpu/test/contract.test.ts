import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { parseAvatarDescriptor, parseAvatarPacket } from '../src/contract.js';

function runtimeFeatures() {
  return {
    presentation: {
      avatar_instances: {
        magic: 0x50444152,
        version: 2,
        header_words: 8,
        record_words: 12,
        supported_flags: 0,
        default_max_records: 4,
        hard_max_records: 8,
        default_max_entities_scanned: 16,
        hard_max_entities_scanned: 32,
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
}

test('runtime descriptor is the single packet-layout authority', () => {
  const descriptor = parseAvatarDescriptor(runtimeFeatures());
  assert.equal(descriptor.version, 2);
  assert.equal(descriptor.fields.entity_generation, 1);
  assert.equal(descriptor.defaultMaxRecords, 4);
  assert.equal(Object.isFrozen(descriptor.fields), true);
});

test('descriptor rejects ambiguous and out-of-record fields', () => {
  const duplicate = structuredClone(runtimeFeatures());
  duplicate.presentation.avatar_instances.fields.x = 1;
  assert.throws(() => parseAvatarDescriptor(duplicate), /duplicate_field_offset/);

  const outside = structuredClone(runtimeFeatures());
  outside.presentation.avatar_instances.fields.x = 12;
  assert.throws(() => parseAvatarDescriptor(outside), /field_out_of_record/);
});

test('packet validation is exact and bounded', () => {
  const descriptor = parseAvatarDescriptor(runtimeFeatures());
  const packet = new Uint32Array(descriptor.headerWords + descriptor.recordWords);
  packet.set([descriptor.magic, descriptor.version, descriptor.recordWords, 1, 9, 2, 0, 0]);
  const header = parseAvatarPacket(packet, descriptor);
  assert.equal(header.count, 1);
  assert.equal(header.frame, (2n << 32n) | 9n);

  packet[3] = 5;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /packet_record_limit/);
});

test('packet validation rejects unsupported or semantically invalid record words', () => {
  const descriptor = parseAvatarDescriptor(runtimeFeatures());
  const packet = new Uint32Array(descriptor.headerWords + descriptor.recordWords);
  packet.set([descriptor.magic, descriptor.version, descriptor.recordWords, 1, 0, 0, 1, 0]);
  assert.throws(() => parseAvatarPacket(packet, descriptor), /unsupported_flags/);

  packet[6] = 0;
  packet[descriptor.headerWords + descriptor.fields.target_active] = 2;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /invalid_boolean/);

  packet[descriptor.headerWords + descriptor.fields.target_active] = 0;
  packet[descriptor.headerWords + descriptor.fields.model_id] = descriptor.modelNames.length;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /unknown_model/);

  packet[descriptor.headerWords + descriptor.fields.model_id] = 0;
  packet[descriptor.headerWords + descriptor.fields.x] = 0x7fc0_0000;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /non_finite_float/);
});
