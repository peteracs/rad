import type { RadEntity, RadWorld } from '../radHost';
import { DEFAULT_AVATAR_MODEL } from './avatarModelId.js';

// Adapts the RAD world snapshot into render-ready view models. This layer owns
// the mapping from RAD component fields to typed scene/avatar state, so the
// Three.js scene never parses raw snapshots and the RAD component names live in
// exactly one place on the client.

export interface MobaSceneConfig {
  backgroundColor: string;
  planeColor: string;
  planeW: number;
  planeH: number;
  cameraZoom: number;
  avatarSpawnX: number;
  avatarSpawnY: number;
  avatarScale: number;
  staticColliders: StaticTerrainCollider[];
}

export interface StaticTerrainCollider {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export interface AvatarRenderState {
  model: string;
  x: number;
  y: number;
  targetX: number;
  targetY: number;
  targetActive: boolean;
  commandId: number;
}

export const DEFAULT_SCENE: MobaSceneConfig = {
  backgroundColor: '#ffffff',
  planeColor: '#000000',
  planeW: 284.29,
  planeH: 152.57,
  cameraZoom: 1,
  avatarSpawnX: 0,
  avatarSpawnY: 0,
  avatarScale: 5.5,
  staticColliders: [
    { minX: 20, minY: -30, maxX: 24, maxY: 30 },
    { minX: -88, minY: 18, maxX: -36, maxY: 24 },
    { minX: 58, minY: -12, maxX: 78, maxY: 10 },
  ],
};

export function readSceneConfig(resources: Record<string, Record<string, unknown>>): MobaSceneConfig {
  const scene = resources.MobaScene ?? {};
  return {
    backgroundColor: String(scene.background_color ?? DEFAULT_SCENE.backgroundColor),
    planeColor: String(scene.plane_color ?? DEFAULT_SCENE.planeColor),
    planeW: numberFrom(scene.plane_w, DEFAULT_SCENE.planeW),
    planeH: numberFrom(scene.plane_h, DEFAULT_SCENE.planeH),
    cameraZoom: numberFrom(scene.camera_zoom, DEFAULT_SCENE.cameraZoom),
    avatarSpawnX: numberFrom(scene.avatar_spawn_x, DEFAULT_SCENE.avatarSpawnX),
    avatarSpawnY: numberFrom(scene.avatar_spawn_y, DEFAULT_SCENE.avatarSpawnY),
    avatarScale: numberFrom(scene.avatar_scale, DEFAULT_SCENE.avatarScale),
    staticColliders: readStaticColliders(scene),
  };
}

export function createAvatarRenderState(): AvatarRenderState {
  return {
    model: DEFAULT_AVATAR_MODEL,
    x: DEFAULT_SCENE.avatarSpawnX,
    y: DEFAULT_SCENE.avatarSpawnY,
    targetX: DEFAULT_SCENE.avatarSpawnX,
    targetY: DEFAULT_SCENE.avatarSpawnY,
    targetActive: false,
    commandId: 0,
  };
}

export function writeControlledAvatarState(world: RadWorld, playerId: number, out: AvatarRenderState): boolean {
  let avatar: RadEntity | null = null;
  for (let i = 0; i < world.entities.length; i += 1) {
    const entity = world.entities[i];
    if (componentNumber(entity, 'PlayerControlled', 'player_id') === playerId) {
      avatar = entity;
      break;
    }
  }
  if (!avatar) return false;

  const position = componentFields(avatar, 'Position');
  const target = componentFields(avatar, 'MoveTarget');
  const render = componentFields(avatar, 'RenderAvatar');
  if (!position) return false;

  // The shared RAD `RenderAvatar.model` default is empty on purpose; the render
  // client owns the fallback so gameplay code never names a roster character.
  const rawModel = render?.model;
  out.model = typeof rawModel === 'string' && rawModel.length > 0 ? rawModel : DEFAULT_AVATAR_MODEL;
  out.x = numberFrom(position.x, DEFAULT_SCENE.avatarSpawnX);
  out.y = numberFrom(position.y, DEFAULT_SCENE.avatarSpawnY);
  out.targetX = numberFrom(target?.x, DEFAULT_SCENE.avatarSpawnX);
  out.targetY = numberFrom(target?.y, DEFAULT_SCENE.avatarSpawnY);
  out.targetActive = target?.active === true;
  out.commandId = numberFrom(target?.command_id, 0);
  return true;
}

function componentFields(entity: RadEntity, type: string): Record<string, unknown> | null {
  for (let i = 0; i < entity.components.length; i += 1) {
    const component = entity.components[i];
    if (component.type === type) return component.fields;
  }
  return null;
}

function componentNumber(entity: RadEntity, type: string, field: string): number | null {
  const fields = componentFields(entity, type);
  if (!fields) return null;

  const n = Number(fields[field]);
  return Number.isFinite(n) ? n : null;
}

function numberFrom(value: unknown, fallback: number): number {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

function readStaticColliders(scene: Record<string, unknown>): StaticTerrainCollider[] {
  const count = Math.max(0, Math.min(3, Math.trunc(numberFrom(
    scene.static_collider_count,
    DEFAULT_SCENE.staticColliders.length,
  ))));
  const colliders: StaticTerrainCollider[] = [];
  for (let i = 0; i < count; i += 1) {
    const fallback = DEFAULT_SCENE.staticColliders[i] ?? DEFAULT_SCENE.staticColliders[0];
    colliders.push({
      minX: numberFrom(scene[`static_${i}_min_x`], fallback.minX),
      minY: numberFrom(scene[`static_${i}_min_y`], fallback.minY),
      maxX: numberFrom(scene[`static_${i}_max_x`], fallback.maxX),
      maxY: numberFrom(scene[`static_${i}_max_y`], fallback.maxY),
    });
  }
  return colliders;
}
