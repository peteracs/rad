import {
  parseAvatarDescriptor,
  parseAvatarPacket,
  type AvatarPresentationDescriptor,
  type AvatarPresentationPacket,
} from './contract.js';

export interface RadPresentationRuntime {
  runtime_features(): string;
  session_render_buffer_refresh_bounded(maxRecords: number, maxEntitiesScanned: number): void;
  session_render_buffer_ptr(): number;
  session_render_buffer_u32_len(): number;
}

export interface WasmAvatarSourceOptions {
  readonly maxRecords?: number;
  readonly maxEntitiesScanned?: number;
}

/**
 * A zero-copy, source-neutral bridge from RAD's WASM packet to browser hosts.
 * Each view is reacquired after the refresh call because WASM memory growth
 * detaches old views. Consumers must finish synchronous reads/uploads before
 * invoking the runtime again.
 */
export class WasmAvatarPresentationSource {
  readonly descriptor: AvatarPresentationDescriptor;
  readonly maxRecords: number;
  readonly maxEntitiesScanned: number;

  constructor(
    private readonly runtime: RadPresentationRuntime,
    private readonly memory: WebAssembly.Memory,
    options: WasmAvatarSourceOptions = {},
  ) {
    this.descriptor = parseAvatarDescriptor(runtime.runtime_features());
    this.maxRecords = options.maxRecords ?? this.descriptor.defaultMaxRecords;
    if (!Number.isInteger(this.maxRecords) || this.maxRecords <= 0) {
      throw new Error('presentation.invalid_host_record_limit');
    }
    if (this.maxRecords > this.descriptor.hardMaxRecords) {
      throw new Error('presentation.host_record_limit_exceeds_runtime');
    }
    this.maxEntitiesScanned = options.maxEntitiesScanned
      ?? this.descriptor.defaultMaxEntitiesScanned;
    if (!Number.isInteger(this.maxEntitiesScanned) || this.maxEntitiesScanned <= 0) {
      throw new Error('presentation.invalid_host_entity_scan_limit');
    }
    if (this.maxEntitiesScanned > this.descriptor.hardMaxEntitiesScanned) {
      throw new Error('presentation.host_entity_scan_limit_exceeds_runtime');
    }
  }

  refresh(): AvatarPresentationPacket {
    this.runtime.session_render_buffer_refresh_bounded(
      this.maxRecords,
      this.maxEntitiesScanned,
    );
    const pointer = this.runtime.session_render_buffer_ptr();
    const length = this.runtime.session_render_buffer_u32_len();
    if (!Number.isInteger(pointer) || pointer < 0 || pointer % Uint32Array.BYTES_PER_ELEMENT !== 0) {
      throw new Error('presentation.packet_pointer_invalid');
    }
    if (!Number.isInteger(length) || length < 0) {
      throw new Error('presentation.packet_length_invalid');
    }
    const byteLength = length * Uint32Array.BYTES_PER_ELEMENT;
    const end = pointer + byteLength;
    if (!Number.isSafeInteger(end) || end > this.memory.buffer.byteLength) {
      throw new Error('presentation.packet_outside_wasm_memory');
    }

    const words = new Uint32Array(this.memory.buffer, pointer, length);
    const header = parseAvatarPacket(words, this.descriptor, this.maxRecords);
    return Object.freeze({
      words,
      records: words.subarray(this.descriptor.headerWords),
      header,
      descriptor: this.descriptor,
    });
  }
}
