import type {
  ServerAvatarState,
  ServerPeerState,
  ServerProjectileImpactState,
  ServerProjectileState,
  ServerState,
} from './serverState';
import { DEFAULT_AVATAR_MODEL } from '../render/avatarModelId.js';
import {
  coordFromWire,
  coordToWire,
  hasHeaderPrefix,
  readI32,
  readU32,
  requirePacketSize,
  writeHeader,
  writeI32,
  writeU32,
} from './matchWire.js';
import {
  createServerStateBuffer,
  resizeServerStateRecords,
} from './serverStateBuffer.js';

// Mirrors server/src/protocol/match_protocol.rad. The WebTransport edge proxy
// forwards these bytes without parsing, so protocol ownership stays here and
// in the RAD server module.

export interface MatchClientIdentity {
  sessionId: number;
  playerId: number;
}

export const MATCH_PROTOCOL_NAME = 'moba-rad/udp-v10-peer-snapshot';
export const PROTOCOL_MAGIC = 0x4d;
export const PROTOCOL_VERSION = 10;
export const PACKET_KIND_SYNC = 1;
export const PACKET_KIND_MOVE = 2;
export const PACKET_KIND_DISCONNECT = 3;
export const PACKET_KIND_STATE = 4;
export const PACKET_KIND_ERROR = 5;
export const PACKET_KIND_CAST = 6;
export const SYNC_PACKET_BYTES = 15;
export const MOVE_PACKET_BYTES = 31;
export const DISCONNECT_PACKET_BYTES = 15;
export const CAST_PACKET_BYTES = 35;
export const STATE_PACKET_HEADER_BYTES = 92;
export const STATE_PEER_RECORD_BYTES = 44;
export const STATE_AVATAR_RECORD_BYTES = 26;
export const STATE_PROJECTILE_RECORD_BYTES = 36;
export const STATE_PROJECTILE_IMPACT_RECORD_BYTES = 25;
export const STATE_PACKET_BYTES = STATE_PACKET_HEADER_BYTES + STATE_AVATAR_RECORD_BYTES;
export const ERROR_PACKET_BYTES = 4;

export function encodeMoveOrderPacket(
  identity: MatchClientIdentity,
  clientSeq: number,
  targetTick: number,
  commandId: number,
  targetX: number,
  targetY: number,
  out: Uint8Array = new Uint8Array(MOVE_PACKET_BYTES),
): Uint8Array {
  requirePacketSize(out, MOVE_PACKET_BYTES, MATCH_PROTOCOL_NAME);
  writeHeader(out, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_MOVE);
  writeU32(out, 3, clientSeq);
  writeU32(out, 7, identity.sessionId);
  writeU32(out, 11, identity.playerId);
  writeU32(out, 15, targetTick);
  writeU32(out, 19, commandId);
  writeI32(out, 23, coordToWire(targetX));
  writeI32(out, 27, coordToWire(targetY));
  return out;
}

export function encodeSyncPacket(
  identity: MatchClientIdentity,
  clientSeq: number,
  out: Uint8Array = new Uint8Array(SYNC_PACKET_BYTES),
): Uint8Array {
  requirePacketSize(out, SYNC_PACKET_BYTES, MATCH_PROTOCOL_NAME);
  writeHeader(out, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_SYNC);
  writeU32(out, 3, clientSeq);
  writeU32(out, 7, identity.sessionId);
  writeU32(out, 11, identity.playerId);
  return out;
}

export function encodeDisconnectPacket(
  identity: MatchClientIdentity,
  clientSeq: number,
  out: Uint8Array = new Uint8Array(DISCONNECT_PACKET_BYTES),
): Uint8Array {
  requirePacketSize(out, DISCONNECT_PACKET_BYTES, MATCH_PROTOCOL_NAME);
  writeHeader(out, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_DISCONNECT);
  writeU32(out, 3, clientSeq);
  writeU32(out, 7, identity.sessionId);
  writeU32(out, 11, identity.playerId);
  return out;
}

export function encodeCastPacket(
  identity: MatchClientIdentity,
  clientSeq: number,
  targetTick: number,
  commandId: number,
  dirX: number,
  dirY: number,
  fireViewTick: number,
  out: Uint8Array = new Uint8Array(CAST_PACKET_BYTES),
): Uint8Array {
  requirePacketSize(out, CAST_PACKET_BYTES, MATCH_PROTOCOL_NAME);
  writeHeader(out, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_CAST);
  writeU32(out, 3, clientSeq);
  writeU32(out, 7, identity.sessionId);
  writeU32(out, 11, identity.playerId);
  writeU32(out, 15, targetTick);
  writeU32(out, 19, commandId);
  writeI32(out, 23, coordToWire(dirX));
  writeI32(out, 27, coordToWire(dirY));
  writeU32(out, 31, fireViewTick);
  return out;
}

export function parseServerStatePacket(
  packet: Uint8Array,
  out: ServerState = createServerStateBuffer(),
): ServerState | null {
  if (!hasHeaderPrefix(packet, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_STATE)) return null;
  if (packet.length < STATE_PACKET_HEADER_BYTES) return null;

  const serverMs = readU32(packet, 3);
  const serverTick = readU32(packet, 7);
  const serverSeq = readU32(packet, 11);
  const sessionId = readU32(packet, 15);
  const playerId = readU32(packet, 19);
  const ackClientSeq = readU32(packet, 23);
  const ackBits = readU32(packet, 27);
  const commandId = readU32(packet, 31);
  const x = coordFromWire(readI32(packet, 35));
  const y = coordFromWire(readI32(packet, 39));
  const targetX = coordFromWire(readI32(packet, 43));
  const targetY = coordFromWire(readI32(packet, 47));
  const avatarCount = packet[54] ?? 0;
  const projectileCount = packet[55] ?? 0;
  const projectileImpactCount = packet[62] ?? 0;
  const peerRecordCount = packet[63] ?? 0;
  const peerOffset = STATE_PACKET_HEADER_BYTES;
  const avatarOffset = peerOffset + peerRecordCount * STATE_PEER_RECORD_BYTES;
  const projectileOffset = avatarOffset + avatarCount * STATE_AVATAR_RECORD_BYTES;
  const impactOffset = projectileOffset + projectileCount * STATE_PROJECTILE_RECORD_BYTES;
  if (
    packet.length !== impactOffset + projectileImpactCount * STATE_PROJECTILE_IMPACT_RECORD_BYTES
  ) {
    return null;
  }

  out.ok = true;
  out.status = statusFromCode(packet[52] ?? 0);
  out.correction_reason = correctionReasonFromCode(packet[53] ?? 0);
  out.server_ms = serverMs;
  out.server_tick = serverTick;
  out.server_seq = serverSeq;
  out.session_id = sessionId;
  out.player_id = playerId;
  out.ack_client_seq = ackClientSeq;
  out.ack_bits = ackBits;
  out.command_id = commandId;

  out.avatar.player_id = playerId;
  out.avatar.model = DEFAULT_AVATAR_MODEL;
  out.avatar.x = x;
  out.avatar.y = y;
  out.avatar.target_x = targetX;
  out.avatar.target_y = targetY;
  out.avatar.target_active = (packet[51] ?? 0) !== 0;
  out.avatar.command_id = commandId;

  out.authority.peer_count = packet[56] ?? 0;
  out.authority.max_peers = packet[57] ?? 0;
  out.authority.input_queue_slots = packet[58] ?? 0;
  out.authority.pending_move_inputs = packet[59] ?? 0;
  out.authority.pending_cast_inputs = packet[60] ?? 0;
  out.authority.peer_connected = (packet[61] ?? 0) !== 0;
  out.authority.late_inputs = readU32(packet, 64);
  out.authority.future_inputs = readU32(packet, 68);
  out.authority.duplicate_inputs = readU32(packet, 72);
  out.authority.overwritten_inputs = readU32(packet, 76);
  out.authority.last_client_seq = readU32(packet, 80);
  out.authority.last_applied_client_seq = readU32(packet, 84);
  out.authority.applied_ack_bits = readU32(packet, 88);

  resizeServerStateRecords(
    out,
    peerRecordCount,
    avatarCount,
    projectileCount,
    projectileImpactCount,
  );

  for (let i = 0; i < peerRecordCount; i += 1) {
    writePeerRecord(out.peers[i], packet, peerOffset + i * STATE_PEER_RECORD_BYTES);
  }
  for (let i = 0; i < avatarCount; i += 1) {
    writeAvatarRecord(out.avatars[i], packet, avatarOffset + i * STATE_AVATAR_RECORD_BYTES);
  }
  for (let i = 0; i < projectileCount; i += 1) {
    writeProjectileRecord(
      out.projectiles[i],
      packet,
      projectileOffset + i * STATE_PROJECTILE_RECORD_BYTES,
    );
  }
  for (let i = 0; i < projectileImpactCount; i += 1) {
    writeProjectileImpactRecord(
      out.projectile_impacts[i],
      packet,
      impactOffset + i * STATE_PROJECTILE_IMPACT_RECORD_BYTES,
    );
  }

  return out;
}

function writePeerRecord(out: ServerPeerState, packet: Uint8Array, offset: number): void {
  out.player_id = readU32(packet, offset);
  out.session_id = readU32(packet, offset + 4);
  out.last_client_seq = readU32(packet, offset + 8);
  out.received_client_seq = readU32(packet, offset + 12);
  out.last_applied_client_seq = readU32(packet, offset + 16);
  out.applied_ack_bits = readU32(packet, offset + 20);
  out.pending_move_inputs = packet[offset + 24] ?? 0;
  out.pending_cast_inputs = packet[offset + 25] ?? 0;
  out.connected = (packet[offset + 26] ?? 0) !== 0;
  out.late_inputs = readU32(packet, offset + 28);
  out.future_inputs = readU32(packet, offset + 32);
  out.duplicate_inputs = readU32(packet, offset + 36);
  out.overwritten_inputs = readU32(packet, offset + 40);
}

function writeAvatarRecord(out: ServerAvatarState, packet: Uint8Array, offset: number): void {
  out.player_id = readU32(packet, offset);
  out.command_id = readU32(packet, offset + 4);
  out.x = coordFromWire(readI32(packet, offset + 8));
  out.y = coordFromWire(readI32(packet, offset + 12));
  out.target_x = coordFromWire(readI32(packet, offset + 16));
  out.target_y = coordFromWire(readI32(packet, offset + 20));
  out.target_active = (packet[offset + 24] ?? 0) !== 0;
  out.model = modelFromCode(packet[offset + 25] ?? 0);
}

function writeProjectileRecord(out: ServerProjectileState, packet: Uint8Array, offset: number): void {
  out.projectile_id = readU32(packet, offset);
  out.owner_id = readU32(packet, offset + 4);
  out.command_id = readU32(packet, offset + 8);
  out.x = coordFromWire(readI32(packet, offset + 12));
  out.y = coordFromWire(readI32(packet, offset + 16));
  out.velocity_x = coordFromWire(readI32(packet, offset + 20));
  out.velocity_y = coordFromWire(readI32(packet, offset + 24));
  out.spawn_tick = readU32(packet, offset + 28);
  out.fire_view_tick = readU32(packet, offset + 32);
}

function writeProjectileImpactRecord(
  out: ServerProjectileImpactState,
  packet: Uint8Array,
  offset: number,
): void {
  out.event_id = readU32(packet, offset);
  out.projectile_id = readU32(packet, offset + 4);
  out.owner_id = readU32(packet, offset + 8);
  out.target_id = readU32(packet, offset + 12);
  out.x = coordFromWire(readI32(packet, offset + 16));
  out.y = coordFromWire(readI32(packet, offset + 20));
  out.reason = projectileImpactReasonFromCode(packet[offset + 24] ?? 0);
}

function projectileImpactReasonFromCode(code: number): string {
  switch (code) {
    case 1: return 'hit';
    case 2: return 'range';
    case 3: return 'lifetime';
    default: return 'unknown';
  }
}

function modelFromCode(code: number): string {
  switch (code) {
    case 1: return 'clockwork_mage';
    default: return DEFAULT_AVATAR_MODEL;
  }
}

function statusFromCode(code: number): string {
  switch (code) {
    case 1: return 'sync';
    case 2: return 'queued';
    case 3: return 'duplicate';
    case 4: return 'late';
    case 5: return 'too-far-ahead';
    case 6: return 'applied';
    case 7: return 'snapshot';
    case 8: return 'player-conflict';
    case 9: return 'peer-table-full';
    case 10: return 'rejected';
    case 11: return 'expired';
    case 12: return 'bye';
    case 13: return 'bye-miss';
    case 14: return 'full-sync';
    case 15: return 'cast-queued';
    case 16: return 'cast';
    default: return 'unknown';
  }
}

function correctionReasonFromCode(code: number): string {
  switch (code) {
    case 1: return 'applied';
    case 2: return 'late';
    case 3: return 'too-far-ahead';
    case 4: return 'duplicate';
    case 5: return 'expired';
    case 6: return 'queued';
    case 7: return 'sync';
    case 8: return 'snapshot';
    case 9: return 'player-conflict';
    case 10: return 'peer-table-full';
    case 11: return 'rejected';
    case 12: return 'cast';
    default: return 'none';
  }
}
