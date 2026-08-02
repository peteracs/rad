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
  out[0] = magic;
  out[1] = version;
  out[2] = kind;
}

export function hasHeaderPrefix(
  packet: Uint8Array,
  magic: number,
  version: number,
  kind: number,
): boolean {
  return packet.length >= 3
    && packet[0] === magic
    && packet[1] === version
    && packet[2] === kind;
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

const COORD_SCALE = 1000;

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
