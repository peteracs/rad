import { DEFAULT_AVATAR_MODEL } from '../render/avatarModelId.js';
import type {
  ServerAvatarState,
  ServerPeerState,
  ServerProjectileImpactState,
  ServerProjectileState,
  ServerState,
} from './serverState';

export function createServerStateBuffer(): ServerState {
  return {
    ok: false,
    status: 'unknown',
    correction_reason: 'none',
    server_ms: 0,
    server_tick: 0,
    server_seq: 0,
    session_id: 0,
    player_id: 0,
    ack_client_seq: 0,
    ack_bits: 0,
    command_id: 0,
    avatar: createAvatarState(),
    peers: [],
    avatars: [],
    projectiles: [],
    projectile_impacts: [],
    authority: createAuthorityTelemetry(),
  };
}

export function resizeServerStateRecords(
  out: ServerState,
  peerCount: number,
  avatarCount: number,
  projectileCount: number,
  projectileImpactCount: number,
): void {
  resizeArray(out.peers, peerCount, createPeerState);
  resizeArray(out.avatars, avatarCount, createAvatarState);
  resizeArray(out.projectiles, projectileCount, createProjectileState);
  resizeArray(out.projectile_impacts, projectileImpactCount, createProjectileImpactState);
}

export function copyServerState(source: ServerState, out: ServerState): ServerState {
  out.ok = source.ok;
  out.status = source.status;
  out.correction_reason = source.correction_reason;
  out.server_ms = source.server_ms;
  out.server_tick = source.server_tick;
  out.server_seq = source.server_seq;
  out.session_id = source.session_id;
  out.player_id = source.player_id;
  out.ack_client_seq = source.ack_client_seq;
  out.ack_bits = source.ack_bits;
  out.command_id = source.command_id;
  copyAvatarState(source.avatar, out.avatar);
  copyAuthorityTelemetry(source.authority, out.authority);

  resizeServerStateRecords(
    out,
    source.peers.length,
    source.avatars.length,
    source.projectiles.length,
    source.projectile_impacts.length,
  );
  for (let i = 0; i < source.peers.length; i += 1) copyPeerState(source.peers[i], out.peers[i]);
  for (let i = 0; i < source.avatars.length; i += 1) copyAvatarState(source.avatars[i], out.avatars[i]);
  for (let i = 0; i < source.projectiles.length; i += 1) {
    copyProjectileState(source.projectiles[i], out.projectiles[i]);
  }
  for (let i = 0; i < source.projectile_impacts.length; i += 1) {
    copyProjectileImpactState(source.projectile_impacts[i], out.projectile_impacts[i]);
  }
  return out;
}

function resizeArray<T>(items: T[], count: number, create: () => T): void {
  while (items.length < count) items.push(create());
  if (items.length > count) items.length = count;
}

function createAvatarState(): ServerAvatarState {
  return {
    player_id: 0,
    model: DEFAULT_AVATAR_MODEL,
    x: 0,
    y: 0,
    target_x: 0,
    target_y: 0,
    target_active: false,
    command_id: 0,
  };
}

function createPeerState(): ServerPeerState {
  return {
    player_id: 0,
    session_id: 0,
    last_client_seq: 0,
    received_client_seq: 0,
    last_applied_client_seq: 0,
    applied_ack_bits: 0,
    pending_move_inputs: 0,
    pending_cast_inputs: 0,
    connected: false,
    late_inputs: 0,
    future_inputs: 0,
    duplicate_inputs: 0,
    overwritten_inputs: 0,
  };
}

function createProjectileState(): ServerProjectileState {
  return {
    projectile_id: 0,
    owner_id: 0,
    command_id: 0,
    x: 0,
    y: 0,
    velocity_x: 0,
    velocity_y: 0,
    spawn_tick: 0,
    fire_view_tick: 0,
  };
}

function createProjectileImpactState(): ServerProjectileImpactState {
  return {
    event_id: 0,
    projectile_id: 0,
    owner_id: 0,
    target_id: 0,
    x: 0,
    y: 0,
    reason: 'unknown',
  };
}

function createAuthorityTelemetry(): ServerState['authority'] {
  return {
    peer_count: 0,
    max_peers: 0,
    input_queue_slots: 0,
    pending_move_inputs: 0,
    pending_cast_inputs: 0,
    peer_connected: false,
    late_inputs: 0,
    future_inputs: 0,
    duplicate_inputs: 0,
    overwritten_inputs: 0,
    last_client_seq: 0,
    last_applied_client_seq: 0,
    applied_ack_bits: 0,
  };
}

function copyAvatarState(source: ServerAvatarState, out: ServerAvatarState): void {
  out.player_id = source.player_id;
  out.model = source.model;
  out.x = source.x;
  out.y = source.y;
  out.target_x = source.target_x;
  out.target_y = source.target_y;
  out.target_active = source.target_active;
  out.command_id = source.command_id;
}

function copyPeerState(source: ServerPeerState, out: ServerPeerState): void {
  out.player_id = source.player_id;
  out.session_id = source.session_id;
  out.last_client_seq = source.last_client_seq;
  out.received_client_seq = source.received_client_seq;
  out.last_applied_client_seq = source.last_applied_client_seq;
  out.applied_ack_bits = source.applied_ack_bits;
  out.pending_move_inputs = source.pending_move_inputs;
  out.pending_cast_inputs = source.pending_cast_inputs;
  out.connected = source.connected;
  out.late_inputs = source.late_inputs;
  out.future_inputs = source.future_inputs;
  out.duplicate_inputs = source.duplicate_inputs;
  out.overwritten_inputs = source.overwritten_inputs;
}

function copyProjectileState(source: ServerProjectileState, out: ServerProjectileState): void {
  out.projectile_id = source.projectile_id;
  out.owner_id = source.owner_id;
  out.command_id = source.command_id;
  out.x = source.x;
  out.y = source.y;
  out.velocity_x = source.velocity_x;
  out.velocity_y = source.velocity_y;
  out.spawn_tick = source.spawn_tick;
  out.fire_view_tick = source.fire_view_tick;
}

function copyProjectileImpactState(
  source: ServerProjectileImpactState,
  out: ServerProjectileImpactState,
): void {
  out.event_id = source.event_id;
  out.projectile_id = source.projectile_id;
  out.owner_id = source.owner_id;
  out.target_id = source.target_id;
  out.x = source.x;
  out.y = source.y;
  out.reason = source.reason;
}

function copyAuthorityTelemetry(
  source: ServerState['authority'],
  out: ServerState['authority'],
): void {
  out.peer_count = source.peer_count;
  out.max_peers = source.max_peers;
  out.input_queue_slots = source.input_queue_slots;
  out.pending_move_inputs = source.pending_move_inputs;
  out.pending_cast_inputs = source.pending_cast_inputs;
  out.peer_connected = source.peer_connected;
  out.late_inputs = source.late_inputs;
  out.future_inputs = source.future_inputs;
  out.duplicate_inputs = source.duplicate_inputs;
  out.overwritten_inputs = source.overwritten_inputs;
  out.last_client_seq = source.last_client_seq;
  out.last_applied_client_seq = source.last_applied_client_seq;
  out.applied_ack_bits = source.applied_ack_bits;
}
