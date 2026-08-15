import * as THREE from 'three';

export function createHealthBar(): THREE.Group {
  const group = new THREE.Group();
  group.name = 'AvatarHealthBar';

  const background = new THREE.Mesh(
    new THREE.PlaneGeometry(10.8, 0.58),
    new THREE.MeshBasicMaterial({
      color: 0x10151d,
      transparent: true,
      opacity: 0.86,
    }),
  );
  background.position.z = 0.01;
  group.add(background);

  const fill = new THREE.Mesh(
    new THREE.PlaneGeometry(10.2, 0.34),
    new THREE.MeshBasicMaterial({ color: 0x46e06f }),
  );
  fill.name = 'avatar-health-fill';
  fill.position.z = 0.02;
  group.add(fill);

  return group;
}

export function writeHealthBarPosition(group: THREE.Group, x: number, y: number, z: number): void {
  group.position.set(x, y + 6.2, z);
  group.visible = true;
}
