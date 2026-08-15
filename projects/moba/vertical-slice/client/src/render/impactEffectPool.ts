import * as THREE from 'three';
import { disposeObject3D } from './disposeObject3D';

const IMPACT_Z = 0.42;
const IMPACT_LIFETIME_SECONDS = 0.22;

interface ImpactEffectEntry {
  readonly mesh: THREE.Group;
  readonly ring: THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>;
  readonly flash: THREE.Mesh<THREE.SphereGeometry, THREE.MeshBasicMaterial>;
  active: boolean;
  ageSeconds: number;
  baseScale: number;
}

export class ImpactEffectPool {
  private readonly entries: ImpactEffectEntry[] = [];
  private activeEntries = 0;

  constructor(scene: THREE.Scene, capacity: number) {
    for (let i = 0; i < capacity; i += 1) {
      const effect = createImpactMesh();
      effect.mesh.visible = false;
      scene.add(effect.mesh);
      this.entries.push({
        mesh: effect.mesh,
        ring: effect.ring,
        flash: effect.flash,
        active: false,
        ageSeconds: 0,
        baseScale: 1,
      });
    }
  }

  spawn(x: number, y: number, reason: string): void {
    const entry = this.acquire();
    if (!entry) return;
    configureImpactEntry(entry, reason);
    entry.ageSeconds = 0;
    entry.mesh.position.set(x, y, IMPACT_Z);
    entry.mesh.scale.setScalar(entry.baseScale);
    entry.mesh.visible = true;
  }

  render(dt: number): void {
    const step = Math.max(0, dt);
    for (let i = 0; i < this.entries.length; i += 1) {
      const entry = this.entries[i];
      if (!entry.active) continue;
      entry.ageSeconds += step;
      if (entry.ageSeconds >= IMPACT_LIFETIME_SECONDS) {
        this.release(entry);
        continue;
      }
      const t = entry.ageSeconds / IMPACT_LIFETIME_SECONDS;
      entry.mesh.scale.setScalar(entry.baseScale * (1.0 + t * 1.8));
    }
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
      scene.remove(entry.mesh);
      entry.active = false;
      entry.mesh.visible = false;
      disposeObject3D(entry.mesh);
    }
    this.activeEntries = 0;
    this.entries.length = 0;
  }

  private acquire(): ImpactEffectEntry | null {
    for (let i = 0; i < this.entries.length; i += 1) {
      const entry = this.entries[i];
      if (entry.active) continue;
      entry.active = true;
      this.activeEntries += 1;
      return entry;
    }
    return null;
  }

  private release(entry: ImpactEffectEntry): void {
    if (!entry.active) return;
    entry.active = false;
    entry.mesh.visible = false;
    entry.ageSeconds = 0;
    this.activeEntries -= 1;
  }
}

function createImpactMesh(): Pick<ImpactEffectEntry, 'mesh' | 'ring' | 'flash'> {
  const group = new THREE.Group();
  group.name = 'ProjectileImpact';

  const ring = new THREE.Mesh(
    new THREE.RingGeometry(0.65, 1.0, 18),
    new THREE.MeshBasicMaterial({ color: 0xffffff, side: THREE.DoubleSide }),
  );
  group.add(ring);

  const flash = new THREE.Mesh(
    new THREE.SphereGeometry(0.45, 10, 6),
    new THREE.MeshBasicMaterial({ color: 0x7df9ff }),
  );
  group.add(flash);

  return { mesh: group, ring, flash };
}

function configureImpactEntry(entry: ImpactEffectEntry, reason: string): void {
  switch (reason) {
    case 'hit':
      entry.baseScale = 1.2;
      entry.ring.material.color.set(0xffffff);
      entry.flash.material.color.set(0x7df9ff);
      break;
    case 'range':
      entry.baseScale = 0.95;
      entry.ring.material.color.set(0xffd36b);
      entry.flash.material.color.set(0xff9f43);
      break;
    case 'lifetime':
      entry.baseScale = 0.75;
      entry.ring.material.color.set(0x9aa4b2);
      entry.flash.material.color.set(0x5f6f86);
      break;
    default:
      entry.baseScale = 0.85;
      entry.ring.material.color.set(0xffffff);
      entry.flash.material.color.set(0xb8c4ff);
      break;
  }
}
