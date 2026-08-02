import * as THREE from "https://cdn.jsdelivr.net/npm/three@0.165.0/build/three.module.js";

const MAP4 = {
  cols: 282,
  rows: 155,
  x0: -136210,
  z0: -72080,
  cell: 1000,
  laneTop: [-114020, -2910, -112540, -16900, -110390, -31760, -105280, -45740, -93010, -57540, -77900, -56220, -61560, -42050, -51060, -23300, -42770, -13980, -30490, -6970, -17210, -4210, 3510, -470, 23530, -5190, 37780, -10990, 52480, -22660, 61470, -35510, 66110, -46020, 78660, -57760, 94280, -57450, 106910, -45640, 109710, -35090, 109780, -22970, 105100, -8150],
  laneBot: [-114720, 18550, -110490, 27160, -110030, 41390, -105040, 57580, -86500, 65220, -55510, 66560, -41360, 66430, -26210, 66880, -1100, 65310, 22150, 65950, 53240, 66230, 76020, 65960, 93360, 66410, 112150, 51150, 108700, 32690, 107400, 18380],
  turrets: [1, -103160, 7500, 1, -111420, -38670, 1, -112300, 54230, 1, -55280, -29210, 1, -67920, 66420, 2, 100810, 8150, 2, 112440, -37530, 2, 57040, -30800, 2, 112910, 53100, 2, 67380, 65910],
  nexus: [1, -91490, 7690, 2, 89790, 7680],
};

const DEFAULT_CONFIG = {
  worldW: MAP4.cols * MAP4.cell,
  worldH: MAP4.rows * MAP4.cell,
  worldX0: MAP4.x0,
  worldY0: MAP4.z0,
  worldX1: MAP4.x0 + MAP4.cols * MAP4.cell,
  worldY1: MAP4.z0 + MAP4.rows * MAP4.cell,
  mapName: "browser-moba/map4",
  mapCols: MAP4.cols,
  mapRows: MAP4.rows,
  mapX0: MAP4.x0,
  mapZ0: MAP4.z0,
  mapCell: MAP4.cell,
  cameraPitchDeg: 57,
  cameraYawDeg: 0,
  cameraZoom: 1.05,
};

function num(v, fallback = 0) {
  return Number.isFinite(Number(v)) ? Number(v) : fallback;
}

function component(ent, type) {
  return ent?.components?.find(c => c.type === type)?.fields || null;
}

function entitiesWith(world, ...types) {
  return [...world.entities.values()].filter(ent => types.every(t => component(ent, t)));
}

function byName(world, name) {
  return [...world.entities.values()].find(ent => ent.name === name) || null;
}

function posOf(ent) {
  const p = component(ent, "Position");
  return p ? { x: num(p.x), y: num(p.y) } : { x: 0, y: 0 };
}

function healthPct(ent) {
  const h = component(ent, "Health");
  if (!h) return 1;
  return Math.max(0, Math.min(1, num(h.hp) / Math.max(1, num(h.max))));
}

function shieldPct(ent) {
  const h = component(ent, "Health");
  if (!h) return 0;
  return Math.max(0, Math.min(1, num(h.shield) / 400));
}

function readConfig(raw = {}) {
  return {
    worldW: num(raw.world_w, DEFAULT_CONFIG.worldW),
    worldH: num(raw.world_h, DEFAULT_CONFIG.worldH),
    worldX0: num(raw.world_x0, DEFAULT_CONFIG.worldX0),
    worldY0: num(raw.world_y0, DEFAULT_CONFIG.worldY0),
    worldX1: num(raw.world_x1, DEFAULT_CONFIG.worldX1),
    worldY1: num(raw.world_y1, DEFAULT_CONFIG.worldY1),
    mapName: raw.map_name || DEFAULT_CONFIG.mapName,
    mapCols: num(raw.map_cols, DEFAULT_CONFIG.mapCols),
    mapRows: num(raw.map_rows, DEFAULT_CONFIG.mapRows),
    mapX0: num(raw.map_x0, DEFAULT_CONFIG.mapX0),
    mapZ0: num(raw.map_z0, DEFAULT_CONFIG.mapZ0),
    mapCell: num(raw.map_cell, DEFAULT_CONFIG.mapCell),
    cameraPitchDeg: num(raw.camera_pitch_deg, DEFAULT_CONFIG.cameraPitchDeg),
    cameraYawDeg: num(raw.camera_yaw_deg, DEFAULT_CONFIG.cameraYawDeg),
    cameraZoom: num(raw.camera_zoom, DEFAULT_CONFIG.cameraZoom),
  };
}

function disposeObject(obj) {
  if (obj.geometry) obj.geometry.dispose();
  if (obj.material) {
    const materials = Array.isArray(obj.material) ? obj.material : [obj.material];
    for (const mat of materials) {
      if (mat.map) mat.map.dispose();
      mat.dispose();
    }
  }
}

function clearGroup(group) {
  while (group.children.length) {
    const child = group.children.pop();
    child.traverse(disposeObject);
  }
}

function makeCanvasTexture(width, height, draw) {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  draw(ctx, width, height);
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  return texture;
}

function pathPairs(flat) {
  const pairs = [];
  for (let i = 0; i + 1 < flat.length; i += 2) pairs.push([flat[i], flat[i + 1]]);
  return pairs;
}

function drawBrowserMobaMap(ctx, cfg, width, height) {
  const mapW = cfg.mapCols * cfg.mapCell;
  const mapH = cfg.mapRows * cfg.mapCell;
  const project = (x, z) => [
    ((x - cfg.mapX0) / mapW) * width,
    ((z - cfg.mapZ0) / mapH) * height,
  ];

  const base = ctx.createLinearGradient(0, 0, width, height);
  base.addColorStop(0, "#172c2a");
  base.addColorStop(0.42, "#203228");
  base.addColorStop(1, "#18212b");
  ctx.fillStyle = base;
  ctx.fillRect(0, 0, width, height);

  ctx.globalAlpha = 0.24;
  for (let y = 0; y < height; y += 34) {
    for (let x = (y / 34) % 2 ? 17 : 0; x < width; x += 34) {
      ctx.fillStyle = (x + y) % 3 ? "#244039" : "#283b31";
      ctx.fillRect(x, y, 18, 18);
    }
  }
  ctx.globalAlpha = 1;

  function strokeLane(points, color, glow) {
    const lane = pathPairs(points).map(([x, z]) => project(x, z));
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = glow;
    ctx.lineWidth = 72;
    ctx.beginPath();
    lane.forEach(([x, y], i) => i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y));
    ctx.stroke();
    ctx.strokeStyle = color;
    ctx.lineWidth = 42;
    ctx.beginPath();
    lane.forEach(([x, y], i) => i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y));
    ctx.stroke();
    ctx.strokeStyle = "rgba(238, 220, 162, .38)";
    ctx.lineWidth = 3;
    ctx.beginPath();
    lane.forEach(([x, y], i) => i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y));
    ctx.stroke();
  }

  strokeLane(MAP4.laneTop, "rgba(84, 91, 77, .86)", "rgba(201, 177, 105, .14)");
  strokeLane(MAP4.laneBot, "rgba(84, 91, 77, .80)", "rgba(70, 184, 180, .12)");

  ctx.strokeStyle = "rgba(86, 166, 255, .15)";
  ctx.lineWidth = 44;
  ctx.beginPath();
  ctx.moveTo(width * 0.08, height * 0.53);
  ctx.bezierCurveTo(width * 0.30, height * 0.43, width * 0.46, height * 0.50, width * 0.60, height * 0.39);
  ctx.bezierCurveTo(width * 0.70, height * 0.30, width * 0.82, height * 0.31, width * 0.94, height * 0.18);
  ctx.stroke();

  ctx.lineWidth = 1;
  ctx.strokeStyle = "rgba(210, 227, 226, .055)";
  for (let x = 0; x <= width; x += width / 10) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, height);
    ctx.stroke();
  }
  for (let y = 0; y <= height; y += height / 7) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }

  for (let i = 0; i + 2 < MAP4.turrets.length; i += 3) {
    const team = MAP4.turrets[i];
    const [x, y] = project(MAP4.turrets[i + 1], MAP4.turrets[i + 2]);
    ctx.fillStyle = team === 1 ? "rgba(88, 216, 200, .75)" : "rgba(255, 91, 105, .75)";
    ctx.strokeStyle = "rgba(255, 245, 205, .55)";
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 8, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }

  for (let i = 0; i + 2 < MAP4.nexus.length; i += 3) {
    const team = MAP4.nexus[i];
    const [x, y] = project(MAP4.nexus[i + 1], MAP4.nexus[i + 2]);
    ctx.fillStyle = team === 1 ? "rgba(88, 216, 200, .88)" : "rgba(255, 91, 105, .88)";
    ctx.strokeStyle = "rgba(255, 245, 205, .72)";
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.rect(x - 12, y - 12, 24, 24);
    ctx.fill();
    ctx.stroke();
  }

  ctx.strokeStyle = "rgba(255, 242, 190, .26)";
  ctx.lineWidth = 6;
  ctx.strokeRect(3, 3, width - 6, height - 6);
}

function makeLabelSprite(text, hp, shield, color) {
  const texture = makeCanvasTexture(256, 80, (ctx, width, height) => {
    ctx.clearRect(0, 0, width, height);
    ctx.font = "700 24px Segoe UI, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.lineWidth = 5;
    ctx.strokeStyle = "rgba(2, 4, 6, .9)";
    ctx.strokeText(text, width / 2, 2);
    ctx.fillStyle = "#f1f6f8";
    ctx.fillText(text, width / 2, 2);
    ctx.fillStyle = "rgba(3, 5, 8, .86)";
    ctx.fillRect(38, 42, 180, 11);
    ctx.fillStyle = color;
    ctx.fillRect(40, 44, 176 * hp, 7);
    if (shield > 0) {
      ctx.fillStyle = "#58d8c8";
      ctx.fillRect(40, 58, 176 * shield, 5);
    }
  });
  const mat = new THREE.SpriteMaterial({ map: texture, transparent: true, depthTest: false });
  const sprite = new THREE.Sprite(mat);
  sprite.scale.set(112, 35, 1);
  return sprite;
}

export class MobaArena3D {
  constructor(canvas) {
    this.canvas = canvas;
    this.scene = new THREE.Scene();
    this.scene.background = new THREE.Color(0x07090b);
    this.camera = new THREE.OrthographicCamera(-500, 500, 350, -350, 1, 1000000);
    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: false,
      preserveDrawingBuffer: false,
      powerPreference: "high-performance",
    });
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.maxPixelRatio = 1.25;
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, this.maxPixelRatio));
    this.renderer.shadowMap.enabled = false;

    this.raycaster = new THREE.Raycaster();
    this.pickPlane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
    this.config = { ...DEFAULT_CONFIG };
    this.staticKey = "";
    this.sizeKey = "";
    this.staticGroup = new THREE.Group();
    this.effectGroup = new THREE.Group();
    this.unitGroup = new THREE.Group();
    this.dynamicGroup = this.effectGroup;
    this.unitViews = new Map();
    this.scene.add(this.staticGroup, this.effectGroup, this.unitGroup);
    this.buildStatic(this.config);
  }

  worldToThree(p, y = 0) {
    return new THREE.Vector3(
      p.x - (this.config.worldX0 + this.config.worldW / 2),
      y,
      p.y - (this.config.worldY0 + this.config.worldH / 2)
    );
  }

  configure(rawConfig = {}) {
    const next = readConfig(rawConfig);
    const key = JSON.stringify(next);
    this.config = next;
    this.resize();
    if (key !== this.staticKey) {
      this.staticKey = key;
      this.buildStatic(next);
      this.updateCamera();
    }
  }

  resize() {
    const width = Math.max(1, this.canvas.clientWidth || this.canvas.width);
    const height = Math.max(1, this.canvas.clientHeight || this.canvas.height);
    const dpr = Math.min(window.devicePixelRatio || 1, this.maxPixelRatio);
    this.renderer.setPixelRatio(dpr);
    const nextSizeKey = `${Math.round(width)}:${Math.round(height)}:${dpr}`;
    if (nextSizeKey !== this.sizeKey) {
      this.sizeKey = nextSizeKey;
      this.renderer.setSize(width, height, false);
      this.updateCamera();
    }
  }

  updateCamera() {
    const width = Math.max(1, this.canvas.clientWidth || this.canvas.width);
    const height = Math.max(1, this.canvas.clientHeight || this.canvas.height);
    const aspect = width / height;
    const fitPadding = 1.12;
    const viewH = Math.max(this.config.worldH * fitPadding, (this.config.worldW / aspect) * fitPadding) / Math.max(0.35, this.config.cameraZoom);
    const viewW = viewH * aspect;
    this.camera.left = -viewW / 2;
    this.camera.right = viewW / 2;
    this.camera.top = viewH / 2;
    this.camera.bottom = -viewH / 2;

    const pitch = THREE.MathUtils.degToRad(this.config.cameraPitchDeg);
    const yaw = THREE.MathUtils.degToRad(this.config.cameraYawDeg);
    const distance = Math.max(this.config.worldW, this.config.worldH) * 1.28;
    const y = Math.sin(pitch) * distance;
    const horizontal = Math.cos(pitch) * distance;
    this.camera.position.set(Math.sin(yaw) * horizontal, y, Math.cos(yaw) * horizontal);
    this.camera.far = distance * 2.5;
    this.camera.lookAt(0, 0, 0);
    this.camera.updateProjectionMatrix();
  }

  worldUnitsPerPixel() {
    const width = Math.max(1, this.canvas.clientWidth || this.canvas.width);
    return Math.max(1, (this.camera.right - this.camera.left) / width);
  }

  unitVisualScale() {
    return Math.max(1, Math.min(140, this.worldUnitsPerPixel() * 0.38));
  }

  minWorldSize(pixels) {
    return this.worldUnitsPerPixel() * pixels;
  }

  buildStatic(cfg) {
    clearGroup(this.staticGroup);

    const groundTexture = makeCanvasTexture(1536, 1024, (ctx, width, height) => drawBrowserMobaMap(ctx, cfg, width, height));
    const ground = new THREE.Mesh(
      new THREE.PlaneGeometry(cfg.worldW, cfg.worldH, 1, 1),
      new THREE.MeshStandardMaterial({ map: groundTexture, roughness: 0.92, metalness: 0.02 })
    );
    ground.rotation.x = -Math.PI / 2;
    ground.receiveShadow = true;
    this.staticGroup.add(ground);

    const border = new THREE.Mesh(
      new THREE.RingGeometry(Math.min(cfg.worldW, cfg.worldH) * 0.49, Math.min(cfg.worldW, cfg.worldH) * 0.495, 96),
      new THREE.MeshBasicMaterial({ color: 0xd8b45f, transparent: true, opacity: 0.0 })
    );
    border.rotation.x = -Math.PI / 2;
    this.staticGroup.add(border);

    const ambient = new THREE.HemisphereLight(0xddefff, 0x16211b, 1.15);
    this.staticGroup.add(ambient);

    const sun = new THREE.DirectionalLight(0xfff1c9, 2.25);
    sun.position.set(-260, 760, 380);
    sun.castShadow = true;
    sun.shadow.mapSize.set(1024, 1024);
    sun.shadow.camera.left = -700;
    sun.shadow.camera.right = 700;
    sun.shadow.camera.top = 700;
    sun.shadow.camera.bottom = -700;
    this.staticGroup.add(sun);
  }

  visualPosOf(ent) {
    const p = posOf(ent);
    const view = this.unitViews.get(ent?.name);
    return view?.displayPos || p;
  }

  pickPoint(ev, rawConfig = {}) {
    this.configure(rawConfig);
    const rect = this.canvas.getBoundingClientRect();
    const clientX = ev.clientX ?? ev.touches?.[0]?.clientX ?? 0;
    const clientY = ev.clientY ?? ev.touches?.[0]?.clientY ?? 0;
    const ndc = new THREE.Vector2(
      ((clientX - rect.left) / rect.width) * 2 - 1,
      -(((clientY - rect.top) / rect.height) * 2 - 1)
    );
    this.raycaster.setFromCamera(ndc, this.camera);
    const hit = new THREE.Vector3();
    this.raycaster.ray.intersectPlane(this.pickPlane, hit);
    const rawX = hit.x + this.config.worldX0 + this.config.worldW / 2;
    const rawY = hit.z + this.config.worldY0 + this.config.worldH / 2;
    return {
      x: Math.max(this.config.worldX0, Math.min(this.config.worldX1, rawX)),
      y: Math.max(this.config.worldY0, Math.min(this.config.worldY1, rawY)),
    };
  }

  addGroundRing(center, radius, color, opacity = 0.7, width = 3) {
    const visibleRadius = Math.max(radius, this.minWorldSize(5));
    const visibleWidth = Math.max(width, this.minWorldSize(1.5));
    const ring = new THREE.Mesh(
      new THREE.RingGeometry(Math.max(1, visibleRadius - visibleWidth), visibleRadius + visibleWidth, 128),
      new THREE.MeshBasicMaterial({ color, transparent: true, opacity, side: THREE.DoubleSide })
    );
    ring.position.copy(this.worldToThree(center, 2.2));
    ring.rotation.x = -Math.PI / 2;
    this.dynamicGroup.add(ring);
    return ring;
  }

  addDisk(center, radius, color, opacity = 0.22) {
    const visibleRadius = Math.max(radius, this.minWorldSize(5));
    const disk = new THREE.Mesh(
      new THREE.CircleGeometry(visibleRadius, 96),
      new THREE.MeshBasicMaterial({ color, transparent: true, opacity, side: THREE.DoubleSide, depthWrite: false })
    );
    disk.position.copy(this.worldToThree(center, 1.8));
    disk.rotation.x = -Math.PI / 2;
    this.dynamicGroup.add(disk);
    return disk;
  }

  addBeam(a, b, color, radius = 3, y = 9, opacity = 0.82) {
    const start = this.worldToThree(a, y);
    const end = this.worldToThree(b, y);
    const mid = start.clone().add(end).multiplyScalar(0.5);
    const dir = end.clone().sub(start);
    const len = dir.length();
    if (len < 0.01) return null;
    const visibleRadius = Math.max(radius, this.minWorldSize(1.1));
    const mesh = new THREE.Mesh(
      new THREE.CylinderGeometry(visibleRadius, visibleRadius, len, 12),
      new THREE.MeshBasicMaterial({ color, transparent: true, opacity })
    );
    mesh.position.copy(mid);
    mesh.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir.normalize());
    this.dynamicGroup.add(mesh);
    return mesh;
  }

  addZone(ent) {
    const z = component(ent, "Zone");
    const center = { x: num(z.x), y: num(z.y) };
    const life = Math.max(0.18, Math.min(0.65, num(z.ticks_left) / 35));
    this.addDisk(center, num(z.radius), 0x58d8c8, life * 0.28);
    this.addGroundRing(center, num(z.radius), 0x58d8c8, 0.5, 2.5);
  }

  addTravel(world) {
    const ball = byName(world, "orianna_ball");
    const bt = component(ball, "BallTravel");
    if (!ball || !bt) return;
    const p = this.visualPosOf(ball);
    const target = { x: num(bt.target_x), y: num(bt.target_y) };
    this.addBeam(p, target, 0xd8b45f, 3.2, 13, 0.82);
    this.addGroundRing(target, num(bt.finish_radius, 25), 0xd8b45f, 0.55, 2);
  }

  addMoveTarget(world) {
    const ori = byName(world, "orianna");
    const mt = component(ori, "MoveTarget");
    if (!ori || !mt) return;
    const p = this.visualPosOf(ori);
    const target = { x: num(mt.x), y: num(mt.y) };
    this.addBeam(p, target, 0x5aa7ff, 2, 7, 0.55);
    this.addGroundRing(target, 12, 0x5aa7ff, 0.85, 2.5);
  }

  addAim(world, ui) {
    const ori = byName(world, "orianna");
    const ball = byName(world, "orianna_ball");
    if (!ori || !ball) return;
    const op = this.visualPosOf(ori);
    const bp = this.visualPosOf(ball);
    if (ui.mode === "q") this.addGroundRing(op, 885, 0xd8b45f, 0.4, 2.5);
    if (ui.mode === "e") this.addGroundRing(op, 1020, 0x58d8c8, 0.36, 2.5);
    if (ui.mode === "move" && ui.hover) this.addGroundRing(ui.hover, 14, 0xffffff, 0.38, 1.6);
    this.addBeam(op, bp, 0xd8b45f, 1.4, 6, 0.35);
  }

  createUnitView(ent) {
    const u = component(ent, "Unit");
    const isOri = u.id === "orianna";
    const isBall = u.kind === "ball";
    const isAlly = u.team === "blue";
    const group = new THREE.Group();
    const view = { group, kind: u.kind, isOri, isBall, isAlly, label: null, labelKey: "" };

    if (isBall) {
      const ballMat = new THREE.MeshStandardMaterial({ color: 0xd8b45f, emissive: 0x8f641c, emissiveIntensity: 0.9, roughness: 0.35, metalness: 0.52 });
      const ball = new THREE.Mesh(new THREE.SphereGeometry(14, 32, 18), ballMat);
      ball.position.y = 22;
      group.add(ball);
      const glow = new THREE.PointLight(0xd8b45f, 1.8, 180);
      glow.position.y = 26;
      group.add(glow);
      const ring = new THREE.Mesh(
        new THREE.TorusGeometry(24, 2, 8, 48),
        new THREE.MeshBasicMaterial({ color: 0xd8b45f, transparent: true, opacity: 0.58 })
      );
      ring.rotation.x = Math.PI / 2;
      ring.position.y = 2.2;
      group.add(ring);
      view.ball = ball;
      this.unitGroup.add(group);
      this.unitViews.set(ent.name, view);
      return view;
    }

    const radius = isOri ? 24 : 20;
    const bodyColor = isOri ? 0x5aa7ff : isAlly ? 0x58d8c8 : 0xff5b69;
    const trimColor = isOri ? 0xd8b45f : isAlly ? 0xd9fff8 : 0xffd5db;

    const baseRingMat = new THREE.MeshBasicMaterial({ color: trimColor, transparent: true, opacity: 1 });
    const baseRing = new THREE.Mesh(
      new THREE.TorusGeometry(radius + 4, isOri ? 2.3 : 1.8, 10, 48),
      baseRingMat
    );
    baseRing.rotation.x = Math.PI / 2;
    baseRing.position.y = 2;
    group.add(baseRing);

    const bodyMat = new THREE.MeshStandardMaterial({ color: bodyColor, roughness: 0.46, metalness: isOri ? 0.28 : 0.08, transparent: true, opacity: 1 });
    const body = new THREE.Mesh(
      new THREE.CylinderGeometry(radius * 0.48, radius * 0.68, isOri ? 46 : 38, 28),
      bodyMat
    );
    body.position.y = isOri ? 27 : 23;
    group.add(body);

    const headMat = new THREE.MeshStandardMaterial({ color: isOri ? 0xc8d9ff : trimColor, roughness: 0.32, metalness: isOri ? 0.2 : 0.04, transparent: true, opacity: 1 });
    const head = new THREE.Mesh(
      new THREE.SphereGeometry(radius * 0.42, 24, 16),
      headMat
    );
    head.position.y = isOri ? 56 : 47;
    group.add(head);

    if (isOri) {
      const halo = new THREE.Mesh(
        new THREE.TorusGeometry(radius * 0.82, 1.7, 8, 36),
        new THREE.MeshBasicMaterial({ color: 0xd8b45f, transparent: true, opacity: 0.78 })
      );
      halo.position.y = 65;
      halo.rotation.x = Math.PI / 2.2;
      group.add(halo);
      view.halo = halo;
    }

    const forcedRing = new THREE.Mesh(
      new THREE.TorusGeometry(radius + 14, 3.5, 8, 64),
      new THREE.MeshBasicMaterial({ color: 0xd8b45f, transparent: true, opacity: 0.86 })
    );
    forcedRing.rotation.x = Math.PI / 2;
    forcedRing.position.y = 2.6;
    forcedRing.visible = false;
    group.add(forcedRing);

    const inspectRing = new THREE.Mesh(
      new THREE.TorusGeometry(radius + 20, 3.2, 8, 64),
      new THREE.MeshBasicMaterial({ color: 0xfff1b8, transparent: true, opacity: 0.92 })
    );
    inspectRing.rotation.x = Math.PI / 2;
    inspectRing.position.y = 3.2;
    inspectRing.visible = false;
    group.add(inspectRing);

    view.baseRingMat = baseRingMat;
    view.bodyMat = bodyMat;
    view.headMat = headMat;
    view.forcedRing = forcedRing;
    view.inspectRing = inspectRing;
    this.unitGroup.add(group);
    this.unitViews.set(ent.name, view);
    return view;
  }

  updateLabel(view, text, hp, shield, color, y) {
    const key = `${text}:${Math.round(hp * 100)}:${Math.round(shield * 100)}:${color}`;
    if (view.labelKey === key) return;
    const old = view.label;
    const label = makeLabelSprite(text, hp, shield, color);
    label.position.set(0, y, 0);
    view.group.add(label);
    view.label = label;
    view.labelKey = key;
    if (old) {
      view.group.remove(old);
      old.traverse(disposeObject);
    }
  }

  updateUnitView(ent, ui) {
    const u = component(ent, "Unit");
    let view = this.unitViews.get(ent.name);
    if (!view || view.kind !== u.kind) {
      if (view) {
        this.unitGroup.remove(view.group);
        view.group.traverse(disposeObject);
        this.unitViews.delete(ent.name);
      }
      view = this.createUnitView(ent);
    }

    const p = posOf(ent);
    const alive = u.alive !== false;
    const dtMs = Math.max(0, Math.min(80, num(ui.frameDtMs, 1000 / 60)));
    if (!view.displayPos) {
      view.displayPos = { x: p.x, y: p.y };
    } else {
      const dx = p.x - view.displayPos.x;
      const dy = p.y - view.displayPos.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist > 480 || dtMs <= 0) {
        view.displayPos.x = p.x;
        view.displayPos.y = p.y;
      } else {
        const alpha = 1 - Math.exp(-dtMs / 45);
        view.displayPos.x += dx * alpha;
        view.displayPos.y += dy * alpha;
      }
    }
    view.group.position.copy(this.worldToThree(view.displayPos, 0));
    view.group.scale.setScalar(this.unitVisualScale());
    view.group.visible = alive || view.isBall;

    if (view.isBall) {
      view.ball.position.y = 22 + Math.sin(performance.now() / 260) * 3;
      return;
    }

    const opacity = alive ? 1 : 0.38;
    view.baseRingMat.opacity = opacity;
    view.bodyMat.opacity = opacity;
    view.headMat.opacity = opacity;
    view.forcedRing.visible = !!component(ent, "ForcedMove");
    view.inspectRing.visible = ent.name === ui.inspectedName;

    const label = view.isOri ? "Orianna" : String(u.id || ent.name).replace("_", " ");
    this.updateLabel(view, label, healthPct(ent), shieldPct(ent), view.isAlly ? "#6eee9b" : "#ff5b69", view.isOri ? 96 : 82);
  }

  pruneUnitViews(liveNames) {
    for (const [name, view] of this.unitViews) {
      if (liveNames.has(name)) continue;
      this.unitGroup.remove(view.group);
      view.group.traverse(disposeObject);
      this.unitViews.delete(name);
    }
  }

  addError(errorText) {
    if (!errorText) return;
    const texture = makeCanvasTexture(1024, 128, (ctx, width, height) => {
      ctx.fillStyle = "rgba(80, 20, 28, .94)";
      ctx.fillRect(0, 0, width, height);
      ctx.fillStyle = "#ffd8dc";
      ctx.font = "700 28px Consolas, monospace";
      ctx.fillText(String(errorText).slice(0, 140), 28, 74);
    });
    const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: texture, transparent: true, depthTest: false }));
    sprite.position.set(0, 120, this.config.worldH * 0.42);
    sprite.scale.set(this.config.worldW * 0.72, 90, 1);
    this.dynamicGroup.add(sprite);
  }

  render(world, ui = {}) {
    this.configure(ui.config || {});
    const liveNames = new Set();
    for (const ent of entitiesWith(world, "Unit", "Position")) {
      liveNames.add(ent.name);
      this.updateUnitView(ent, ui);
    }
    this.pruneUnitViews(liveNames);
    clearGroup(this.effectGroup);
    for (const ent of entitiesWith(world, "Zone")) this.addZone(ent);
    this.addTravel(world);
    this.addMoveTarget(world);
    this.addAim(world, ui);
    this.addError(ui.errorText);
    this.renderer.render(this.scene, this.camera);
  }
}
