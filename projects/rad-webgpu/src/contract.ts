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
  readonly fields: Readonly<Record<AvatarFieldName, number>>;
}

export interface AvatarPacketHeader {
  readonly count: number;
  readonly frame: bigint;
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
  const recordWords = unsignedIntegerAt(raw, 'record_words');
  const fields = Object.fromEntries(
    AVATAR_FIELD_NAMES.map((name) => [name, unsignedIntegerAt(fieldsRaw, name)]),
  ) as Record<AvatarFieldName, number>;
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

  const descriptor: AvatarPresentationDescriptor = {
    magic: unsignedIntegerAt(raw, 'magic'),
    version: positiveIntegerAt(raw, 'version'),
    headerWords: positiveIntegerAt(raw, 'header_words'),
    recordWords,
    supportedFlags: unsignedIntegerAt(raw, 'supported_flags'),
    defaultMaxRecords: positiveIntegerAt(raw, 'default_max_records'),
    hardMaxRecords: positiveIntegerAt(raw, 'hard_max_records'),
    defaultMaxEntitiesScanned: positiveIntegerAt(raw, 'default_max_entities_scanned'),
    hardMaxEntitiesScanned: positiveIntegerAt(raw, 'hard_max_entities_scanned'),
    modelNames: Object.freeze(modelNames),
    fields,
  };
  if (descriptor.headerWords < 8) throw new Error('presentation.descriptor_header_too_small');
  if (descriptor.recordWords === 0) throw new Error('presentation.descriptor_empty_record');
  if (descriptor.defaultMaxRecords > descriptor.hardMaxRecords) {
    throw new Error('presentation.descriptor_default_exceeds_hard_limit');
  }
  if (descriptor.defaultMaxEntitiesScanned > descriptor.hardMaxEntitiesScanned) {
    throw new Error('presentation.descriptor_default_scan_exceeds_hard_limit');
  }
  return Object.freeze({ ...descriptor, fields: Object.freeze({ ...fields }) });
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
  if (words[0] !== descriptor.magic) throw new Error('presentation.packet_magic_mismatch');
  if (words[1] !== descriptor.version) throw new Error('presentation.packet_version_mismatch');
  if (words[2] !== descriptor.recordWords) throw new Error('presentation.packet_stride_mismatch');

  const count = words[3] ?? 0;
  if (count > acceptedMaxRecords) throw new Error('presentation.packet_record_limit');
  const recordWordCount = checkedProduct(count, descriptor.recordWords);
  const expectedWords = checkedSum(descriptor.headerWords, recordWordCount);
  if (words.length !== expectedWords) throw new Error('presentation.packet_length_mismatch');
  const flags = words[6] ?? 0;
  if ((flags & ~descriptor.supportedFlags) !== 0) {
    throw new Error('presentation.packet_unsupported_flags');
  }
  if ((words[7] ?? 0) !== 0) throw new Error('presentation.packet_reserved_header_nonzero');
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
    frame: joinU64(words[4] ?? 0, words[5] ?? 0),
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
