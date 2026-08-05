import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { parseAvatarDescriptor, parseAvatarPacket } from '../src/contract.js';
import { runtimeFeatures } from './fixtures.js';

test('runtime descriptor is the single packet-layout authority', () => {
  const descriptor = parseAvatarDescriptor(runtimeFeatures());
  assert.equal(descriptor.version, 3);
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
  packet.set([
    descriptor.magic, descriptor.version, descriptor.recordWords, 1,
    7, 0, 3, 0, 9, 2, descriptor.packetKinds.full, 0, 0, 0, 0, 0,
  ]);
  const header = parseAvatarPacket(packet, descriptor);
  assert.equal(header.count, 1);
  assert.equal(header.streamId, 7n);
  assert.equal(header.sequence, 3n);
  assert.equal(header.frame, (2n << 32n) | 9n);
  assert.equal(header.packetKind, 'full');

  packet[3] = 5;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /packet_record_limit/);
});

test('packet validation rejects unsupported or semantically invalid record words', () => {
  const descriptor = parseAvatarDescriptor(runtimeFeatures());
  const packet = new Uint32Array(descriptor.headerWords + descriptor.recordWords);
  packet.set([
    descriptor.magic, descriptor.version, descriptor.recordWords, 1,
    1, 0, 0, 0, 0, 0, descriptor.packetKinds.full, 0, 0, 1, 0, 0,
  ]);
  assert.throws(() => parseAvatarPacket(packet, descriptor), /unsupported_flags/);

  packet[descriptor.headerFields.flags] = 0;
  packet[descriptor.headerWords + descriptor.fields.target_active] = 2;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /invalid_boolean/);

  packet[descriptor.headerWords + descriptor.fields.target_active] = 0;
  packet[descriptor.headerWords + descriptor.fields.model_id] = descriptor.modelNames.length;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /unknown_model/);

  packet[descriptor.headerWords + descriptor.fields.model_id] = 0;
  packet[descriptor.headerWords + descriptor.fields.x] = 0x7fc0_0000;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /non_finite_float/);
});

test('packet validation binds full and delta lineage fields', () => {
  const descriptor = parseAvatarDescriptor(runtimeFeatures());
  const packet = new Uint32Array(descriptor.headerWords);
  packet.set([
    descriptor.magic, descriptor.version, descriptor.recordWords, 0,
    9, 0, 4, 0, 20, 0, descriptor.packetKinds.delta, 3, 0, 0, 0, 0,
  ]);
  const header = parseAvatarPacket(packet, descriptor);
  assert.equal(header.packetKind, 'delta');
  assert.equal(header.baseSequence, 3n);

  packet[descriptor.headerFields.packet_kind] = descriptor.packetKinds.full;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /full_has_base_sequence/);

  packet[descriptor.headerFields.stream_id_low] = 0;
  packet[descriptor.headerFields.base_sequence_low] = 0;
  assert.throws(() => parseAvatarPacket(packet, descriptor), /zero_stream_id/);
});
