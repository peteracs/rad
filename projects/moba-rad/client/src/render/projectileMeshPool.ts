import * as THREE from 'three';
import { disposeObject3D } from './disposeObject3D';

export interface ProjectileMeshHandle {
  readonly mesh: THREE.Group;
}

interface ProjectilePoolEntry extends ProjectileMeshHandle {
  active: boolean;
}

export class ProjectileMeshPool {
  private readonly entries: ProjectilePoolEntry[] = [];
  private activeEntries = 0;

  constructor(scene: THREE.Scene, capacity: number) {
    for (let i = 0; i < capacity; i += 1) {
      const mesh = createProjectileMesh();
      mesh.visible = false;
      scene.add(mesh);
      this.entries.push({ mesh, active: false });
    }
  }

  acquire(): ProjectileMeshHandle | null {
    let entry: ProjectilePoolEntry | null = null;
    for (let i = 0; i < this.entries.length; i += 1) {
      if (this.entries[i].active) continue;
      entry = this.entries[i];
      break;
    }
    if (!entry) return null;

    entry.active = true;
    this.activeEntries += 1;
    entry.mesh.visible = true;
    return entry;
  }

  release(handle: ProjectileMeshHandle): void {
    const entry = handle as ProjectilePoolEntry;
    if (!entry.active) return;
    entry.active = false;
    this.activeEntries -= 1;
    entry.mesh.visible = false;
  }

  activeCount(): number {
    return this.activeEntries;
  }

  idleCount(): number {
    return this.entries.length - this.activeEntries;
  }

  dispose(scene: THREE.Scene): void {
    for (const entry of this.entries) {
      scene.remove(entry.mesh);
      entry.active = false;
      entry.mesh.visible = false;
      disposeObject3D(entry.mesh);
    }
    this.activeEntries = 0;
    this.entries.length = 0;
  }
}

function createProjectileMesh(): THREE.Group {
  const group = new THREE.Group();
  group.name = 'Projectile:linear';

  const core = new THREE.Mesh(
    new THREE.SphereGeometry(0.75, 12, 8),
    new THREE.MeshBasicMaterial({ color: 0x7df9ff }),
  );
  core.scale.set(1.6, 0.7, 0.7);
  group.add(core);

  const trail = new THREE.Mesh(
    new THREE.BoxGeometry(2.2, 0.22, 0.22),
    new THREE.MeshBasicMaterial({ color: 0x2f6fff }),
  );
  trail.position.x = -1.35;
  group.add(trail);

  return group;
}
