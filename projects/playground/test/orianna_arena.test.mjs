import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = normalize(join(here, "../../.."));
const runtimeUrl = new URL("../pkg-node/rad_vm.js", import.meta.url);
const runtimePath = fileURLToPath(runtimeUrl);
const RadRuntime = existsSync(runtimePath) ? (await import(runtimeUrl.href)).RadRuntime : null;

function requireRuntime(t) {
  if (RadRuntime) return true;
  t.skip("projects/playground/pkg-node/rad_vm.js is not built");
  return false;
}

function bundleRad(entry) {
  const seen = new Set();
  function load(rel) {
    const clean = normalize(rel).replaceAll("\\", "/");
    if (seen.has(clean)) return "";
    seen.add(clean);
    const abs = join(root, clean);
    const src = readFileSync(abs, "utf8");
    const dir = dirname(clean).replaceAll("\\", "/");
    const chunks = [];
    for (const match of src.matchAll(/^use\s+"([^"]+)"\s*$/gm)) {
      chunks.push(load(`${dir}/${match[1]}`));
    }
    chunks.push(`\n// module: ${clean}\n${src.replace(/^use\s+"[^"]+"\s*$/gm, "")}`);
    return chunks.join("\n");
  }
  return load(entry);
}

function freshArena() {
  const rt = new RadRuntime();
  const source = bundleRad("projects/dogfood/orianna_gui/orianna_arena.rad");
  rt.session_start(source);
  return rt;
}

function snapshot(rt) {
  return JSON.parse(rt.get_world_snapshot());
}

function entityByName(snap, name) {
  return snap.entities.find(ent => ent.name === name);
}

function comp(ent, type) {
  return ent.components.find(c => c.type === type)?.fields;
}

function pump(rt, name, fields) {
  rt.session_emit(name, JSON.stringify(fields));
  rt.session_pump();
}

test("Orianna arena exports a host contract and runtime feature fingerprint", (t) => {
  if (!requireRuntime(t)) return;
  const rt = freshArena();
  const features = JSON.parse(rt.runtime_features());
  assert.equal(features.version, "0.5.0");
  assert.ok(features.features.includes("undo-redo"));

  const contract = snapshot(rt).resources.HostContract;
  assert.equal(contract.game_stack_version, "moba-stack/0.2");
  assert.equal(contract.ability_schema, "q,w,e,r,aa");
  assert.match(contract.host_features, /event-bridge/);
  assert.match(contract.event_schema, /CastCommand/);
});

test("Orianna browser shell matches the RAD ability contract", () => {
  const html = readFileSync(join(root, "projects/playground/orianna_arena.html"), "utf8");
  const host = readFileSync(join(root, "projects/playground/moba_host.js"), "utf8");
  const slots = [...html.matchAll(/data-slot="([^"]+)"/g)].map(match => match[1]);
  assert.deepEqual(slots, ["q", "w", "e", "r", "aa"]);
  assert.equal(slots.includes("return"), false);
  assert.match(host, /CastCommand: \{ slot: \["q", "w", "e", "r"\]/);
  assert.doesNotMatch(host, /"return"/);
});

test("Orianna arena runs Q travel through the module-bundled game stack", (t) => {
  if (!requireRuntime(t)) return;
  const rt = freshArena();
  rt.session_checkpoint();
  pump(rt, "CastCommand", { slot: "q", x: 610, y: 350, target_id: "" });

  let snap = snapshot(rt);
  const ball = entityByName(snap, "orianna_ball");
  assert.equal(comp(ball, "BallTravel").kind, "q");
  assert.equal(comp(entityByName(snap, "orianna"), "SpellBook").mana, 930);

  assert.equal(rt.session_undo(), true);
  snap = snapshot(rt);
  assert.equal(comp(entityByName(snap, "orianna_ball"), "BallTravel"), undefined);
  assert.equal(comp(entityByName(snap, "orianna"), "SpellBook").mana, 1000);

  assert.equal(rt.session_redo(), true);
  snap = snapshot(rt);
  assert.equal(comp(entityByName(snap, "orianna_ball"), "BallTravel").kind, "q");
});

test("Orianna arena dogfoods inspect why after a passive hit", (t) => {
  if (!requireRuntime(t)) return;
  const rt = freshArena();
  pump(rt, "BasicAttackCommand", { target_id: "red_front" });
  rt.session_pump();
  const snap = snapshot(rt);
  const red = entityByName(snap, "red_front");
  assert.ok(comp(red, "Health").hp < comp(red, "Health").max);

  const why = rt.session_why("red_front", "Health");
  assert.match(why, /DamageRequest/);
  assert.match(why, /BasicAttackCommand/);
});
