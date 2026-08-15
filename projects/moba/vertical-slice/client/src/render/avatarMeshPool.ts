import * as THREE from 'three';
import { createHealthBar } from './avatarOverlays';
import { createAvatarModel, DEFAULT_AVATAR_MODEL } from './avatarModels';
import { disposeObject3D } from './disposeObject3D';
import { createServerGhost, type ServerGhostHandle } from './serverGhost';
import { createTargetIndicator } from './targetIndicator';

export interface RemoteAvatarMeshHandle {
  readonly avatar: THREE.Group;
  readonly targetIndicator: THREE.Group;
  readonly healthBar: THREE.Group;
  readonly ghost: ServerGhostHandle;
  readonly model: string;
}

interface PoolEntry extends RemoteAvatarMeshHandle {
  active: boolean;
}

export class RemoteAvatarMeshPool {
  private readonly entries: PoolEntry[] = [];
  private activeEntries = 0;

  constructor(scene: THREE.Scene, capacity: number, model = DEFAULT_AVATAR_MODEL) {
    for (let i = 0; i < capacity; i += 1) {
      const avatar = createAvatarModel(model);
      avatar.visible = false;
      const targetIndicator = createTargetIndicator();
      targetIndicator.visible = false;
      const healthBar = createHealthBar();
      healthBar.visible = false;
      const ghost = createServerGhost(6.1);
      scene.add(avatar);
      scene.add(targetIndicator);
      scene.add(healthBar);
      scene.add(ghost.group);
      this.entries.push({ avatar, targetIndicator, healthBar, ghost, model, active: false });
    }
  }

  acquire(model = DEFAULT_AVATAR_MODEL): RemoteAvatarMeshHandle | null {
    for (let i = 0; i < this.entries.length; i += 1) {
      const entry = this.entries[i];
      if (entry.active || entry.model !== model) continue;
      entry.active = true;
      this.activeEntries += 1;
      entry.avatar.visible = false;
      entry.targetIndicator.visible = false;
      entry.healthBar.visible = false;
      entry.ghost.group.visible = false;
      return entry;
    }
    return null;
  }

  release(handle: RemoteAvatarMeshHandle): void {
    const entry = handle as PoolEntry;
    if (!entry.active) return;
    entry.active = false;
    this.activeEntries -= 1;
    entry.avatar.visible = false;
    entry.targetIndicator.visible = false;
    entry.healthBar.visible = false;
    entry.ghost.group.visible = false;
  }

  activeCount(): number {
    return this.activeEntries;
  }

  idleCount(): number {
    return this.entries.length - this.activeEntries;
  }

  dispose(scene: THREE.Scene): void {
    for (let i = 0; i < this.entries.length; i += 1) {
      const entry = this.entries[i];
      scene.remove(entry.avatar);
      scene.remove(entry.targetIndicator);
      scene.remove(entry.healthBar);
      entry.active = false;
      disposeObject3D(entry.avatar);
      disposeObject3D(entry.targetIndicator);
      disposeObject3D(entry.healthBar);
      entry.ghost.dispose(scene);
    }
    this.activeEntries = 0;
    this.entries.length = 0;
  }
}
