import * as THREE from 'three';

export interface ServerGhostHandle {
  group: THREE.Group;
  dispose(scene: THREE.Scene): void;
}

export function createServerGhost(radius: number): ServerGhostHandle {
  const group = new THREE.Group();
  group.name = 'ServerGhost';
  group.visible = false;

  const material = new THREE.LineBasicMaterial({
    color: 0xff5fa8,
    transparent: true,
    opacity: 0.5,
  });

  const ringGeometry = circleGeometry(radius, 28);
  const ring = new THREE.LineLoop(ringGeometry, material);
  ring.name = 'server-ghost-ring';
  group.add(ring);

  const crossGeometry = new THREE.BufferGeometry();
  crossGeometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array([
    -radius, 0, 0, radius, 0, 0,
    0, -radius, 0, 0, radius, 0,
  ]), 3));
  const cross = new THREE.LineSegments(crossGeometry, material);
  cross.name = 'server-ghost-cross';
  group.add(cross);

  return {
    group,
    dispose(scene: THREE.Scene): void {
      scene.remove(group);
      ringGeometry.dispose();
      crossGeometry.dispose();
      material.dispose();
    },
  };
}

function circleGeometry(radius: number, segments: number): THREE.BufferGeometry {
  const points = new Float32Array(segments * 3);
  for (let i = 0; i < segments; i += 1) {
    const angle = (i / segments) * Math.PI * 2;
    points[i * 3] = Math.cos(angle) * radius;
    points[i * 3 + 1] = Math.sin(angle) * radius;
    points[i * 3 + 2] = 0;
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(points, 3));
  return geometry;
}
