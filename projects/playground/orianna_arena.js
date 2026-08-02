import { MobaArena3D } from "./orianna_three_renderer.js?v=three-perf-3";
import { RadMobaHost, ORIANNA_EVENT_SCHEMA } from "./moba_host.js?v=three-perf-3";

const STEP = 0.1;

const canvas = document.getElementById("arena");
const arena3d = new MobaArena3D(canvas);
const statusEl = document.getElementById("status");
const modeEl = document.getElementById("mode");
const logEl = document.getElementById("log");
const hpBar = document.getElementById("hpBar");
const hpText = document.getElementById("hpText");
const manaBar = document.getElementById("manaBar");
const manaText = document.getElementById("manaText");
const ballState = document.getElementById("ballState");
const anchorState = document.getElementById("anchorState");
const targetState = document.getElementById("targetState");
const tickState = document.getElementById("tickState");
const runtimeState = document.getElementById("runtimeState");
const schemaState = document.getElementById("schemaState");
const perfState = document.getElementById("perfState");
const inspectPanel = document.getElementById("inspectPanel");
const abilityEls = new Map([...document.querySelectorAll(".ability")].map(b => [b.dataset.slot, b]));
const cooldownProps = { q: "q", w: "w", e: "e", r: "r" };

let host = null;
let ready = false;
let world = { entities: new Map(), resources: {} };
let queue = [];
let mode = "move";
let hover = { x: 500, y: 350 };
let selectedTarget = "red_front";
let inspectedName = "";
let inspectMode = false;
let paused = false;
let accumulator = 0;
let lastTs = 0;
let lastHudTs = 0;
let errorText = "";
const HUD_INTERVAL_MS = 100;
const perfStats = {
  fps: 0,
  frameMs: 0,
  worstFrameMs: 0,
  frames: 0,
  sampleMs: 0,
  sampleFrames: 0,
  sampleWorstMs: 0,
};
window.__oriannaArenaPerf = perfStats;

function component(ent, type) {
  return ent?.components?.find(c => c.type === type)?.fields || null;
}

function entitiesWith(...types) {
  return [...world.entities.values()].filter(ent => types.every(t => component(ent, t)));
}

function byName(name) {
  return [...world.entities.values()].find(ent => ent.name === name) || null;
}

function unitById(id) {
  return entitiesWith("Unit").find(ent => component(ent, "Unit").id === id) || null;
}

function num(v, fallback = 0) {
  return Number.isFinite(Number(v)) ? Number(v) : fallback;
}

function send(name, fields) {
  queue.push({ name, fields });
}

function pumpEvents(events, checkpoint = false) {
  if (!host || events.length === 0) return;
  try {
    const result = host.sendBatch(events, { checkpoint });
    if (result.output) console.log("[rad]", result.output);
    world = host.world;
    errorText = "";
  } catch (err) {
    errorText = String(err);
    statusEl.textContent = errorText;
    console.error(err);
  }
}

function setMode(next) {
  mode = next;
  modeEl.textContent = next.toUpperCase();
  for (const [slot, el] of abilityEls) el.classList.toggle("active", slot === next);
  if (next === "move") targetState.textContent = selectedTarget;
  if (next === "q") targetState.textContent = "ground";
  if (next === "e") targetState.textContent = "ally";
}

function parseAbilitySchema(schema = "") {
  return new Set(String(schema).split(",").map(slot => slot.trim()).filter(Boolean));
}

function syncAbilityButtons(contract) {
  const supported = parseAbilitySchema(contract?.ability_schema);
  if (supported.size === 0) return supported;
  for (const [slot, el] of abilityEls) {
    const enabled = supported.has(slot);
    el.hidden = !enabled;
    el.disabled = !enabled;
    if (!enabled && mode === slot) setMode("move");
  }
  return supported;
}

function setInspectMode(enabled) {
  inspectMode = enabled;
  document.getElementById("inspect").classList.toggle("active", enabled);
}

function canvasPoint(ev) {
  return arena3d.pickPoint(ev, world.resources.GameHostConfig || {});
}

function distance(a, b) {
  const dx = a.x - b.x;
  const dy = a.y - b.y;
  return Math.hypot(dx, dy);
}

function posOf(ent) {
  const p = component(ent, "Position");
  return p ? { x: num(p.x), y: num(p.y) } : { x: 0, y: 0 };
}

function nearestUnit(point, predicate, maxDist = 70) {
  let best = null;
  let bestD = maxDist;
  for (const ent of entitiesWith("Unit", "Position")) {
    const u = component(ent, "Unit");
    if (!predicate(ent, u)) continue;
    const d = distance(point, posOf(ent));
    if (d < bestD) {
      best = ent;
      bestD = d;
    }
  }
  return best;
}

function nearestEnemy(point) {
  return nearestUnit(point, (ent, u) => u.team === "red" && u.alive !== false, 95);
}

function nearestAlly(point) {
  return nearestUnit(point, (ent, u) => u.team === "blue" && u.id !== "orianna_ball", 110);
}

function entityDisplayName(ent) {
  const u = component(ent, "Unit");
  return u?.id || ent?.name || "";
}

function inspectEntity(ent) {
  if (!ent || !host) return;
  inspectedName = ent.name;
  const lines = [`${entityDisplayName(ent)} (${ent.name})`];
  for (const c of ent.components) {
    lines.push("");
    lines.push(`${c.type}: ${JSON.stringify(c.fields)}`);
    try {
      lines.push(host.why(ent.name, c.type));
    } catch (err) {
      lines.push(`why unavailable: ${String(err)}`);
    }
  }
  inspectPanel.textContent = lines.join("\n");
}

function cast(slot, point = hover) {
  if (slot === "q") {
    send("CastCommand", { slot: "q", x: point.x, y: point.y, target_id: "" });
    setMode("move");
    return;
  }
  if (slot === "w" || slot === "r") {
    send("CastCommand", { slot, x: point.x, y: point.y, target_id: "" });
    setMode("move");
    return;
  }
  if (slot === "e") {
    const ally = nearestAlly(point) || unitById("blue_ally");
    send("CastCommand", {
      slot: "e",
      x: point.x,
      y: point.y,
      target_id: component(ally, "Unit").id,
    });
    setMode("move");
    return;
  }
}

function passiveHit(point = hover) {
  const target = nearestEnemy(point) || unitById(selectedTarget) || unitById("red_front");
  if (target) {
    const id = component(target, "Unit").id;
    selectedTarget = id;
    send("BasicAttackCommand", { target_id: id });
  }
}

canvas.addEventListener("pointermove", ev => {
  hover = canvasPoint(ev);
  const enemy = nearestEnemy(hover);
  if (enemy) selectedTarget = component(enemy, "Unit").id;
});

canvas.addEventListener("pointerdown", ev => {
  ev.preventDefault();
  canvas.setPointerCapture?.(ev.pointerId);
  const p = canvasPoint(ev);
  hover = p;
  if (inspectMode) {
    const picked = nearestUnit(p, () => true, 130);
    if (picked) inspectEntity(picked);
    return;
  }
  if (mode === "q") return cast("q", p);
  if (mode === "e") return cast("e", p);
  const enemy = nearestEnemy(p);
  if (ev.altKey && enemy) return passiveHit(p);
  send("MoveCommand", { x: p.x, y: p.y });
});

document.addEventListener("keydown", ev => {
  if (ev.repeat) return;
  const k = ev.key.toLowerCase();
  const mod = ev.ctrlKey || ev.metaKey;
  if (mod && k === "z") {
    ev.preventDefault();
    if (ev.shiftKey) redoWorld();
    else undoWorld();
    return;
  }
  if (k === "q") { setMode(mode === "q" ? "move" : "q"); ev.preventDefault(); }
  if (k === "w") { cast("w"); ev.preventDefault(); }
  if (k === "e") { setMode(mode === "e" ? "move" : "e"); ev.preventDefault(); }
  if (k === "r") { cast("r"); ev.preventDefault(); }
  if (k === "a") { passiveHit(); ev.preventDefault(); }
  if (k === "escape") { setMode("move"); ev.preventDefault(); }
});

for (const [slot, el] of abilityEls) {
  el.addEventListener("click", () => {
    if (slot === "q" || slot === "e") setMode(mode === slot ? "move" : slot);
    else if (slot === "aa") passiveHit();
    else cast(slot);
  });
}

document.getElementById("reset").addEventListener("click", () => {
  send("ResetGame", { reason: "button" });
  setMode("move");
});

function undoWorld() {
  try {
    if (host?.undo()) {
      world = host.world;
      statusEl.textContent = "Rewound one command";
      updateHud();
      draw();
    }
  } catch (err) {
    errorText = String(err);
  }
}

function redoWorld() {
  try {
    if (host?.redo()) {
      world = host.world;
      statusEl.textContent = "Redid one command";
      updateHud();
      draw();
    }
  } catch (err) {
    errorText = String(err);
  }
}

document.getElementById("undo").addEventListener("click", undoWorld);
document.getElementById("redo").addEventListener("click", redoWorld);
document.getElementById("inspect").addEventListener("click", () => setInspectMode(!inspectMode));
document.getElementById("pause").addEventListener("click", () => {
  paused = !paused;
  document.getElementById("pause").classList.toggle("active", paused);
});
document.getElementById("step").addEventListener("click", () => {
  pumpEvents([{ name: "Tick", fields: { dt: STEP } }]);
  updateHud();
  draw();
});

function draw(frameDtMs = 1000 / 60) {
  arena3d.render(world, {
    config: world.resources.GameHostConfig || {},
    mode,
    hover,
    selectedTarget,
    inspectedName,
    errorText,
    frameDtMs,
  });
}

function cooldownText(value) {
  const ticks = num(value);
  if (ticks <= 0) return "";
  return (ticks / 10).toFixed(1);
}

function updateHud() {
  const ori = byName("orianna");
  if (!ori) return;
  const h = component(ori, "Health");
  const book = component(ori, "SpellBook");
  const cd = component(ori, "Cooldowns") || {};
  const st = component(ori, "OriannaState") || {};
  const clock = world.resources.MatchClock || { tick: 0 };
  const contract = world.resources.HostContract || {};
  const ball = byName("orianna_ball");
  const bp = ball ? posOf(ball) : { x: 0, y: 0 };

  const hp = num(h?.hp);
  const hpMax = Math.max(1, num(h?.max));
  hpBar.style.width = `${Math.max(0, Math.min(100, hp / hpMax * 100))}%`;
  hpText.textContent = `${Math.round(hp)} / ${Math.round(hpMax)}`;

  const mana = num(book?.mana);
  manaBar.style.width = `${Math.max(0, Math.min(100, mana / 1000 * 100))}%`;
  manaText.textContent = `${Math.round(mana)} / 1000`;

  ballState.textContent = `${Math.round(bp.x)}, ${Math.round(bp.y)}`;
  anchorState.textContent = st.anchor_id || "-";
  targetState.textContent = selectedTarget;
  tickState.textContent = String(clock.tick || 0);
  runtimeState.textContent = host?.runtimeFeatures?.version || "-";
  schemaState.textContent = contract.game_stack_version || "-";
  perfState.textContent = perfStats.fps > 0 ? `${Math.round(perfStats.fps)} / ${perfStats.frameMs.toFixed(1)}ms` : "-";
  statusEl.textContent = paused ? "Paused" : book?.sealed ? "Ball in flight" : "Rad session live";

  const supported = syncAbilityButtons(contract);
  for (const [slot, prop] of Object.entries(cooldownProps)) {
    if (supported.size > 0 && !supported.has(slot)) continue;
    const el = abilityEls.get(slot);
    const badge = el?.querySelector("em");
    if (!el || !badge) continue;
    const text = cooldownText(cd[prop]);
    badge.textContent = text;
    el.classList.toggle("cooling", text !== "");
    el.classList.toggle("blocked", !!book?.sealed);
  }

  const logs = entitiesWith("LogLine")
    .map(ent => component(ent, "LogLine"))
    .sort((a, b) => num(b.idx) - num(a.idx))
    .slice(0, 14)
    .map(l => l.text);
  logEl.textContent = logs.join("\n");
}

function updatePerf(dtMs) {
  perfStats.frames += 1;
  perfStats.sampleFrames += 1;
  perfStats.sampleMs += dtMs;
  perfStats.sampleWorstMs = Math.max(perfStats.sampleWorstMs, dtMs);

  if (perfStats.sampleMs >= 500) {
    perfStats.fps = perfStats.sampleFrames / (perfStats.sampleMs / 1000);
    perfStats.frameMs = perfStats.sampleMs / Math.max(1, perfStats.sampleFrames);
    perfStats.worstFrameMs = perfStats.sampleWorstMs;
    perfStats.sampleFrames = 0;
    perfStats.sampleMs = 0;
    perfStats.sampleWorstMs = 0;
  }
}

function frame(ts) {
  if (!lastTs) lastTs = ts;
  const dt = Math.min(0.25, (ts - lastTs) / 1000);
  const dtMs = dt * 1000;
  lastTs = ts;
  accumulator += dt;
  updatePerf(dtMs);

  if (ready) {
    if (queue.length) {
      const events = queue.splice(0, queue.length);
      pumpEvents(events, true);
    }
    while (!paused && accumulator >= STEP) {
      accumulator -= STEP;
      pumpEvents([{ name: "Tick", fields: { dt: STEP } }]);
    }
    if (ts - lastHudTs >= HUD_INTERVAL_MS) {
      updateHud();
      lastHudTs = ts;
    }
  }
  draw(dtMs);
  requestAnimationFrame(frame);
}

async function boot() {
  try {
    host = new RadMobaHost({
      entryPath: "projects/dogfood/orianna_gui/orianna_arena.rad",
      eventSchema: ORIANNA_EVENT_SCHEMA,
    });
    await host.start();
    world = host.world;
    ready = true;
    const files = host.bundle.files.map(path => path.split("/").pop()).join(", ");
    statusEl.textContent = `Rad session live (${files})`;
    setMode("move");
  } catch (err) {
    errorText = String(err);
    statusEl.textContent = errorText;
    console.error(err);
  }
}

boot();
requestAnimationFrame(frame);
