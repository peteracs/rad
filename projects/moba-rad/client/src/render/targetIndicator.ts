import * as THREE from 'three';

// The move-order reticle: a ring plus a crosshair shown where the player has
// commanded the avatar to go. Pure visual factory with no scene state.
export function createTargetIndicator(): THREE.Group {
  const group = new THREE.Group();
  group.name = 'MoveTargetIndicator';

  const material = new THREE.MeshBasicMaterial({
    color: 0xffffff,
    transparent: true,
    opacity: 0.82,
  });
  const ring = new THREE.Mesh(new THREE.RingGeometry(2.15, 2.45, 48), material);
  ring.name = 'move-target-ring';
  group.add(ring);

  const barH = new THREE.Mesh(new THREE.PlaneGeometry(4.2, 0.16), material);
  barH.name = 'move-target-horizontal';
  group.add(barH);

  const barV = new THREE.Mesh(new THREE.PlaneGeometry(0.16, 4.2), material);
  barV.name = 'move-target-vertical';
  group.add(barV);

  return group;
}
