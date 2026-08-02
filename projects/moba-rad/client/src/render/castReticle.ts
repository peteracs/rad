import * as THREE from 'three';

const BEAM_WIDTH = 1.4;

export interface CastReticleHandle {
  group: THREE.Group;
  beam: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial>;
  tip: THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>;
}

export function createCastReticle(): CastReticleHandle {
  const group = new THREE.Group();
  group.name = 'CastReticle';
  group.visible = false;

  const material = new THREE.MeshBasicMaterial({
    color: 0x6fd0ff,
    transparent: true,
    opacity: 0.55,
    depthWrite: false,
  });

  const beam = new THREE.Mesh(new THREE.PlaneGeometry(1, BEAM_WIDTH), material);
  beam.name = 'cast-reticle-beam';
  group.add(beam);

  const tip = new THREE.Mesh(new THREE.RingGeometry(2.6, 3.2, 40), material);
  tip.name = 'cast-reticle-tip';
  group.add(tip);

  return { group, beam, tip };
}

export function aimCastReticle(
  handle: CastReticleHandle,
  originX: number,
  originY: number,
  targetX: number,
  targetY: number,
  z: number,
  maxRange: number,
): boolean {
  const dx = targetX - originX;
  const dy = targetY - originY;
  const dist = Math.hypot(dx, dy);
  if (dist <= 0.0001) return false;

  const length = Math.min(dist, maxRange);
  handle.group.position.set(originX, originY, z);
  handle.group.rotation.z = Math.atan2(dy, dx);
  handle.beam.scale.x = length;
  handle.beam.position.set(length / 2, 0, 0);
  handle.tip.position.set(length, 0, 0);
  handle.group.visible = true;
  return true;
}
