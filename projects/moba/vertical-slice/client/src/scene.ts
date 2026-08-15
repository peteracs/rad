import * as THREE from 'three';
import {
  LOCAL_CORRECTION_SMOOTH_MS,
  IMPACT_EFFECT_POOL_SIZE,
  PROJECTILE_LIFETIME_SECONDS,
  PROJECTILE_POOL_SIZE,
  REMOTE_AVATAR_HISTORY_SAMPLES,
  REMOTE_AVATAR_POOL_SIZE,
} from './netcode/constants';
import { AvatarInterpolator } from './render/avatarInterpolator';
import { createHealthBar, writeHealthBarPosition } from './render/avatarOverlays';
import { DEFAULT_AVATAR_MODEL, createAvatarModel } from './render/avatarModels';
import { RemoteAvatarMeshPool, type RemoteAvatarMeshHandle } from './render/avatarMeshPool';
import { AvatarTimeline } from './render/avatarTimeline';
import { aimCastReticle, createCastReticle } from './render/castReticle';
import { CorrectionSmoother } from './render/correctionSmoother';
import { disposeObject3D } from './render/disposeObject3D';
import { createGroundGrid, type GroundGrid } from './render/gridTexture';
import { ImpactEffectPool } from './render/impactEffectPool';
import { OrthoCameraRig } from './render/orthoCameraRig';
import { ProjectileMeshPool, type ProjectileMeshHandle } from './render/projectileMeshPool';
import { createServerGhost, type ServerGhostHandle } from './render/serverGhost';
import { createTargetIndicator } from './render/targetIndicator';
import {
  createAvatarRenderState,
  DEFAULT_SCENE,
  type AvatarRenderState,
  type MobaSceneConfig,
} from './render/worldView';

const AVATAR_Z = 0.2;
const TARGET_Z = 0.12;
const PROJECTILE_Z = 0.34;
const TERRAIN_Z = 0.06;
const HEALTH_BAR_Z = 0.72;
const GHOST_Z = 0.18;

export interface ProjectileVisualState {
  projectileId: number;
  x: number;
  y: number;
  velocityX: number;
  velocityY: number;
}

export interface ProjectileImpactVisualState {
  eventId: number;
  projectileId: number;
  x: number;
  y: number;
  reason: string;
}

export interface MeshPoolDiagnostics {
  remoteAvatarPoolActive: number;
  remoteAvatarPoolIdle: number;
  projectilePoolActive: number;
  projectilePoolIdle: number;
  impactPoolActive: number;
  impactPoolIdle: number;
}

// Owns Three.js presentation only: renderer, scene graph, map plane, camera,
// and visual avatar views. Simulation, rollback, reconciliation, and transport
// state stay outside this class.
export class MobaRadScene {
  private readonly scene = new THREE.Scene();
  private readonly cameraRig = new OrthoCameraRig();
  private readonly raycaster = new THREE.Raycaster();
  private readonly groundPlane = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0);
  private readonly pointerNdc = new THREE.Vector2();
  private readonly hitPoint = new THREE.Vector3();
  private readonly backgroundColor = new THREE.Color(DEFAULT_SCENE.backgroundColor);
  private readonly renderer: THREE.WebGLRenderer;
  private readonly plane: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial>;
  private groundGrid: GroundGrid | null = null;
  private groundGridColor = '';
  private readonly castReticle = createCastReticle();
  private readonly localAvatar = new LocalAvatarView(this.scene);
  private readonly remoteAvatarPool = new RemoteAvatarMeshPool(this.scene, REMOTE_AVATAR_POOL_SIZE);
  private readonly projectilePool = new ProjectileMeshPool(this.scene, PROJECTILE_POOL_SIZE);
  private readonly impactEffectPool = new ImpactEffectPool(this.scene, IMPACT_EFFECT_POOL_SIZE);
  private readonly remoteAvatars = new Map<number, RemoteAvatarView>();
  private readonly projectiles = new Map<number, ProjectileView>();
  private readonly terrainMeshes: THREE.Mesh<THREE.BoxGeometry, THREE.MeshBasicMaterial>[] = [];
  private config = DEFAULT_SCENE;
  private pixelWidth = 0;
  private pixelHeight = 0;
  private disposed = false;
  private remoteSnapshotGeneration = 0;
  private projectileSnapshotGeneration = 0;
  private debugVisualsVisible = false;

  constructor(private readonly canvas: HTMLCanvasElement) {
    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: false,
      powerPreference: 'high-performance',
    });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));

    this.plane = new THREE.Mesh(
      new THREE.PlaneGeometry(1, 1),
      new THREE.MeshBasicMaterial({ color: 0xffffff }),
    );
    this.scene.add(this.plane);
    this.scene.add(this.castReticle.group);
  }

  configure(config: MobaSceneConfig): void {
    this.config = config;
    this.pixelWidth = 0;
    this.pixelHeight = 0;
    this.backgroundColor.set(config.backgroundColor);
    this.scene.background = this.backgroundColor;
    this.configureGroundGrid(config);
    this.plane.scale.set(config.planeW, config.planeH, 1);
    this.syncTerrainMeshes(config);
    this.localAvatar.reset(config);
    for (const avatar of this.remoteAvatars.values()) avatar.reset(config);
  }

  applyAvatarState(state: AvatarRenderState | null, tick: number): void {
    if (!state) return;
    this.localAvatar.pushSample(tick, state, this.config);
  }

  beginLocalCorrectionBlend(nowMs: number): void {
    this.localAvatar.beginCorrectionBlend(nowMs);
  }

  beginRemoteAvatarSnapshot(): void {
    this.remoteSnapshotGeneration += 1;
  }

  applyRemoteAvatarState(playerId: number, state: AvatarRenderState, tick: number): void {
    let avatar = this.remoteAvatars.get(playerId);
    if (!avatar) {
      const handle = this.remoteAvatarPool.acquire(state.model);
      if (!handle) return;
      avatar = new RemoteAvatarView(handle);
      avatar.reset(this.config);
      this.remoteAvatars.set(playerId, avatar);
    }
    avatar.seenGeneration = this.remoteSnapshotGeneration;
    avatar.pushSample(tick, state, this.config);
    avatar.writeAuthorityGhost(state.x, state.y, this.debugVisualsVisible);
  }

  applyLocalAuthorityGhost(x: number, y: number): void {
    this.localAvatar.writeAuthorityGhost(x, y, this.debugVisualsVisible);
  }

  setDebugVisualsVisible(visible: boolean): void {
    this.debugVisualsVisible = visible;
    this.localAvatar.setDebugVisualsVisible(visible);
    for (const avatar of this.remoteAvatars.values()) avatar.setDebugVisualsVisible(visible);
  }

  setAimReticle(startX: number, startY: number, endX: number, endY: number): void {
    if (!aimCastReticle(this.castReticle, startX, startY, endX, endY, 0.24, Number.POSITIVE_INFINITY)) {
      this.hideAimReticle();
    }
  }

  hideAimReticle(): void {
    this.castReticle.group.visible = false;
  }

  endRemoteAvatarSnapshot(): void {
    for (const [playerId, avatar] of this.remoteAvatars) {
      if (avatar.seenGeneration === this.remoteSnapshotGeneration) continue;
      avatar.dispose(this.remoteAvatarPool);
      this.remoteAvatars.delete(playerId);
    }
  }

  spawnPredictedProjectile(state: ProjectileVisualState): void {
    let projectile = this.projectiles.get(state.projectileId);
    if (!projectile) {
      const handle = this.projectilePool.acquire();
      if (!handle) return;
      projectile = new ProjectileView(handle);
      this.projectiles.set(state.projectileId, projectile);
    }
    projectile.writePredicted(state);
  }

  beginProjectileSnapshot(): void {
    this.projectileSnapshotGeneration += 1;
  }

  applyProjectileState(state: ProjectileVisualState): void {
    let projectile = this.projectiles.get(state.projectileId);
    if (!projectile) {
      const handle = this.projectilePool.acquire();
      if (!handle) return;
      projectile = new ProjectileView(handle);
      this.projectiles.set(state.projectileId, projectile);
    }
    projectile.seenGeneration = this.projectileSnapshotGeneration;
    projectile.writeAuthoritative(state);
  }

  endProjectileSnapshot(): void {
    for (const [projectileId, projectile] of this.projectiles) {
      if (!projectile.authoritative || projectile.seenGeneration === this.projectileSnapshotGeneration) {
        continue;
      }
      projectile.dispose(this.projectilePool);
      this.projectiles.delete(projectileId);
    }
  }

  spawnProjectileImpact(state: ProjectileImpactVisualState): void {
    this.impactEffectPool.spawn(state.x, state.y, state.reason);
    const projectile = this.projectiles.get(state.projectileId);
    if (!projectile) return;
    projectile.dispose(this.projectilePool);
    this.projectiles.delete(state.projectileId);
  }

  writeMeshPoolDiagnostics(out: MeshPoolDiagnostics): MeshPoolDiagnostics {
    out.remoteAvatarPoolActive = this.remoteAvatarPool.activeCount();
    out.remoteAvatarPoolIdle = this.remoteAvatarPool.idleCount();
    out.projectilePoolActive = this.projectilePool.activeCount();
    out.projectilePoolIdle = this.projectilePool.idleCount();
    out.impactPoolActive = this.impactEffectPool.activeCount();
    out.impactPoolIdle = this.impactEffectPool.idleCount();
    return out;
  }

  writeWorldPointFromCanvas(clientX: number, clientY: number, out: THREE.Vector2): boolean {
    this.resize();

    const rect = this.canvas.getBoundingClientRect();
    const width = Math.max(1, rect.width);
    const height = Math.max(1, rect.height);
    this.pointerNdc.set(
      ((clientX - rect.left) / width) * 2 - 1,
      -(((clientY - rect.top) / height) * 2 - 1),
    );
    this.raycaster.setFromCamera(this.pointerNdc, this.cameraRig.camera);

    const hit = this.raycaster.ray.intersectPlane(this.groundPlane, this.hitPoint);
    if (!hit) return false;

    const halfW = this.config.planeW / 2;
    const halfH = this.config.planeH / 2;
    if (Math.abs(this.hitPoint.x) > halfW || Math.abs(this.hitPoint.y) > halfH) return false;

    out.set(this.hitPoint.x, this.hitPoint.y);
    return true;
  }

  render(localAlpha = 0, remoteRenderTick = 0, nowMs = performance.now(), projectileDt = 0): void {
    if (this.disposed) return;
    this.resize();
    this.localAvatar.render(localAlpha, nowMs);
    for (const avatar of this.remoteAvatars.values()) {
      avatar.render(remoteRenderTick);
    }
    this.renderProjectiles(projectileDt);
    this.impactEffectPool.render(projectileDt);
    this.renderer.render(this.scene, this.cameraRig.camera);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.scene.remove(this.plane);
    this.scene.remove(this.castReticle.group);
    disposeObject3D(this.plane);
    disposeObject3D(this.castReticle.group);
    this.localAvatar.dispose(this.scene);
    for (const avatar of this.remoteAvatars.values()) avatar.dispose(this.remoteAvatarPool);
    this.remoteAvatars.clear();
    this.remoteAvatarPool.dispose(this.scene);
    for (const projectile of this.projectiles.values()) projectile.dispose(this.projectilePool);
    this.projectiles.clear();
    this.projectilePool.dispose(this.scene);
    this.impactEffectPool.dispose(this.scene);
    for (let i = 0; i < this.terrainMeshes.length; i += 1) {
      this.scene.remove(this.terrainMeshes[i]);
      disposeObject3D(this.terrainMeshes[i]);
    }
    this.terrainMeshes.length = 0;
    this.renderer.dispose();
  }

  private renderProjectiles(dt: number): void {
    for (const [projectileId, projectile] of this.projectiles) {
      projectile.render(dt);
      if (projectile.expired) {
        projectile.dispose(this.projectilePool);
        this.projectiles.delete(projectileId);
      }
    }
  }

  private resize(): void {
    if (this.disposed) return;
    const width = Math.max(1, this.canvas.clientWidth || window.innerWidth);
    const height = Math.max(1, this.canvas.clientHeight || window.innerHeight);
    if (width === this.pixelWidth && height === this.pixelHeight) return;

    this.pixelWidth = width;
    this.pixelHeight = height;
    this.renderer.setSize(width, height, false);
    this.cameraRig.fitToViewport(width, height, this.config);
  }

  private configureGroundGrid(config: MobaSceneConfig): void {
    if (!this.groundGrid || this.groundGridColor !== config.planeColor) {
      if (this.groundGrid) this.groundGrid.texture.dispose();
      this.groundGrid = createGroundGrid(config.planeColor);
      this.groundGridColor = config.planeColor;
      this.plane.material.map = this.groundGrid?.texture ?? null;
      this.plane.material.needsUpdate = true;
    }

    if (this.groundGrid) {
      this.plane.material.color.set(0xffffff);
      this.groundGrid.texture.repeat.set(
        Math.max(1, config.planeW / this.groundGrid.cellWorldUnits),
        Math.max(1, config.planeH / this.groundGrid.cellWorldUnits),
      );
      return;
    }

    this.plane.material.color.set(config.planeColor);
  }

  private syncTerrainMeshes(config: MobaSceneConfig): void {
    while (this.terrainMeshes.length > config.staticColliders.length) {
      const mesh = this.terrainMeshes.pop();
      if (!mesh) return;
      this.scene.remove(mesh);
      disposeObject3D(mesh);
    }

    while (this.terrainMeshes.length < config.staticColliders.length) {
      const mesh = new THREE.Mesh(
        new THREE.BoxGeometry(1, 1, 0.18),
        new THREE.MeshBasicMaterial({ color: 0x2f3540 }),
      );
      this.scene.add(mesh);
      this.terrainMeshes.push(mesh);
    }

    for (let i = 0; i < config.staticColliders.length; i += 1) {
      const collider = config.staticColliders[i];
      const mesh = this.terrainMeshes[i];
      const width = Math.max(0.1, collider.maxX - collider.minX);
      const height = Math.max(0.1, collider.maxY - collider.minY);
      mesh.position.set(collider.minX + width / 2, collider.minY + height / 2, TERRAIN_Z);
      mesh.scale.set(width, height, 1);
      mesh.visible = true;
    }
  }
}

class LocalAvatarView {
  private readonly interpolator = new AvatarInterpolator(
    createAvatarRenderState(),
    createAvatarRenderState(),
  );
  private readonly visualState = createAvatarRenderState();
  private readonly correction = new CorrectionSmoother();
  private readonly targetIndicator = createTargetIndicator();
  private readonly healthBar = createHealthBar();
  private readonly ghost = createServerGhost(6.1);
  private readonly previousAvatarPosition = new THREE.Vector3();
  private avatar = createAvatarModel();
  private avatarModel = DEFAULT_AVATAR_MODEL;

  constructor(scene: THREE.Scene) {
    scene.add(this.avatar);
    scene.add(this.targetIndicator);
    scene.add(this.healthBar);
    scene.add(this.ghost.group);
    this.targetIndicator.visible = false;
    this.healthBar.visible = false;
  }

  reset(config: MobaSceneConfig): void {
    this.interpolator.reset();
    this.avatar.position.set(config.avatarSpawnX, config.avatarSpawnY, AVATAR_Z);
    this.avatar.scale.setScalar(config.avatarScale);
    writeHealthBarPosition(this.healthBar, config.avatarSpawnX, config.avatarSpawnY, HEALTH_BAR_Z);
    this.targetIndicator.visible = false;
    this.ghost.group.visible = false;
  }

  pushSample(tick: number, state: AvatarRenderState, config: MobaSceneConfig): void {
    this.ensureAvatarModel(state.model);
    this.avatar.scale.setScalar(config.avatarScale);
    this.interpolator.pushSample(tick, state);
    this.targetIndicator.position.set(state.targetX, state.targetY, TARGET_Z);
    this.targetIndicator.visible = state.targetActive;
  }

  beginCorrectionBlend(nowMs: number): void {
    this.correction.start(
      this.avatar.position.x,
      this.avatar.position.y,
      nowMs,
      LOCAL_CORRECTION_SMOOTH_MS,
    );
  }

  render(alpha: number, nowMs: number): void {
    if (this.interpolator.writeVisualState(alpha, this.visualState)) {
      this.correction.write(this.visualState.x, this.visualState.y, nowMs, this.visualState);
      this.avatar.position.set(this.visualState.x, this.visualState.y, AVATAR_Z);
      writeHealthBarPosition(this.healthBar, this.visualState.x, this.visualState.y, HEALTH_BAR_Z);
    }
  }

  writeAuthorityGhost(x: number, y: number, visible: boolean): void {
    this.ghost.group.position.set(x, y, GHOST_Z);
    this.ghost.group.visible = visible;
  }

  setDebugVisualsVisible(visible: boolean): void {
    this.ghost.group.visible = visible;
  }

  dispose(scene: THREE.Scene): void {
    scene.remove(this.avatar);
    scene.remove(this.targetIndicator);
    scene.remove(this.healthBar);
    disposeObject3D(this.avatar);
    disposeObject3D(this.targetIndicator);
    disposeObject3D(this.healthBar);
    this.ghost.dispose(scene);
  }

  private ensureAvatarModel(model: string): void {
    if (model === this.avatarModel) return;

    const parent = this.avatar.parent;
    this.previousAvatarPosition.copy(this.avatar.position);
    if (parent) parent.remove(this.avatar);
    disposeObject3D(this.avatar);
    this.avatar = createAvatarModel(model);
    this.avatarModel = model;
    this.avatar.position.copy(this.previousAvatarPosition);
    if (parent) parent.add(this.avatar);
  }
}

class RemoteAvatarView {
  readonly timeline = new AvatarTimeline(createTimelineSamples());
  readonly visualState = createAvatarRenderState();
  seenGeneration = 0;
  private readonly avatar: THREE.Group;
  private readonly targetIndicator: THREE.Group;
  private readonly healthBar: THREE.Group;
  private readonly ghost: ServerGhostHandle;

  constructor(private readonly mesh: RemoteAvatarMeshHandle) {
    this.avatar = mesh.avatar;
    this.targetIndicator = mesh.targetIndicator;
    this.healthBar = mesh.healthBar;
    this.ghost = mesh.ghost;
  }

  reset(config: MobaSceneConfig): void {
    this.timeline.reset();
    this.avatar.position.set(config.avatarSpawnX, config.avatarSpawnY, AVATAR_Z);
    this.avatar.scale.setScalar(config.avatarScale);
    this.avatar.visible = false;
    this.targetIndicator.visible = false;
    this.healthBar.visible = false;
    this.ghost.group.visible = false;
  }

  pushSample(tick: number, state: AvatarRenderState, config: MobaSceneConfig): void {
    this.avatar.scale.setScalar(config.avatarScale);
    this.timeline.pushSample(tick, state);
  }

  render(renderTick: number): void {
    if (!this.timeline.writeVisualStateAt(renderTick, this.visualState)) {
      this.avatar.visible = false;
      this.targetIndicator.visible = false;
      this.healthBar.visible = false;
      return;
    }

    this.avatar.visible = true;
    this.avatar.position.set(this.visualState.x, this.visualState.y, AVATAR_Z);
    writeHealthBarPosition(this.healthBar, this.visualState.x, this.visualState.y, HEALTH_BAR_Z);
    this.targetIndicator.position.set(this.visualState.targetX, this.visualState.targetY, TARGET_Z);
    this.targetIndicator.visible = this.visualState.targetActive;
  }

  writeAuthorityGhost(x: number, y: number, visible: boolean): void {
    this.ghost.group.position.set(x, y, GHOST_Z);
    this.ghost.group.visible = visible;
  }

  setDebugVisualsVisible(visible: boolean): void {
    this.ghost.group.visible = visible;
  }

  dispose(pool: RemoteAvatarMeshPool): void {
    pool.release(this.mesh);
  }
}

class ProjectileView {
  seenGeneration = 0;
  authoritative = false;
  expired = false;
  private ageSeconds = 0;
  private x = 0;
  private y = 0;
  private velocityX = 0;
  private velocityY = 0;
  private readonly mesh: THREE.Group;

  constructor(private readonly handle: ProjectileMeshHandle) {
    this.mesh = handle.mesh;
  }

  writePredicted(state: ProjectileVisualState): void {
    this.authoritative = false;
    this.expired = false;
    this.ageSeconds = 0;
    this.writeState(state);
  }

  writeAuthoritative(state: ProjectileVisualState): void {
    this.authoritative = true;
    this.expired = false;
    this.writeState(state);
  }

  render(dt: number): void {
    if (this.expired) return;
    this.ageSeconds += Math.max(0, dt);
    if (!this.authoritative && this.ageSeconds > PROJECTILE_LIFETIME_SECONDS) {
      this.expired = true;
      return;
    }

    this.x += this.velocityX * dt;
    this.y += this.velocityY * dt;
    this.mesh.position.set(this.x, this.y, PROJECTILE_Z);
  }

  dispose(pool: ProjectileMeshPool): void {
    pool.release(this.handle);
  }

  private writeState(state: ProjectileVisualState): void {
    this.x = state.x;
    this.y = state.y;
    this.velocityX = state.velocityX;
    this.velocityY = state.velocityY;
    this.mesh.position.set(this.x, this.y, PROJECTILE_Z);
    this.mesh.rotation.z = Math.atan2(this.velocityY, this.velocityX);
    this.mesh.visible = true;
  }
}

function createTimelineSamples(): AvatarRenderState[] {
  const samples: AvatarRenderState[] = [];
  for (let i = 0; i < REMOTE_AVATAR_HISTORY_SAMPLES; i += 1) {
    samples.push(createAvatarRenderState());
  }
  return samples;
}
