export function requirePacketSize(
  out: Uint8Array,
  size: number,
  protocolName: string,
): void {
  if (out.length !== size) {
    throw new Error(`${protocolName} packet buffer must be exactly ${size} bytes`);
  }
}

export function writeHeader(
  out: Uint8Array,
  magic: number,
  version: number,
  kind: number,
): void {
  out[HEADER_MAGIC_OFFSET] = magic;
  out[HEADER_VERSION_OFFSET] = version;
  out[HEADER_KIND_OFFSET] = kind;
}

export function hasHeaderPrefix(
  packet: Uint8Array,
  magic: number,
  version: number,
  kind: number,
): boolean {
  return packet.length >= PROTOCOL_HEADER_BYTES
    && packet[HEADER_MAGIC_OFFSET] === magic
    && packet[HEADER_VERSION_OFFSET] === version
    && packet[HEADER_KIND_OFFSET] === kind;
}

export function writeU32(out: Uint8Array, offset: number, value: number): void {
  const n = Math.trunc(value) >>> 0;
  out[offset] = n & 0xff;
  out[offset + 1] = (n >>> 8) & 0xff;
  out[offset + 2] = (n >>> 16) & 0xff;
  out[offset + 3] = (n >>> 24) & 0xff;
}

export function writeI32(out: Uint8Array, offset: number, value: number): void {
  writeU32(out, offset, Math.trunc(value));
}

export function readU32(packet: Uint8Array, offset: number): number {
  return (
    (packet[offset] ?? 0)
    | ((packet[offset + 1] ?? 0) << 8)
    | ((packet[offset + 2] ?? 0) << 16)
    | ((packet[offset + 3] ?? 0) << 24)
  ) >>> 0;
}

export function readI32(packet: Uint8Array, offset: number): number {
  const value = readU32(packet, offset);
  return value >= 0x80000000 ? value - 0x100000000 : value;
}

export function coordToWire(value: number): number {
  return Math.round(clamp(value, -1_000_000, 1_000_000) * COORD_SCALE);
}

export function coordFromWire(value: number): number {
  return value / COORD_SCALE;
}

function clamp(value: number, min: number, max: number): number {
  if (value < min) return min;
  if (value > max) return max;
  return value;
}
import {
  COORD_SCALE,
  HEADER_KIND_OFFSET,
  HEADER_MAGIC_OFFSET,
  HEADER_VERSION_OFFSET,
  PROTOCOL_HEADER_BYTES,
} from '../generated/matchProtocol.js';
