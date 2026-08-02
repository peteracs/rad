import * as THREE from 'three';

const CELL_PIXELS = 64;

export interface GroundGrid {
  texture: THREE.CanvasTexture;
  cellWorldUnits: number;
}

export function createGroundGrid(baseColor: string, cellWorldUnits = 8): GroundGrid | null {
  const canvas = document.createElement('canvas');
  canvas.width = CELL_PIXELS;
  canvas.height = CELL_PIXELS;
  const context = canvas.getContext('2d');
  if (!context) return null;

  context.fillStyle = baseColor;
  context.fillRect(0, 0, CELL_PIXELS, CELL_PIXELS);

  context.strokeStyle = 'rgba(150, 170, 205, 0.34)';
  context.lineWidth = 2;
  context.beginPath();
  context.moveTo(0, 1);
  context.lineTo(CELL_PIXELS, 1);
  context.moveTo(1, 0);
  context.lineTo(1, CELL_PIXELS);
  context.stroke();

  context.strokeStyle = 'rgba(120, 140, 175, 0.16)';
  context.lineWidth = 1;
  context.beginPath();
  context.moveTo(0, CELL_PIXELS / 2);
  context.lineTo(CELL_PIXELS, CELL_PIXELS / 2);
  context.moveTo(CELL_PIXELS / 2, 0);
  context.lineTo(CELL_PIXELS / 2, CELL_PIXELS);
  context.stroke();

  const texture = new THREE.CanvasTexture(canvas);
  texture.wrapS = THREE.RepeatWrapping;
  texture.wrapT = THREE.RepeatWrapping;
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  texture.needsUpdate = true;
  return { texture, cellWorldUnits };
}
