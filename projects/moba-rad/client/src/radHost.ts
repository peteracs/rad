import init, { RadRuntime } from '../../../playground/pkg/rad_vm.js';
import { FIXED_DT } from './netcode/constants';
import type { ServerState } from './transport/serverState';
import { radSources } from 'moba-rad/rad-sources';
import {
  type AvatarPresentationPacket,
  isRenderableEntityId,
  signedI64AsSafeNumber,
  WasmAvatarPresentationSource,
} from './render/renderBufferContract';

const POSITION_COMPONENT = 0;
const MOVE_TARGET_COMPONENT = 1;
const RENDER_AVATAR_COMPONENT = 2;
const PLAYER_CONTROLLED_COMPONENT = 3;

export interface RadComponent {
  type: string;
  fields: Record<string, unknown>;
}

export interface RadEntity {
  id: number;
  name: string | null;
  components: RadComponent[];
}

export interface RadWorld {
  entities: RadEntity[];
  resources: Record<string, Record<string, unknown>>;
}

interface RadRenderDelta {
  upsert?: RadEntity[];
  remove?: number[];
  resources?: Record<string, Record<string, unknown>>;
}

// A cheap, allocation-light fingerprint of the exact source handed to the wasm
// VM. Lets us prove from the browser console whether the client is sending the
// seed-bearing source at all (vs a stale/empty Vite virtual module).
function describeSource(source: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < source.length; i += 1) {
    hash ^= source.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  const hasSeedHandler = /on\s+SeedLocalAvatar\b/.test(source);
  return (
    `source(len=${source.length}, fnv1a=${(hash >>> 0).toString(16)}, ` +
    `has_seed_local_avatar_handler=${hasSeedHandler})`
  );
}

// Ground-truth boot check read straight from the wasm VM world (NOT from the
// render-buffer-derived `this.world`, so a render-buffer copy bug cannot mask a
// seed failure, and vice versa). After `seedLocalAvatar` the world MUST contain
// THIS client's PlayerControlled avatar; without it the local champion can never
// be predicted or rendered and freezes at spawn while the packet-fed ghost moves.
function assertWasmWorldSeeded(
  runtime: RadRuntime,
  source: string,
  startOutput: string,
  expectedPlayerId: number,
): void {
  const snapshot = runtime.get_world_snapshot();
  const playerIds: number[] = [];
  try {
    const world = JSON.parse(snapshot) as RadWorld;
    for (const entity of world.entities ?? []) {
      for (const component of entity.components ?? []) {
        if (component.type !== 'PlayerControlled') continue;
        const id = Number(component.fields.player_id);
        if (Number.isFinite(id)) playerIds.push(id);
      }
    }
  } catch {
    // Fall through to the error path with the raw snapshot below.
  }

  if (playerIds.includes(expectedPlayerId)) {
    // eslint-disable-next-line no-console
    console.info(
      `[moba-rad] RAD world seeded local avatar player_id=${expectedPlayerId} ` +
        `(present: [${playerIds.join(', ')}]) from ${describeSource(source)}.`,
    );
    return;
  }

  // eslint-disable-next-line no-console
  console.error(
    `[moba-rad] SeedLocalAvatar did not produce a PlayerControlled avatar for ` +
      `player_id=${expectedPlayerId} (present: [${playerIds.join(', ')}]).\n` +
      `  ${describeSource(source)}\n` +
      `  session_start output: ${JSON.stringify(startOutput)}\n` +
      `  wasm world snapshot: ${snapshot}\n` +
      'If the SeedLocalAvatar event/handler is missing the client is sending ' +
      'stale RAD source — restart `npm run dev` to refresh the Vite virtual module.',
  );
}

export class RadGameSession {
  private readonly fixedTickPayload = JSON.stringify({ dt: FIXED_DT });
  private readonly entityIndexById = new Map<number, number>();
  private readonly avatarEntityGenerations = new Map<number, number>();
  private readonly entityLifetimeGenerations = new Map<number, number>();
  private readonly avatarEntityIds: number[] = [];
  private readonly world: RadWorld = { entities: [], resources: {} };
  private renderGeneration = 0;
  private renderBufferWarned = false;

  private constructor(
    private readonly runtime: RadRuntime,
    private readonly presentationSource: WasmAvatarPresentationSource,
  ) {}

  static async create(playerId: number): Promise<RadGameSession> {
    // `rad_vm_bg.wasm` has a stable name, so a normal reload happily reuses a
    // browser-cached copy of an OLD build. In dev we force a fresh fetch so a
    // `wasm-pack` rebuild is always picked up without a hard reload.
    const wasmUrl = new URL('../../../playground/pkg/rad_vm_bg.wasm', import.meta.url);
    if (import.meta.env.DEV) wasmUrl.searchParams.set('t', `${Date.now()}`);
    const wasm = await init(wasmUrl);
    const runtime = new RadRuntime();
    const source = [
      radSources.components,
      radSources.scene,
      radSources.avatars,
      radSources.movement,
      radSources.client,
    ].join('\n');
    let startOutput: string;
    try {
      startOutput = runtime.session_start(source);
    } catch (error) {
      // session_start rejects on lex/parse/type/compile errors. Swallowing it
      // leaves an empty world and a silently frozen champion — surface the
      // RAD diagnostics verbatim with the source fingerprint so the failing
      // source is identifiable.
      // eslint-disable-next-line no-console
      console.error(
        `[moba-rad] session_start FAILED on ${describeSource(source)}:\n${String(error)}`,
      );
      throw error;
    }
    const presentationSource = new WasmAvatarPresentationSource(runtime, wasm.memory);
    const session = new RadGameSession(runtime, presentationSource);
    // Identity is owned by the client (app/matchIdentity.ts persists a unique
    // id per tab), so the local avatar is seeded HERE with this client's id
    // rather than a hardcoded player 1 in the RAD source. Two tabs therefore
    // predict two distinct avatars instead of fighting over the same one.
    session.seedLocalAvatar(playerId);
    assertWasmWorldSeeded(runtime, source, startOutput, playerId);
    session.refreshJsonDelta();
    session.clearEntityCache();
    session.refresh();
    return session;
  }

  // Materialize this client's locally-predicted avatar. Idempotent on the RAD
  // side (`player_avatar` is lookup-or-seed), so it is safe to call once at boot.
  seedLocalAvatar(playerId: number): void {
    this.runtime.session_emit('SeedLocalAvatar', JSON.stringify({ player_id: playerId }));
    this.runtime.session_pump();
  }

  snapshot(): RadWorld {
    return this.world;
  }

  refresh(): RadWorld {
    this.applyRenderBuffer();
    return this.world;
  }

  private refreshJsonDelta(): RadWorld {
    this.applyDelta(JSON.parse(this.runtime.session_render_delta()) as RadRenderDelta);
    return this.world;
  }

  moveOrder(playerId: number, commandId: number, targetX: number, targetY: number): void {
    this.runtime.session_emit('MoveOrder', JSON.stringify({
      player_id: playerId,
      command_id: commandId,
      target_x: targetX,
      target_y: targetY,
    }));
    this.runtime.session_pump();
  }

  tick(dt: number): void {
    this.runtime.session_emit('Tick', JSON.stringify({ dt }));
    this.runtime.session_pump();
  }

  tickFixed(): void {
    this.runtime.session_emit('Tick', this.fixedTickPayload);
    this.runtime.session_pump();
  }

  applyAuthoritativeState(state: ServerState): void {
    this.runtime.session_emit('AuthoritativeState', JSON.stringify({
      player_id: state.player_id,
      command_id: state.avatar.command_id,
      x: state.avatar.x,
      y: state.avatar.y,
      target_x: state.avatar.target_x,
      target_y: state.avatar.target_y,
      target_active: state.avatar.target_active,
    }));
    this.runtime.session_pump();
  }

  private applyDelta(delta: RadRenderDelta): void {
    for (const entity of delta.upsert ?? []) {
      const existing = this.entityIndexById.get(entity.id);
      if (existing === undefined) {
        this.entityIndexById.set(entity.id, this.world.entities.length);
        this.world.entities.push(entity);
      } else {
        this.world.entities[existing] = entity;
      }
    }

    for (const id of delta.remove ?? []) {
      const index = this.entityIndexById.get(id);
      if (index === undefined) continue;

      const last = this.world.entities.pop();
      this.entityIndexById.delete(id);
      if (!last || index >= this.world.entities.length) continue;

      this.world.entities[index] = last;
      this.entityIndexById.set(last.id, index);
    }

    const resources = delta.resources ?? {};
    for (const name of Object.keys(resources)) {
      this.world.resources[name] = resources[name];
    }
  }

  private applyRenderBuffer(): void {
    let packet: AvatarPresentationPacket;
    try {
      packet = this.presentationSource.refresh();
    } catch (error) {
      return this.warnRenderBufferOnce(String(error));
    }
    const { words, header } = packet;
    const floats = new Float32Array(words.buffer, words.byteOffset, words.length);
    const presentation = packet.descriptor;

    this.renderGeneration += 1;
    for (let i = 0; i < header.count; i += 1) {
      const offset = presentation.headerWords + i * presentation.recordWords;
      const entityId = words[offset + presentation.fields.entity_slot] ?? -1;
      if (!isRenderableEntityId(entityId)) continue;
      this.writeAvatarEntityFromBuffer(entityId, words, floats, offset, presentation);
    }

    let i = 0;
    while (i < this.avatarEntityIds.length) {
      const entityId = this.avatarEntityIds[i];
      if (this.avatarEntityGenerations.get(entityId) === this.renderGeneration) {
        i += 1;
        continue;
      }

      this.removeEntity(entityId);
      const last = this.avatarEntityIds.pop();
      if (last !== undefined && i < this.avatarEntityIds.length) {
        this.avatarEntityIds[i] = last;
      }
    }
  }

  // The avatar render bridge reads an exact word packet straight out of WASM
  // memory each frame. If that packet is malformed (most commonly a stale or
  // mismatched `playground/pkg` build) the world never gains the controlled
  // avatar, the prediction runner can only emit null samples, and the local
  // champion freezes at spawn while the packet-fed authority ghost keeps moving.
  // Fail loud once instead of silently swallowing the mismatch.
  private warnRenderBufferOnce(reason: string): void {
    if (this.renderBufferWarned) return;
    this.renderBufferWarned = true;
    const presentation = this.presentationSource.descriptor;
    // eslint-disable-next-line no-console
    console.error(
      `[moba-rad] render buffer rejected (${reason}); ` +
        `expected version=${presentation.version} stride=${presentation.recordWords}. ` +
        'The local champion will not move until playground/pkg (the RAD wasm) is rebuilt.',
    );
  }

  private writeAvatarEntityFromBuffer(
    entityId: number,
    words: Uint32Array,
    floats: Float32Array,
    offset: number,
    presentation: AvatarPresentationPacket['descriptor'],
  ): void {
    const field = presentation.fields;
    const lifetimeGeneration = words[offset + field.entity_generation] ?? 0;
    let entity = this.entityById(entityId);
    if (entity && this.entityLifetimeGenerations.get(entityId) !== lifetimeGeneration) {
      const index = this.entityIndexById.get(entityId);
      entity = createAvatarEntity(entityId);
      if (index !== undefined) this.world.entities[index] = entity;
    }
    if (!entity) {
      entity = createAvatarEntity(entityId);
      this.entityIndexById.set(entityId, this.world.entities.length);
      this.avatarEntityIds.push(entityId);
      this.world.entities.push(entity);
    }
    this.avatarEntityGenerations.set(entityId, this.renderGeneration);
    this.entityLifetimeGenerations.set(entityId, lifetimeGeneration);

    const position = entity.components[POSITION_COMPONENT].fields;
    const target = entity.components[MOVE_TARGET_COMPONENT].fields;
    const render = entity.components[RENDER_AVATAR_COMPONENT].fields;
    const player = entity.components[PLAYER_CONTROLLED_COMPONENT].fields;

    player.player_id = words[offset + field.player_id] ?? 0;
    position.x = floats[offset + field.x] ?? 0;
    position.y = floats[offset + field.y] ?? 0;
    target.x = floats[offset + field.target_x] ?? 0;
    target.y = floats[offset + field.target_y] ?? 0;
    target.active = (words[offset + field.target_active] ?? 0) !== 0;
    target.command_id = signedI64AsSafeNumber(
      words[offset + field.command_id_low] ?? 0,
      words[offset + field.command_id_high] ?? 0,
    ) ?? 0;
    render.model = presentation.modelNames[words[offset + field.model_id] ?? 0] ?? '';
  }

  private entityById(entityId: number): RadEntity | null {
    const index = this.entityIndexById.get(entityId);
    if (index === undefined) return null;
    return this.world.entities[index] ?? null;
  }

  private removeEntity(entityId: number): void {
    const index = this.entityIndexById.get(entityId);
    if (index === undefined) return;

    const last = this.world.entities.pop();
    this.entityIndexById.delete(entityId);
    this.avatarEntityGenerations.delete(entityId);
    this.entityLifetimeGenerations.delete(entityId);
    if (!last || index >= this.world.entities.length) return;

    this.world.entities[index] = last;
    this.entityIndexById.set(last.id, index);
  }

  private clearEntityCache(): void {
    this.entityIndexById.clear();
    this.avatarEntityGenerations.clear();
    this.entityLifetimeGenerations.clear();
    this.avatarEntityIds.length = 0;
    this.world.entities.length = 0;
  }
}

function createAvatarEntity(entityId: number): RadEntity {
  return {
    id: entityId,
    name: null,
    components: [
      { type: 'Position', fields: { x: 0, y: 0 } },
      { type: 'MoveTarget', fields: { x: 0, y: 0, active: false, command_id: 0 } },
      { type: 'RenderAvatar', fields: { model: '' } },
      { type: 'PlayerControlled', fields: { player_id: 0 } },
    ],
  };
}
