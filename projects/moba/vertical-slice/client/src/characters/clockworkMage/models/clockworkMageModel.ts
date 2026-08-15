import * as THREE from 'three';

interface ClockworkMageModelOptions {
  ballOffset?: THREE.Vector2;
}

type ClockworkMageMaterials = ReturnType<typeof createMaterials>;

export function createClockworkMageModel(options: ClockworkMageModelOptions = {}): THREE.Group {
  const materials = createMaterials();
  const root = new THREE.Group();
  root.name = 'ClockworkMageModel';

  const body = new THREE.Group();
  body.name = 'ClockworkMageBody';
  root.add(body);

  body.add(circle('base-shadow', 1.42, materials.shadow, 0, -0.08, 0.02, 64));
  body.add(ring('avatar-ring', 1.34, 1.47, materials.ring, 0, -0.08, 0.03));

  body.add(shapeMesh(
    'clockwork-skirt',
    [
      [0, -0.1],
      [-0.72, -0.78],
      [-0.46, -1.14],
      [0, -1.28],
      [0.46, -1.14],
      [0.72, -0.78],
    ],
    materials.dress,
    0.08,
  ));
  body.add(shapeMesh(
    'skirt-trim',
    [
      [-0.46, -1.14],
      [0, -1.28],
      [0.46, -1.14],
      [0.33, -1.04],
      [0, -1.13],
      [-0.33, -1.04],
    ],
    materials.dressTrim,
    0.09,
  ));

  body.add(rect('left-leg', 0.16, 0.42, materials.steelDark, -0.22, -1.24, 0.1, -0.1));
  body.add(rect('right-leg', 0.16, 0.42, materials.steelDark, 0.22, -1.24, 0.1, 0.1));

  body.add(circle('torso-outer', 0.52, materials.brass, 0, -0.06, 0.12, 40, 1.05, 0.84));
  body.add(circle('torso-core', 0.34, materials.steel, 0, -0.03, 0.13, 40, 0.9, 0.8));
  body.add(circle('chest-gem', 0.12, materials.cyan, 0, 0.02, 0.14, 24, 1, 0.8));

  body.add(rect('left-arm', 0.14, 0.72, materials.steel, -0.68, -0.28, 0.11, -0.42));
  body.add(rect('right-arm', 0.14, 0.72, materials.steel, 0.68, -0.28, 0.11, 0.42));
  body.add(circle('left-pauldron', 0.25, materials.brass, -0.52, -0.08, 0.14, 28, 1.08, 0.84));
  body.add(circle('right-pauldron', 0.25, materials.brass, 0.52, -0.08, 0.14, 28, 1.08, 0.84));
  body.add(circle('left-hand', 0.13, materials.porcelain, -0.86, -0.58, 0.14, 20));
  body.add(circle('right-hand', 0.13, materials.porcelain, 0.86, -0.58, 0.14, 20));

  body.add(circle('hair-back', 0.5, materials.cyanDark, 0, 0.56, 0.14, 36, 1.05, 0.86));
  body.add(circle('head', 0.32, materials.porcelain, 0, 0.52, 0.16, 32, 0.92, 1.04));
  body.add(circle('left-hair-puff', 0.21, materials.cyan, -0.28, 0.67, 0.17, 28));
  body.add(circle('right-hair-puff', 0.21, materials.cyan, 0.28, 0.67, 0.17, 28));
  body.add(rect('golden-faceplate', 0.36, 0.08, materials.brass, 0, 0.47, 0.18, 0));
  body.add(circle('left-eye', 0.035, materials.cyan, -0.1, 0.53, 0.19, 12));
  body.add(circle('right-eye', 0.035, materials.cyan, 0.1, 0.53, 0.19, 12));

  const ballOffset = options.ballOffset ?? new THREE.Vector2(1.82, 0.7);
  root.add(linkLine(ballOffset, materials));

  const ball = createClockworkBall(materials);
  ball.position.set(ballOffset.x, ballOffset.y, 0);
  root.add(ball);

  return root;
}

function createMaterials() {
  return {
    brass: new THREE.MeshBasicMaterial({ color: 0xc99a42 }),
    brassDark: new THREE.MeshBasicMaterial({ color: 0x7a5a26 }),
    cyan: new THREE.MeshBasicMaterial({ color: 0x59e2ff }),
    cyanDark: new THREE.MeshBasicMaterial({ color: 0x1a6f82 }),
    porcelain: new THREE.MeshBasicMaterial({ color: 0xe6d6c6 }),
    steel: new THREE.MeshBasicMaterial({ color: 0xb7c0c8 }),
    steelDark: new THREE.MeshBasicMaterial({ color: 0x59646d }),
    dress: new THREE.MeshBasicMaterial({ color: 0x283a74 }),
    dressTrim: new THREE.MeshBasicMaterial({ color: 0x89a9ff }),
    shadow: new THREE.MeshBasicMaterial({ color: 0x10131a }),
    ring: new THREE.MeshBasicMaterial({ color: 0xf3cf78 }),
  };
}

function createClockworkBall(materials: ClockworkMageMaterials): THREE.Group {
  const ball = new THREE.Group();
  ball.name = 'ClockworkMageBall';

  ball.add(circle('ball-shadow', 0.55, materials.shadow, 0, 0, 0.05, 48));
  ball.add(ring('ball-outer-ring', 0.38, 0.5, materials.brass, 0, 0, 0.14));
  ball.add(circle('ball-shell', 0.34, materials.steelDark, 0, 0, 0.15, 40));
  ball.add(circle('ball-core', 0.2, materials.cyanDark, 0, 0, 0.16, 32));
  ball.add(circle('ball-glow', 0.11, materials.cyan, 0, 0, 0.17, 24));

  ball.add(rect('ball-north-fin', 0.12, 0.38, materials.brass, 0, 0.48, 0.13, 0));
  ball.add(rect('ball-east-fin', 0.12, 0.38, materials.brass, 0.48, 0, 0.13, Math.PI / 2));
  ball.add(rect('ball-south-fin', 0.12, 0.38, materials.brass, 0, -0.48, 0.13, 0));
  ball.add(rect('ball-west-fin', 0.12, 0.38, materials.brass, -0.48, 0, 0.13, Math.PI / 2));

  return ball;
}

function circle(
  name: string,
  radius: number,
  material: THREE.Material,
  x: number,
  y: number,
  z: number,
  segments = 32,
  scaleX = 1,
  scaleY = 1,
): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.CircleGeometry(radius, segments), material);
  mesh.name = name;
  mesh.position.set(x, y, z);
  mesh.scale.set(scaleX, scaleY, 1);
  return mesh;
}

function ring(
  name: string,
  innerRadius: number,
  outerRadius: number,
  material: THREE.Material,
  x: number,
  y: number,
  z: number,
): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.RingGeometry(innerRadius, outerRadius, 64), material);
  mesh.name = name;
  mesh.position.set(x, y, z);
  return mesh;
}

function rect(
  name: string,
  width: number,
  height: number,
  material: THREE.Material,
  x: number,
  y: number,
  z: number,
  rotationZ: number,
): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(width, height), material);
  mesh.name = name;
  mesh.position.set(x, y, z);
  mesh.rotation.z = rotationZ;
  return mesh;
}

function shapeMesh(
  name: string,
  points: Array<[number, number]>,
  material: THREE.Material,
  z: number,
): THREE.Mesh {
  const shape = new THREE.Shape();
  const [first, ...rest] = points;
  shape.moveTo(first[0], first[1]);
  for (const [x, y] of rest) shape.lineTo(x, y);
  shape.closePath();

  const mesh = new THREE.Mesh(new THREE.ShapeGeometry(shape), material);
  mesh.name = name;
  mesh.position.z = z;
  return mesh;
}

function linkLine(ballOffset: THREE.Vector2, materials: ClockworkMageMaterials): THREE.Mesh {
  const length = ballOffset.length();
  const mesh = rect('ball-command-line', 0.045, length, materials.brassDark, ballOffset.x / 2, ballOffset.y / 2, 0.06, 0);
  mesh.rotation.z = Math.atan2(ballOffset.y, ballOffset.x) - Math.PI / 2;
  return mesh;
}
