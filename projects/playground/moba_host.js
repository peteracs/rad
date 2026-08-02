import init, { RadRuntime } from "./pkg/rad_vm.js";

export const ORIANNA_EVENT_SCHEMA = {
  Tick: { dt: "number" },
  MoveCommand: { x: "number", y: "number" },
  CastCommand: { slot: ["q", "w", "e", "r"], x: "number", y: "number", target_id: "string" },
  ResetGame: { reason: "string" },
  BasicAttackCommand: { target_id: "string" },
};

const EMPTY_DELTA = { upsert: [], remove: [], resources: {} };

function normalizePath(path) {
  const parts = [];
  for (const part of path.replaceAll("\\", "/").split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return parts.join("/");
}

function dirname(path) {
  const at = path.lastIndexOf("/");
  return at >= 0 ? path.slice(0, at + 1) : "";
}

function hashSource(source) {
  let h = 5381;
  for (let i = 0; i < source.length; i++) h = ((h * 33) ^ source.charCodeAt(i)) >>> 0;
  return h.toString(36);
}

function cmpVersion(actual, want) {
  const a = String(actual || "0").split(".").map(n => Number(n) || 0);
  const b = String(want || "0").split(".").map(n => Number(n) || 0);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const av = a[i] || 0;
    const bv = b[i] || 0;
    if (av < bv) return -1;
    if (av > bv) return 1;
  }
  return 0;
}

export async function bundleRad(entryPath, options = {}) {
  const rootPrefix = options.rootPrefix || "../../";
  const seen = new Set();
  const files = [];

  async function load(path) {
    const normalized = normalizePath(path);
    if (seen.has(normalized)) return "";
    seen.add(normalized);

    const response = await fetch(`${rootPrefix}${normalized}?v=${Date.now()}`, { cache: "no-store" });
    if (!response.ok) throw new Error(`cannot load ${normalized} (${response.status})`);
    const source = await response.text();
    files.push(normalized);

    const deps = [];
    for (const match of source.matchAll(/^use\s+"([^"]+)"\s*$/gm)) {
      deps.push(normalizePath(dirname(normalized) + match[1]));
    }

    const chunks = [];
    for (const dep of deps) chunks.push(await load(dep));
    const stripped = source.replace(/^use\s+"[^"]+"\s*$/gm, "");
    chunks.push(`\n// module: ${normalized}\n${stripped}`);
    return chunks.join("\n");
  }

  const cleanEntryPath = entryPath.replace(/^dogfood\//, "projects/dogfood/");
  const source = await load(cleanEntryPath);
  return { source, files, hash: hashSource(source) };
}

export function validateEvent(schema, name, fields) {
  const spec = schema[name];
  if (!spec) throw new Error(`host event '${name}' is not in the typed bridge`);
  for (const [field, rule] of Object.entries(spec)) {
    if (!(field in fields)) throw new Error(`${name}.${field} is missing`);
    const value = fields[field];
    if (Array.isArray(rule)) {
      if (!rule.includes(value)) throw new Error(`${name}.${field} must be one of ${rule.join(", ")}`);
    } else if (rule === "number") {
      if (!Number.isFinite(Number(value))) throw new Error(`${name}.${field} must be a finite number`);
    } else if (typeof value !== rule) {
      throw new Error(`${name}.${field} must be ${rule}`);
    }
  }
}

export class RadMobaHost {
  constructor(options = {}) {
    this.entryPath = options.entryPath || "projects/dogfood/orianna_gui/orianna_arena.rad";
    this.eventSchema = options.eventSchema || ORIANNA_EVENT_SCHEMA;
    this.runtime = null;
    this.bundle = null;
    this.world = { entities: new Map(), resources: {} };
    this.runtimeFeatures = { version: "unknown", features: [] };
    this.contractWarnings = [];
  }

  async start() {
    await init("./pkg/rad_vm_bg.wasm");
    this.runtime = new RadRuntime();
    this.bundle = await bundleRad(this.entryPath);
    if (typeof this.runtime.runtime_features === "function") {
      this.runtimeFeatures = JSON.parse(this.runtime.runtime_features());
    }
    const output = this.runtime.session_start(this.bundle.source);
    this.applyDelta(JSON.parse(this.runtime.session_render_delta()));
    this.checkContract();
    return output;
  }

  applyDelta(delta) {
    for (const ent of delta.upsert || []) this.world.entities.set(ent.id, ent);
    for (const id of delta.remove || []) this.world.entities.delete(id);
    for (const [name, fields] of Object.entries(delta.resources || {})) this.world.resources[name] = fields;
  }

  checkContract() {
    const contract = this.world.resources.HostContract || {};
    const warnings = [];
    if (contract.runtime_min && cmpVersion(this.runtimeFeatures.version, contract.runtime_min) < 0) {
      warnings.push(`Rad WASM ${this.runtimeFeatures.version} is older than required ${contract.runtime_min}`);
    }
    const required = String(contract.host_features || "").split(",").filter(Boolean);
    const hostFeatures = new Set([
      "modules",
      "event-bridge",
      "runtime-contract",
      "undo-redo",
      "inspect-why",
      "vec2",
      "spatial",
      "cooldowns",
      "ability-dsl",
      "map4-world",
    ]);
    for (const feature of required) {
      if (!hostFeatures.has(feature)) warnings.push(`host feature missing: ${feature}`);
    }
    this.contractWarnings = warnings;
    if (warnings.length) throw new Error(warnings.join("; "));
  }

  sendBatch(events, options = {}) {
    if (!this.runtime || events.length === 0) return { output: "", delta: EMPTY_DELTA };
    for (const ev of events) validateEvent(this.eventSchema, ev.name, ev.fields);
    if (options.checkpoint) this.runtime.session_checkpoint();
    for (const ev of events) this.runtime.session_emit(ev.name, JSON.stringify(ev.fields));
    const output = this.runtime.session_pump();
    const delta = JSON.parse(this.runtime.session_render_delta());
    this.applyDelta(delta);
    return { output, delta };
  }

  undo() {
    const did = this.runtime.session_undo();
    const delta = did ? JSON.parse(this.runtime.session_render_delta()) : EMPTY_DELTA;
    this.applyDelta(delta);
    return did;
  }

  redo() {
    const did = this.runtime.session_redo();
    const delta = did ? JSON.parse(this.runtime.session_render_delta()) : EMPTY_DELTA;
    this.applyDelta(delta);
    return did;
  }

  why(entityName, componentName) {
    return this.runtime.session_why(entityName, componentName);
  }
}
