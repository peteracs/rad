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
import * as contract from '../generated/matchProtocol.js';

export * from '../generated/matchProtocol.js';

const {
  CAST_PACKET_BYTES,
  correctionReasonFromCode,
  DISCONNECT_PACKET_BYTES,
  MATCH_PROTOCOL_NAME,
  modelFromCode,
  MOVE_PACKET_BYTES,
  PACKET_KIND_CAST,
  PACKET_KIND_DISCONNECT,
  PACKET_KIND_MOVE,
  PACKET_KIND_STATE,
  PACKET_KIND_SYNC,
  projectileImpactReasonFromCode,
  PROTOCOL_MAGIC,
  PROTOCOL_VERSION,
  STATE_AVATAR_RECORD_BYTES,
  STATE_PACKET_HEADER_BYTES,
  STATE_PEER_RECORD_BYTES,
  STATE_PROJECTILE_IMPACT_RECORD_BYTES,
  STATE_PROJECTILE_RECORD_BYTES,
  statusFromCode,
  SYNC_PACKET_BYTES,
} = contract;

// The WebTransport edge proxy forwards these bytes without parsing. Numeric
// identities, sizes, offsets, and code tables come from protocol/contract.json
// through the generated binding imported above.

export interface MatchClientIdentity {
  sessionId: number;
  playerId: number;
}

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
  writeU32(out, contract.MOVE_CLIENT_SEQ_OFFSET, clientSeq);
  writeU32(out, contract.MOVE_SESSION_ID_OFFSET, identity.sessionId);
  writeU32(out, contract.MOVE_PLAYER_ID_OFFSET, identity.playerId);
  writeU32(out, contract.MOVE_TARGET_TICK_OFFSET, targetTick);
  writeU32(out, contract.MOVE_COMMAND_ID_OFFSET, commandId);
  writeI32(out, contract.MOVE_TARGET_X_OFFSET, coordToWire(targetX));
  writeI32(out, contract.MOVE_TARGET_Y_OFFSET, coordToWire(targetY));
  return out;
}

export function encodeSyncPacket(
  identity: MatchClientIdentity,
  clientSeq: number,
  out: Uint8Array = new Uint8Array(SYNC_PACKET_BYTES),
): Uint8Array {
  requirePacketSize(out, SYNC_PACKET_BYTES, MATCH_PROTOCOL_NAME);
  writeHeader(out, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_SYNC);
  writeU32(out, contract.SYNC_CLIENT_SEQ_OFFSET, clientSeq);
  writeU32(out, contract.SYNC_SESSION_ID_OFFSET, identity.sessionId);
  writeU32(out, contract.SYNC_PLAYER_ID_OFFSET, identity.playerId);
  return out;
}

export function encodeDisconnectPacket(
  identity: MatchClientIdentity,
  clientSeq: number,
  out: Uint8Array = new Uint8Array(DISCONNECT_PACKET_BYTES),
): Uint8Array {
  requirePacketSize(out, DISCONNECT_PACKET_BYTES, MATCH_PROTOCOL_NAME);
  writeHeader(out, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_DISCONNECT);
  writeU32(out, contract.DISCONNECT_CLIENT_SEQ_OFFSET, clientSeq);
  writeU32(out, contract.DISCONNECT_SESSION_ID_OFFSET, identity.sessionId);
  writeU32(out, contract.DISCONNECT_PLAYER_ID_OFFSET, identity.playerId);
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
  writeU32(out, contract.CAST_CLIENT_SEQ_OFFSET, clientSeq);
  writeU32(out, contract.CAST_SESSION_ID_OFFSET, identity.sessionId);
  writeU32(out, contract.CAST_PLAYER_ID_OFFSET, identity.playerId);
  writeU32(out, contract.CAST_TARGET_TICK_OFFSET, targetTick);
  writeU32(out, contract.CAST_COMMAND_ID_OFFSET, commandId);
  writeI32(out, contract.CAST_DIRECTION_X_OFFSET, coordToWire(dirX));
  writeI32(out, contract.CAST_DIRECTION_Y_OFFSET, coordToWire(dirY));
  writeU32(out, contract.CAST_FIRE_VIEW_TICK_OFFSET, fireViewTick);
  return out;
}

export function parseServerStatePacket(
  packet: Uint8Array,
  out: ServerState = createServerStateBuffer(),
): ServerState | null {
  if (!hasHeaderPrefix(packet, PROTOCOL_MAGIC, PROTOCOL_VERSION, PACKET_KIND_STATE)) return null;
  if (packet.length < STATE_PACKET_HEADER_BYTES) return null;

  const serverMs = readU32(packet, contract.STATE_SERVER_MS_OFFSET);
  const serverTick = readU32(packet, contract.STATE_SERVER_TICK_OFFSET);
  const serverSeq = readU32(packet, contract.STATE_SERVER_SEQ_OFFSET);
  const sessionId = readU32(packet, contract.STATE_SESSION_ID_OFFSET);
  const playerId = readU32(packet, contract.STATE_PLAYER_ID_OFFSET);
  const ackClientSeq = readU32(packet, contract.STATE_ACK_CLIENT_SEQ_OFFSET);
  const ackBits = readU32(packet, contract.STATE_ACK_BITS_OFFSET);
  const commandId = readU32(packet, contract.STATE_COMMAND_ID_OFFSET);
  const x = coordFromWire(readI32(packet, contract.STATE_X_OFFSET));
  const y = coordFromWire(readI32(packet, contract.STATE_Y_OFFSET));
  const targetX = coordFromWire(readI32(packet, contract.STATE_TARGET_X_OFFSET));
  const targetY = coordFromWire(readI32(packet, contract.STATE_TARGET_Y_OFFSET));
  const avatarCount = packet[contract.STATE_AVATAR_COUNT_OFFSET] ?? 0;
  const projectileCount = packet[contract.STATE_PROJECTILE_COUNT_OFFSET] ?? 0;
  const projectileImpactCount = packet[contract.STATE_PROJECTILE_IMPACT_COUNT_OFFSET] ?? 0;
  const peerRecordCount = packet[contract.STATE_PEER_RECORD_COUNT_OFFSET] ?? 0;
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
  out.status = statusFromCode(packet[contract.STATE_STATUS_OFFSET] ?? 0);
  out.correction_reason = correctionReasonFromCode(
    packet[contract.STATE_CORRECTION_REASON_OFFSET] ?? 0,
  );
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
  out.avatar.target_active = (packet[contract.STATE_TARGET_ACTIVE_OFFSET] ?? 0) !== 0;
  out.avatar.command_id = commandId;

  out.authority.peer_count = packet[contract.STATE_PEER_COUNT_OFFSET] ?? 0;
  out.authority.max_peers = packet[contract.STATE_MAX_PEERS_OFFSET] ?? 0;
  out.authority.input_queue_slots = packet[contract.STATE_INPUT_QUEUE_SLOTS_OFFSET] ?? 0;
  out.authority.pending_move_inputs = packet[contract.STATE_PENDING_MOVE_INPUTS_OFFSET] ?? 0;
  out.authority.pending_cast_inputs = packet[contract.STATE_PENDING_CAST_INPUTS_OFFSET] ?? 0;
  out.authority.peer_connected = (packet[contract.STATE_PEER_CONNECTED_OFFSET] ?? 0) !== 0;
  out.authority.late_inputs = readU32(packet, contract.STATE_LATE_INPUTS_OFFSET);
  out.authority.future_inputs = readU32(packet, contract.STATE_FUTURE_INPUTS_OFFSET);
  out.authority.duplicate_inputs = readU32(packet, contract.STATE_DUPLICATE_INPUTS_OFFSET);
  out.authority.overwritten_inputs = readU32(packet, contract.STATE_OVERWRITTEN_INPUTS_OFFSET);
  out.authority.last_client_seq = readU32(packet, contract.STATE_LAST_CLIENT_SEQ_OFFSET);
  out.authority.last_applied_client_seq = readU32(
    packet,
    contract.STATE_LAST_APPLIED_CLIENT_SEQ_OFFSET,
  );
  out.authority.applied_ack_bits = readU32(packet, contract.STATE_APPLIED_ACK_BITS_OFFSET);

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
  out.player_id = readU32(packet, offset + contract.STATE_PEER_RECORD_PLAYER_ID_OFFSET);
  out.session_id = readU32(packet, offset + contract.STATE_PEER_RECORD_SESSION_ID_OFFSET);
  out.last_client_seq = readU32(packet, offset + contract.STATE_PEER_RECORD_LAST_CLIENT_SEQ_OFFSET);
  out.received_client_seq = readU32(packet, offset + contract.STATE_PEER_RECORD_RECEIVED_CLIENT_SEQ_OFFSET);
  out.last_applied_client_seq = readU32(packet, offset + contract.STATE_PEER_RECORD_LAST_APPLIED_CLIENT_SEQ_OFFSET);
  out.applied_ack_bits = readU32(packet, offset + contract.STATE_PEER_RECORD_APPLIED_ACK_BITS_OFFSET);
  out.pending_move_inputs = packet[offset + contract.STATE_PEER_RECORD_PENDING_MOVE_INPUTS_OFFSET] ?? 0;
  out.pending_cast_inputs = packet[offset + contract.STATE_PEER_RECORD_PENDING_CAST_INPUTS_OFFSET] ?? 0;
  out.connected = (packet[offset + contract.STATE_PEER_RECORD_CONNECTED_OFFSET] ?? 0) !== 0;
  out.late_inputs = readU32(packet, offset + contract.STATE_PEER_RECORD_LATE_INPUTS_OFFSET);
  out.future_inputs = readU32(packet, offset + contract.STATE_PEER_RECORD_FUTURE_INPUTS_OFFSET);
  out.duplicate_inputs = readU32(packet, offset + contract.STATE_PEER_RECORD_DUPLICATE_INPUTS_OFFSET);
  out.overwritten_inputs = readU32(packet, offset + contract.STATE_PEER_RECORD_OVERWRITTEN_INPUTS_OFFSET);
}

function writeAvatarRecord(out: ServerAvatarState, packet: Uint8Array, offset: number): void {
  out.player_id = readU32(packet, offset + contract.STATE_AVATAR_RECORD_PLAYER_ID_OFFSET);
  out.command_id = readU32(packet, offset + contract.STATE_AVATAR_RECORD_COMMAND_ID_OFFSET);
  out.x = coordFromWire(readI32(packet, offset + contract.STATE_AVATAR_RECORD_X_OFFSET));
  out.y = coordFromWire(readI32(packet, offset + contract.STATE_AVATAR_RECORD_Y_OFFSET));
  out.target_x = coordFromWire(readI32(packet, offset + contract.STATE_AVATAR_RECORD_TARGET_X_OFFSET));
  out.target_y = coordFromWire(readI32(packet, offset + contract.STATE_AVATAR_RECORD_TARGET_Y_OFFSET));
  out.target_active = (packet[offset + contract.STATE_AVATAR_RECORD_TARGET_ACTIVE_OFFSET] ?? 0) !== 0;
  out.model = modelFromCode(packet[offset + contract.STATE_AVATAR_RECORD_MODEL_OFFSET] ?? 0);
}

function writeProjectileRecord(out: ServerProjectileState, packet: Uint8Array, offset: number): void {
  out.projectile_id = readU32(packet, offset + contract.STATE_PROJECTILE_RECORD_PROJECTILE_ID_OFFSET);
  out.owner_id = readU32(packet, offset + contract.STATE_PROJECTILE_RECORD_OWNER_ID_OFFSET);
  out.command_id = readU32(packet, offset + contract.STATE_PROJECTILE_RECORD_COMMAND_ID_OFFSET);
  out.x = coordFromWire(readI32(packet, offset + contract.STATE_PROJECTILE_RECORD_X_OFFSET));
  out.y = coordFromWire(readI32(packet, offset + contract.STATE_PROJECTILE_RECORD_Y_OFFSET));
  out.velocity_x = coordFromWire(readI32(packet, offset + contract.STATE_PROJECTILE_RECORD_VELOCITY_X_OFFSET));
  out.velocity_y = coordFromWire(readI32(packet, offset + contract.STATE_PROJECTILE_RECORD_VELOCITY_Y_OFFSET));
  out.spawn_tick = readU32(packet, offset + contract.STATE_PROJECTILE_RECORD_SPAWN_TICK_OFFSET);
  out.fire_view_tick = readU32(packet, offset + contract.STATE_PROJECTILE_RECORD_FIRE_VIEW_TICK_OFFSET);
}

function writeProjectileImpactRecord(
  out: ServerProjectileImpactState,
  packet: Uint8Array,
  offset: number,
): void {
  out.event_id = readU32(packet, offset + contract.STATE_PROJECTILE_IMPACT_RECORD_EVENT_ID_OFFSET);
  out.projectile_id = readU32(packet, offset + contract.STATE_PROJECTILE_IMPACT_RECORD_PROJECTILE_ID_OFFSET);
  out.owner_id = readU32(packet, offset + contract.STATE_PROJECTILE_IMPACT_RECORD_OWNER_ID_OFFSET);
  out.target_id = readU32(packet, offset + contract.STATE_PROJECTILE_IMPACT_RECORD_TARGET_ID_OFFSET);
  out.x = coordFromWire(readI32(packet, offset + contract.STATE_PROJECTILE_IMPACT_RECORD_X_OFFSET));
  out.y = coordFromWire(readI32(packet, offset + contract.STATE_PROJECTILE_IMPACT_RECORD_Y_OFFSET));
  out.reason = projectileImpactReasonFromCode(
    packet[offset + contract.STATE_PROJECTILE_IMPACT_RECORD_REASON_OFFSET] ?? 0,
  );
}
