import * as THREE from 'three';

export function disposeObject3D(root: THREE.Object3D): void {
  root.traverse((object) => {
    const mesh = object as THREE.Mesh;
    if (mesh.geometry) mesh.geometry.dispose();
    const material = mesh.material;
    if (Array.isArray(material)) {
      for (let i = 0; i < material.length; i += 1) disposeMaterial(material[i]);
    } else if (material) {
      disposeMaterial(material);
    }
  });
}

function disposeMaterial(material: THREE.Material): void {
  const mapped = material as THREE.Material & { map?: THREE.Texture | null };
  if (mapped.map) mapped.map.dispose();
  material.dispose();
}
