import type { AvatarPacketHeader } from '../src/contract.js';

export function runtimeFeatures() {
  return {
    presentation: {
      avatar_instances: {
        magic: 0x50444152,
        version: 3,
        header_words: 16,
        record_words: 12,
        supported_flags: 0,
        default_max_records: 4,
        hard_max_records: 8,
        default_max_entities_scanned: 16,
        hard_max_entities_scanned: 32,
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
  };
}

export function packetHeader(
  overrides: Partial<AvatarPacketHeader> = {},
): AvatarPacketHeader {
  return Object.freeze({
    count: 0,
    streamId: 1n,
    sequence: 0n,
    frame: 0n,
    packetKind: 'full',
    baseSequence: 0n,
    flags: 0,
    ...overrides,
  });
}
