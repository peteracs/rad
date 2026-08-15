import { SeenIdRing } from '../netcode/seenIdRing.js';
import {
  createAvatarRenderState,
  type AvatarRenderState,
} from '../render/worldView.js';
import type { MobaRadScene, ProjectileImpactVisualState, ProjectileVisualState } from '../scene';
import type {
  ServerAvatarState,
  ServerProjectileImpactState,
  ServerProjectileState,
  ServerState,
} from '../transport/serverState';

const DEFAULT_SEEN_IMPACT_RING_SIZE = 64;

export interface AuthoritySnapshotVisualStats {
  avatarRecordCount: number;
  remoteAvatarCount: number;
  projectileRecordCount: number;
  projectileImpactRecordCount: number;
}

export function createAuthoritySnapshotVisualStats(): AuthoritySnapshotVisualStats {
  return {
    avatarRecordCount: 0,
    remoteAvatarCount: 0,
    projectileRecordCount: 0,
    projectileImpactRecordCount: 0,
  };
}

// Projects accepted authority snapshots into scene-owned visual state. It owns
// roster/projectile scratch and impact dedupe so the app coordinator does not
// grow transport-specific render loops.
export class AuthoritySnapshotProjector {
  private readonly remoteAvatarScratch = createAvatarRenderState();
  private readonly projectileScratch: ProjectileVisualState = {
    projectileId: 0,
    x: 0,
    y: 0,
    velocityX: 0,
    velocityY: 0,
  };
  private readonly projectileImpactScratch: ProjectileImpactVisualState = {
    eventId: 0,
    projectileId: 0,
    x: 0,
    y: 0,
    reason: 'unknown',
  };
  private readonly seenProjectileImpacts: SeenIdRing;

  constructor(
    private readonly scene: MobaRadScene,
    private readonly localPlayerId: number,
    seenImpactRingSize = DEFAULT_SEEN_IMPACT_RING_SIZE,
  ) {
    this.seenProjectileImpacts = new SeenIdRing(seenImpactRingSize);
  }

  apply(
    state: ServerState,
    serverTick: number,
    out: AuthoritySnapshotVisualStats,
  ): AuthoritySnapshotVisualStats {
    out.avatarRecordCount = state.avatars.length;
    out.remoteAvatarCount = this.applyRemoteAvatarSnapshots(state, serverTick);
    out.projectileRecordCount = state.projectiles.length;
    out.projectileImpactRecordCount = state.projectile_impacts.length;

    this.scene.applyLocalAuthorityGhost(state.avatar.x, state.avatar.y);
    this.applyProjectileSnapshots(state);
    this.applyProjectileImpacts(state);
    return out;
  }

  private applyRemoteAvatarSnapshots(state: ServerState, serverTick: number): number {
    let remoteCount = 0;
    this.scene.beginRemoteAvatarSnapshot();
    for (let i = 0; i < state.avatars.length; i += 1) {
      const avatar = state.avatars[i];
      if (avatar.player_id === this.localPlayerId) continue;
      remoteCount += 1;
      writeServerAvatarRenderState(avatar, this.remoteAvatarScratch);
      this.scene.applyRemoteAvatarState(avatar.player_id, this.remoteAvatarScratch, serverTick);
    }
    this.scene.endRemoteAvatarSnapshot();
    return remoteCount;
  }

  private applyProjectileSnapshots(state: ServerState): void {
    this.scene.beginProjectileSnapshot();
    for (let i = 0; i < state.projectiles.length; i += 1) {
      writeServerProjectileVisualState(state.projectiles[i], this.projectileScratch);
      this.scene.applyProjectileState(this.projectileScratch);
    }
    this.scene.endProjectileSnapshot();
  }

  private applyProjectileImpacts(state: ServerState): void {
    for (let i = 0; i < state.projectile_impacts.length; i += 1) {
      const impact = state.projectile_impacts[i];
      if (!this.seenProjectileImpacts.rememberIfNew(impact.event_id)) continue;
      writeServerProjectileImpactVisualState(impact, this.projectileImpactScratch);
      this.scene.spawnProjectileImpact(this.projectileImpactScratch);
    }
  }
}

function writeServerAvatarRenderState(avatar: ServerAvatarState, out: AvatarRenderState): void {
  out.model = avatar.model;
  out.x = avatar.x;
  out.y = avatar.y;
  out.targetX = avatar.target_x;
  out.targetY = avatar.target_y;
  out.targetActive = avatar.target_active;
  out.commandId = avatar.command_id;
}

function writeServerProjectileVisualState(projectile: ServerProjectileState, out: ProjectileVisualState): void {
  out.projectileId = projectile.projectile_id;
  out.x = projectile.x;
  out.y = projectile.y;
  out.velocityX = projectile.velocity_x;
  out.velocityY = projectile.velocity_y;
}

function writeServerProjectileImpactVisualState(
  impact: ServerProjectileImpactState,
  out: ProjectileImpactVisualState,
): void {
  out.eventId = impact.event_id;
  out.projectileId = impact.projectile_id;
  out.x = impact.x;
  out.y = impact.y;
  out.reason = impact.reason;
}
