import type { RadGameSession, RadWorld } from '../src/radHost.js';
import type { AvatarRenderState } from '../src/render/worldView.js';
import type {
  MobaRadScene,
  ProjectileImpactVisualState,
  ProjectileVisualState,
} from '../src/scene.js';
import type { MatchTransport } from '../src/transport/matchTransport.js';
import type { ServerState } from '../src/transport/serverState.js';
import { createServerStateBuffer } from '../src/transport/serverStateBuffer.js';

export interface FakeAvatarSample {
  tick: number;
  state: AvatarRenderState | null;
}

// Records every scene call the app layer makes. Values are copied because the
// production code reuses scratch objects between calls.
export class FakeScene {
  readonly avatarSamples: FakeAvatarSample[] = [];
  readonly ghostPositions: { x: number; y: number }[] = [];
  readonly remoteAvatarSamples: { playerId: number; tick: number; state: AvatarRenderState }[] = [];
  readonly projectileSamples: ProjectileVisualState[] = [];
  readonly impactSpawns: ProjectileImpactVisualState[] = [];
  readonly correctionBlendTimes: number[] = [];
  remoteSnapshotBegins = 0;
  remoteSnapshotEnds = 0;
  projectileSnapshotBegins = 0;
  projectileSnapshotEnds = 0;

  applyAvatarState(state: AvatarRenderState | null, tick: number): void {
    this.avatarSamples.push({ tick, state: state ? { ...state } : null });
  }

  beginLocalCorrectionBlend(nowMs: number): void {
    this.correctionBlendTimes.push(nowMs);
  }

  applyLocalAuthorityGhost(x: number, y: number): void {
    this.ghostPositions.push({ x, y });
  }

  beginRemoteAvatarSnapshot(): void {
    this.remoteSnapshotBegins += 1;
  }

  applyRemoteAvatarState(playerId: number, state: AvatarRenderState, tick: number): void {
    this.remoteAvatarSamples.push({ playerId, tick, state: { ...state } });
  }

  endRemoteAvatarSnapshot(): void {
    this.remoteSnapshotEnds += 1;
  }

  beginProjectileSnapshot(): void {
    this.projectileSnapshotBegins += 1;
  }

  applyProjectileState(state: ProjectileVisualState): void {
    this.projectileSamples.push({ ...state });
  }

  endProjectileSnapshot(): void {
    this.projectileSnapshotEnds += 1;
  }

  spawnProjectileImpact(state: ProjectileImpactVisualState): void {
    this.impactSpawns.push({ ...state });
  }

  asScene(): MobaRadScene {
    return this as unknown as MobaRadScene;
  }
}

// Deterministic stand-in for the wasm-backed RadGameSession: one controlled
// avatar that steps `speedPerTick` toward its move target on every fixed tick,
// exposed through the same RadWorld component layout radHost produces.
export class FakeRadSession {
  tickFixedCalls = 0;
  readonly moveOrders: { playerId: number; commandId: number; targetX: number; targetY: number }[] = [];
  readonly authorityStatesApplied: {
    x: number;
    y: number;
    targetX: number;
    targetY: number;
    targetActive: boolean;
    commandId: number;
  }[] = [];

  private x: number;
  private y: number;
  private targetX: number;
  private targetY: number;
  private targetActive = false;
  private commandId = 0;
  private readonly world: RadWorld;
  private readonly positionFields: Record<string, unknown>;
  private readonly targetFields: Record<string, unknown>;

  constructor(
    private readonly playerId: number,
    startX = 0,
    startY = 0,
    private readonly speedPerTick = 1,
  ) {
    this.x = startX;
    this.y = startY;
    this.targetX = startX;
    this.targetY = startY;
    this.positionFields = { x: startX, y: startY };
    this.targetFields = { x: startX, y: startY, active: false, command_id: 0 };
    this.world = {
      entities: [
        {
          id: 1,
          name: null,
          components: [
            { type: 'Position', fields: this.positionFields },
            { type: 'MoveTarget', fields: this.targetFields },
            { type: 'RenderAvatar', fields: { model: 'clockwork_mage' } },
            { type: 'PlayerControlled', fields: { player_id: playerId } },
          ],
        },
      ],
      resources: {},
    };
  }

  snapshot(): RadWorld {
    return this.syncWorld();
  }

  refresh(): RadWorld {
    return this.syncWorld();
  }

  moveOrder(playerId: number, commandId: number, targetX: number, targetY: number): void {
    this.moveOrders.push({ playerId, commandId, targetX, targetY });
    if (playerId !== this.playerId) return;
    this.targetX = targetX;
    this.targetY = targetY;
    this.targetActive = true;
    this.commandId = commandId;
  }

  tickFixed(): void {
    this.tickFixedCalls += 1;
    if (!this.targetActive) return;

    const dx = this.targetX - this.x;
    const dy = this.targetY - this.y;
    const distance = Math.hypot(dx, dy);
    if (distance <= this.speedPerTick) {
      this.x = this.targetX;
      this.y = this.targetY;
      this.targetActive = false;
      return;
    }
    this.x += (dx / distance) * this.speedPerTick;
    this.y += (dy / distance) * this.speedPerTick;
  }

  applyAuthoritativeState(state: ServerState): void {
    this.authorityStatesApplied.push({
      x: state.avatar.x,
      y: state.avatar.y,
      targetX: state.avatar.target_x,
      targetY: state.avatar.target_y,
      targetActive: state.avatar.target_active,
      commandId: state.avatar.command_id,
    });
    this.x = state.avatar.x;
    this.y = state.avatar.y;
    this.targetX = state.avatar.target_x;
    this.targetY = state.avatar.target_y;
    this.targetActive = state.avatar.target_active;
    this.commandId = state.avatar.command_id;
  }

  asSession(): RadGameSession {
    return this as unknown as RadGameSession;
  }

  private syncWorld(): RadWorld {
    this.positionFields.x = this.x;
    this.positionFields.y = this.y;
    this.targetFields.x = this.targetX;
    this.targetFields.y = this.targetY;
    this.targetFields.active = this.targetActive;
    this.targetFields.command_id = this.commandId;
    return this.world;
  }
}

export class FakeMatchTransport implements MatchTransport {
  readonly moveOrders: {
    clientSeq: number;
    targetTick: number;
    commandId: number;
    targetX: number;
    targetY: number;
  }[] = [];
  readonly casts: {
    clientSeq: number;
    targetTick: number;
    commandId: number;
    dirX: number;
    dirY: number;
    fireViewTick: number;
  }[] = [];
  readonly stateRequests: number[] = [];
  readonly disconnects: number[] = [];
  stateHandler: (clientSeq: number) => Promise<ServerState> = () =>
    Promise.reject(new Error('no state handler installed'));
  sendFailure: Error | null = null;
  latest: ServerState | null = null;
  dropped = 0;
  closed = false;

  sendMoveOrder(
    clientSeq: number,
    targetTick: number,
    commandId: number,
    targetX: number,
    targetY: number,
  ): Promise<void> {
    this.moveOrders.push({ clientSeq, targetTick, commandId, targetX, targetY });
    return this.sendFailure ? Promise.reject(this.sendFailure) : Promise.resolve();
  }

  sendCast(
    clientSeq: number,
    targetTick: number,
    commandId: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
  ): Promise<void> {
    this.casts.push({ clientSeq, targetTick, commandId, dirX, dirY, fireViewTick });
    return this.sendFailure ? Promise.reject(this.sendFailure) : Promise.resolve();
  }

  latestState(): ServerState | null {
    return this.latest;
  }

  droppedStateCount(): number {
    return this.dropped;
  }

  state(clientSeq: number): Promise<ServerState> {
    this.stateRequests.push(clientSeq);
    return this.stateHandler(clientSeq);
  }

  disconnect(clientSeq: number): Promise<void> {
    this.disconnects.push(clientSeq);
    return Promise.resolve();
  }

  close(): void {
    this.closed = true;
  }
}

export interface ServerStateOptions {
  sessionId?: number;
  playerId?: number;
  serverTick?: number;
  serverSeq?: number;
  ackClientSeq?: number;
  ackBits?: number;
  avatar?: Partial<ServerState['avatar']>;
  authority?: Partial<ServerState['authority']>;
}

export function makeServerState(options: ServerStateOptions = {}): ServerState {
  const state = createServerStateBuffer();
  state.ok = true;
  state.status = 'snapshot';
  state.correction_reason = 'none';
  state.session_id = options.sessionId ?? 11;
  state.player_id = options.playerId ?? 7;
  state.server_tick = options.serverTick ?? 0;
  state.server_seq = options.serverSeq ?? 1;
  state.ack_client_seq = options.ackClientSeq ?? 0;
  state.ack_bits = options.ackBits ?? 0;
  state.avatar.player_id = state.player_id;
  Object.assign(state.avatar, options.avatar);
  Object.assign(state.authority, options.authority);
  state.command_id = state.avatar.command_id;
  return state;
}

export function flushAsync(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve));
}
