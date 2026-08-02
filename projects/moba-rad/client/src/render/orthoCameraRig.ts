import * as THREE from 'three';

// Owns the top-down orthographic camera and the math that frames the map plane
// for the current viewport. Keeping this out of the scene lets framing rules
// (margin, zoom clamp, plane fit) evolve without touching scene-graph code.

export interface CameraFraming {
  planeW: number;
  planeH: number;
  cameraZoom: number;
}

// Extra room around the map so the plane is never flush with the viewport edge.
const VIEW_MARGIN = 1.35;
// Floor on zoom so a misconfigured scene can never divide the view to infinity.
const MIN_ZOOM = 0.25;

export class OrthoCameraRig {
  readonly camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);

  constructor() {
    this.camera.position.set(0, 0, 10);
    this.camera.lookAt(0, 0, 0);
  }

  fitToViewport(width: number, height: number, framing: CameraFraming): void {
    const aspect = width / Math.max(1, height);
    const viewH = Math.max(
      framing.planeH * VIEW_MARGIN,
      (framing.planeW / aspect) * VIEW_MARGIN,
    ) / Math.max(MIN_ZOOM, framing.cameraZoom);
    const viewW = viewH * aspect;

    this.camera.left = -viewW / 2;
    this.camera.right = viewW / 2;
    this.camera.top = viewH / 2;
    this.camera.bottom = -viewH / 2;
    this.camera.updateProjectionMatrix();
  }
}
