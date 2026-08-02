export interface ServerAvatarState {
  player_id: number;
  model: string;
  x: number;
  y: number;
  target_x: number;
  target_y: number;
  target_active: boolean;
  command_id: number;
}

export interface ServerProjectileState {
  projectile_id: number;
  owner_id: number;
  command_id: number;
  x: number;
  y: number;
  velocity_x: number;
  velocity_y: number;
  spawn_tick: number;
  fire_view_tick: number;
}

export interface ServerProjectileImpactState {
  event_id: number;
  projectile_id: number;
  owner_id: number;
  target_id: number;
  x: number;
  y: number;
  reason: string;
}

export interface ServerPeerState {
  player_id: number;
  session_id: number;
  last_client_seq: number;
  received_client_seq: number;
  last_applied_client_seq: number;
  applied_ack_bits: number;
  pending_move_inputs: number;
  pending_cast_inputs: number;
  connected: boolean;
  late_inputs: number;
  future_inputs: number;
  duplicate_inputs: number;
  overwritten_inputs: number;
}

export interface ServerAuthorityTelemetry {
  peer_count: number;
  max_peers: number;
  input_queue_slots: number;
  pending_move_inputs: number;
  pending_cast_inputs: number;
  peer_connected: boolean;
  late_inputs: number;
  future_inputs: number;
  duplicate_inputs: number;
  overwritten_inputs: number;
  last_client_seq: number;
  // Highest client input/cast sequence the fixed RAD tick has consumed.
  // Paired with applied_ack_bits for selective rollback cleanup.
  last_applied_client_seq: number;
  // 32-bit window of input/cast sequences consumed by the fixed RAD tick.
  applied_ack_bits: number;
}

export interface ServerState {
  ok: boolean;
  status: string;
  correction_reason: string;
  server_ms: number;
  server_tick: number;
  server_seq: number;
  session_id: number;
  player_id: number;
  // Receipt ACK window for packets that reached the RAD peer boundary.
  // Used for resend/loss diagnostics, not rollback-history cleanup.
  ack_client_seq: number;
  ack_bits: number;
  command_id: number;
  avatar: ServerAvatarState;
  peers: ServerPeerState[];
  avatars: ServerAvatarState[];
  projectiles: ServerProjectileState[];
  projectile_impacts: ServerProjectileImpactState[];
  authority: ServerAuthorityTelemetry;
}
