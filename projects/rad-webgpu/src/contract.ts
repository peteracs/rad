export const AVATAR_FIELD_NAMES = [
  'entity_slot',
  'entity_generation',
  'player_id',
  'x',
  'y',
  'target_x',
  'target_y',
  'target_active',
  'command_id_low',
  'command_id_high',
  'model_id',
  'reserved',
] as const;

export type AvatarFieldName = (typeof AVATAR_FIELD_NAMES)[number];

export const AVATAR_HEADER_FIELD_NAMES = [
  'magic',
  'version',
  'record_words',
  'count',
  'stream_id_low',
  'stream_id_high',
  'sequence_low',
  'sequence_high',
  'frame_low',
  'frame_high',
  'packet_kind',
  'base_sequence_low',
  'base_sequence_high',
  'flags',
  'reserved_0',
  'reserved_1',
] as const;

export type AvatarHeaderFieldName = (typeof AVATAR_HEADER_FIELD_NAMES)[number];

export interface PresentationPacketKinds {
  readonly full: number;
  readonly delta: number;
}

export interface AvatarPresentationDescriptor {
  readonly magic: number;
  readonly version: number;
  readonly headerWords: number;
  readonly recordWords: number;
  readonly supportedFlags: number;
  readonly defaultMaxRecords: number;
  readonly hardMaxRecords: number;
  readonly defaultMaxEntitiesScanned: number;
  readonly hardMaxEntitiesScanned: number;
  readonly modelNames: readonly string[];
  readonly packetKinds: PresentationPacketKinds;
  readonly headerFields: Readonly<Record<AvatarHeaderFieldName, number>>;
  readonly fields: Readonly<Record<AvatarFieldName, number>>;
}

export type PresentationPacketKind = 'full' | 'delta';

export interface AvatarPacketHeader {
  readonly count: number;
  readonly streamId: bigint;
  readonly sequence: bigint;
  readonly frame: bigint;
  readonly packetKind: PresentationPacketKind;
  readonly baseSequence: bigint;
  readonly flags: number;
}

export interface WordRange {
  readonly firstWord: number;
  readonly wordCount: number;
}

export interface AvatarPresentationPacket {
  /** Ephemeral view into WASM memory; do not retain across another WASM call. */
  readonly words: Uint32Array;
  /** Record-only bytes, ready for a WebGPU storage-buffer upload. */
  readonly records: Uint32Array;
  readonly header: AvatarPacketHeader;
  readonly descriptor: AvatarPresentationDescriptor;
  /** Optional record-relative ranges supplied by a future incremental stream. */
  readonly dirtyRanges?: readonly WordRange[];
}

export function parseAvatarDescriptor(runtimeFeatures: string | unknown): AvatarPresentationDescriptor {
  const root = typeof runtimeFeatures === 'string' ? parseJson(runtimeFeatures) : runtimeFeatures;
  const raw = objectAt(objectAt(root, 'presentation'), 'avatar_instances');
  const fieldsRaw = objectAt(raw, 'fields');
  const headerFieldsRaw = objectAt(raw, 'header_fields');
  const packetKindsRaw = objectAt(raw, 'packet_kinds');
  const recordWords = unsignedIntegerAt(raw, 'record_words');
  const fields = Object.fromEntries(
    AVATAR_FIELD_NAMES.map((name) => [name, unsignedIntegerAt(fieldsRaw, name)]),
  ) as Record<AvatarFieldName, number>;
  const headerFields = Object.fromEntries(
    AVATAR_HEADER_FIELD_NAMES.map((name) => [name, unsignedIntegerAt(headerFieldsRaw, name)]),
  ) as Record<AvatarHeaderFieldName, number>;
  const packetKinds = {
    full: unsignedIntegerAt(packetKindsRaw, 'full'),
    delta: unsignedIntegerAt(packetKindsRaw, 'delta'),
  };
  const modelNames = stringArrayAt(raw, 'model_names');
  if (new Set(modelNames).size !== modelNames.length) {
    throw new Error('presentation.descriptor_duplicate_model_name');
  }

  const offsets = Object.values(fields);
  if (new Set(offsets).size !== offsets.length) {
    throw new Error('presentation.descriptor_duplicate_field_offset');
  }
  if (offsets.some((offset) => offset >= recordWords)) {
    throw new Error('presentation.descriptor_field_out_of_record');
  }
  const headerOffsets = Object.values(headerFields);
  if (new Set(headerOffsets).size !== headerOffsets.length) {
    throw new Error('presentation.descriptor_duplicate_header_field_offset');
  }
  const headerWords = positiveIntegerAt(raw, 'header_words');
  if (headerOffsets.some((offset) => offset >= headerWords)) {
    throw new Error('presentation.descriptor_header_field_out_of_header');
  }
  if (packetKinds.full === packetKinds.delta) {
    throw new Error('presentation.descriptor_duplicate_packet_kind');
  }

  const descriptor: AvatarPresentationDescriptor = {
    magic: unsignedIntegerAt(raw, 'magic'),
    version: positiveIntegerAt(raw, 'version'),
    headerWords,
    recordWords,
    supportedFlags: unsignedIntegerAt(raw, 'supported_flags'),
    defaultMaxRecords: positiveIntegerAt(raw, 'default_max_records'),
    hardMaxRecords: positiveIntegerAt(raw, 'hard_max_records'),
    defaultMaxEntitiesScanned: positiveIntegerAt(raw, 'default_max_entities_scanned'),
    hardMaxEntitiesScanned: positiveIntegerAt(raw, 'hard_max_entities_scanned'),
    modelNames: Object.freeze(modelNames),
    packetKinds: Object.freeze(packetKinds),
    headerFields,
    fields,
  };
  if (descriptor.headerWords < AVATAR_HEADER_FIELD_NAMES.length) {
    throw new Error('presentation.descriptor_header_too_small');
  }
  if (descriptor.recordWords === 0) throw new Error('presentation.descriptor_empty_record');
  if (descriptor.defaultMaxRecords > descriptor.hardMaxRecords) {
    throw new Error('presentation.descriptor_default_exceeds_hard_limit');
  }
  if (descriptor.defaultMaxEntitiesScanned > descriptor.hardMaxEntitiesScanned) {
    throw new Error('presentation.descriptor_default_scan_exceeds_hard_limit');
  }
  return Object.freeze({
    ...descriptor,
    headerFields: Object.freeze({ ...headerFields }),
    fields: Object.freeze({ ...fields }),
  });
}

export function parseAvatarPacket(
  words: Uint32Array,
  descriptor: AvatarPresentationDescriptor,
  acceptedMaxRecords = descriptor.defaultMaxRecords,
): AvatarPacketHeader {
  if (!Number.isInteger(acceptedMaxRecords) || acceptedMaxRecords <= 0) {
    throw new Error('presentation.invalid_host_record_limit');
  }
  if (acceptedMaxRecords > descriptor.hardMaxRecords) {
    throw new Error('presentation.host_record_limit_exceeds_runtime');
  }
  if (words.length < descriptor.headerWords) throw new Error('presentation.packet_header_too_small');
  const header = descriptor.headerFields;
  if (words[header.magic] !== descriptor.magic) throw new Error('presentation.packet_magic_mismatch');
  if (words[header.version] !== descriptor.version) {
    throw new Error('presentation.packet_version_mismatch');
  }
  if (words[header.record_words] !== descriptor.recordWords) {
    throw new Error('presentation.packet_stride_mismatch');
  }

  const count = words[header.count] ?? 0;
  if (count > acceptedMaxRecords) throw new Error('presentation.packet_record_limit');
  const recordWordCount = checkedProduct(count, descriptor.recordWords);
  const expectedWords = checkedSum(descriptor.headerWords, recordWordCount);
  if (words.length !== expectedWords) throw new Error('presentation.packet_length_mismatch');
  const flags = words[header.flags] ?? 0;
  if ((flags & ~descriptor.supportedFlags) !== 0) {
    throw new Error('presentation.packet_unsupported_flags');
  }
  if ((words[header.reserved_0] ?? 0) !== 0 || (words[header.reserved_1] ?? 0) !== 0) {
    throw new Error('presentation.packet_reserved_header_nonzero');
  }
  const packetKindWord = words[header.packet_kind] ?? 0;
  const packetKind = packetKindWord === descriptor.packetKinds.full
    ? 'full'
    : packetKindWord === descriptor.packetKinds.delta
      ? 'delta'
      : null;
  if (packetKind === null) throw new Error('presentation.packet_unknown_kind');
  const baseSequence = joinU64(
    words[header.base_sequence_low] ?? 0,
    words[header.base_sequence_high] ?? 0,
  );
  if (packetKind === 'full' && baseSequence !== 0n) {
    throw new Error('presentation.packet_full_has_base_sequence');
  }
  const streamId = joinU64(
    words[header.stream_id_low] ?? 0,
    words[header.stream_id_high] ?? 0,
  );
  if (streamId === 0n) throw new Error('presentation.packet_zero_stream_id');
  const sequence = joinU64(
    words[header.sequence_low] ?? 0,
    words[header.sequence_high] ?? 0,
  );
  if (packetKind === 'delta' && baseSequence >= sequence) {
    throw new Error('presentation.packet_invalid_delta_base');
  }
  const floats = new Float32Array(words.buffer, words.byteOffset, words.length);
  for (let index = 0; index < count; index += 1) {
    const offset = descriptor.headerWords + index * descriptor.recordWords;
    if ((words[offset + descriptor.fields.reserved] ?? 0) !== 0) {
      throw new Error('presentation.packet_reserved_record_nonzero');
    }
    if ((words[offset + descriptor.fields.target_active] ?? 0) > 1) {
      throw new Error('presentation.packet_invalid_boolean');
    }
    if ((words[offset + descriptor.fields.model_id] ?? 0) >= descriptor.modelNames.length) {
      throw new Error('presentation.packet_unknown_model');
    }
    for (const field of ['x', 'y', 'target_x', 'target_y'] as const) {
      if (!Number.isFinite(floats[offset + descriptor.fields[field]])) {
        throw new Error('presentation.packet_non_finite_float');
      }
    }
  }

  return Object.freeze({
    count,
    streamId,
    sequence,
    frame: joinU64(words[header.frame_low] ?? 0, words[header.frame_high] ?? 0),
    packetKind,
    baseSequence,
    flags,
  });
}

export function joinU64(low: number, high: number): bigint {
  return BigInt(low >>> 0) | (BigInt(high >>> 0) << 32n);
}

export function signedI64AsSafeNumber(low: number, high: number): number | null {
  const value = BigInt.asIntN(64, joinU64(low, high));
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : null;
}

export function isU32(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 0xffff_ffff;
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch (error) {
    throw new Error('presentation.runtime_features_invalid_json', { cause: error });
  }
}

function objectAt(value: unknown, name: string): Record<string, unknown> {
  const selected = name ? (isObject(value) ? value[name] : undefined) : value;
  if (!isObject(selected)) throw new Error(`presentation.descriptor_missing_${name || 'root'}`);
  return selected;
}

function positiveIntegerAt(value: Record<string, unknown>, name: string): number {
  const number = unsignedIntegerAt(value, name);
  if (number === 0) throw new Error(`presentation.descriptor_invalid_${name}`);
  return number;
}

function unsignedIntegerAt(value: Record<string, unknown>, name: string): number {
  const number = value[name];
  if (typeof number !== 'number' || !isU32(number)) {
    throw new Error(`presentation.descriptor_invalid_${name}`);
  }
  return number;
}

function stringArrayAt(value: Record<string, unknown>, name: string): string[] {
  const array = value[name];
  if (!Array.isArray(array) || array.some((item) => typeof item !== 'string')) {
    throw new Error(`presentation.descriptor_invalid_${name}`);
  }
  return [...array] as string[];
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function checkedProduct(left: number, right: number): number {
  const result = left * right;
  if (!Number.isSafeInteger(result)) throw new Error('presentation.packet_size_overflow');
  return result;
}

function checkedSum(left: number, right: number): number {
  const result = left + right;
  if (!Number.isSafeInteger(result)) throw new Error('presentation.packet_size_overflow');
  return result;
}
