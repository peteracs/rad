import { test } from 'node:test';
import assert from 'node:assert/strict';

import { ClientPredictionRunner } from '../src/app/clientPredictionRunner.js';
import { PredictionBuffer } from '../src/netcode/predictionBuffer.js';
import {
  ClientInputSequencer,
  createClientInputReservation,
} from '../src/netcode/inputSequencer.js';
import { FixedTickClock } from '../src/netcode/fixedTickClock.js';
import {
  FIXED_DT,
  INPUT_DELAY_TICKS,
  PREDICTION_LEAD_TICKS,
} from '../src/netcode/constants.js';
import type { AvatarRenderState } from '../src/render/worldView.js';
import type { ServerState } from '../src/transport/serverState.js';

// Local structural mirrors of radHost's RadWorld/RadEntity so this test never
// imports radHost.ts (which pulls in the wasm runtime and is not Node-loadable).
interface RadComponentLike {
  type: string;
  fields: Record<string, unknown>;
}
interface RadEntityLike {
  id: number;
  name: string | null;
  components: RadComponentLike[];
}
interface RadWorldLike {
  entities: RadEntityLike[];
  resources: Record<string, Record<string, unknown>>;
}

// ---------------------------------------------------------------------------
// Reproduces the reported symptom: "the champion doesn't move". The local
// champion mesh is driven exclusively by ClientPredictionRunner pushing avatar
// samples into the scene (the F3 server ghost is fed straight from ServerState,
// which is why it keeps moving even when the champion is frozen). This harness
// drives the REAL ClientPredictionRunner / PredictionBuffer / InputSequencer
// with a faithful fake RAD session (movement.rad's integrator: 58 u/s, FIXED_DT,
// snap-on-arrival, open field) and a fake scene that records every avatar sample
// the runner emits. If the runner ever emits a position that leaves spawn, the
// champion moves; if not, it is frozen.
// ---------------------------------------------------------------------------
const MOVE_SPEED_UNITS_PER_SEC = 58;
const PLAYER_ID = 1;
const SPAWN_X = 0;
const SPAWN_Y = 0;

class FakeRadSession {
  x = SPAWN_X;
  y = SPAWN_Y;
  targetX = SPAWN_X;
  targetY = SPAWN_Y;
  targetActive = false;
  commandId = 0;
  ticksSimulated = 0;
  moveOrders = 0;

  private readonly avatar: RadEntityLike = {
    id: 1,
    name: 'player_1',
    components: [
      { type: 'Position', fields: { x: SPAWN_X, y: SPAWN_Y } },
      { type: 'MoveTarget', fields: { x: SPAWN_X, y: SPAWN_Y, active: false, command_id: 0 } },
      { type: 'RenderAvatar', fields: { model: 'clockwork_mage', radius: 3.8 } },
      { type: 'PlayerControlled', fields: { player_id: PLAYER_ID } },
    ],
  };
  private readonly world: RadWorldLike = { entities: [this.avatar], resources: {} };

  snapshot(): RadWorldLike {
    return this.syncWorld();
  }

  refresh(): RadWorldLike {
    return this.syncWorld();
  }

  moveOrder(_playerId: number, commandId: number, targetX: number, targetY: number): void {
    this.moveOrders += 1;
    this.targetX = targetX;
    this.targetY = targetY;
    this.targetActive = true;
    this.commandId = commandId;
  }

  tickFixed(): void {
    this.ticksSimulated += 1;
    if (!this.targetActive) return;
    const dx = this.targetX - this.x;
    const dy = this.targetY - this.y;
    const distance = Math.sqrt(dx * dx + dy * dy);
    const step = MOVE_SPEED_UNITS_PER_SEC * FIXED_DT;
    if (distance <= 0.001 || distance <= step) {
      this.x = this.targetX;
      this.y = this.targetY;
      this.targetActive = false;
      return;
    }
    this.x += (dx / distance) * step;
    this.y += (dy / distance) * step;
  }

  applyAuthoritativeState(state: ServerState): void {
    this.x = state.avatar.x;
    this.y = state.avatar.y;
    this.targetX = state.avatar.target_x;
    this.targetY = state.avatar.target_y;
    this.targetActive = state.avatar.target_active;
    this.commandId = state.avatar.command_id;
  }

  private syncWorld(): RadWorldLike {
    const position = this.avatar.components[0].fields;
    const target = this.avatar.components[1].fields;
    position.x = this.x;
    position.y = this.y;
    target.x = this.targetX;
    target.y = this.targetY;
    target.active = this.targetActive;
    target.command_id = this.commandId;
    return this.world;
  }
}

class FakeScene {
  samples = 0;
  movingSamples = 0;
  nullSamples = 0;
  lastX = SPAWN_X;
  lastY = SPAWN_Y;
  maxDistanceFromSpawn = 0;

  applyAvatarState(state: AvatarRenderState | null, _tick: number): void {
    this.samples += 1;
    if (!state) {
      this.nullSamples += 1;
      return;
    }
    this.lastX = state.x;
    this.lastY = state.y;
    const distance = Math.hypot(state.x - SPAWN_X, state.y - SPAWN_Y);
    this.maxDistanceFromSpawn = Math.max(this.maxDistanceFromSpawn, distance);
    if (distance > 0.001) this.movingSamples += 1;
  }
}

function makeAuthoritativeState(
  serverTick: number,
  x: number,
  y: number,
  targetX: number,
  targetY: number,
  targetActive: boolean,
  commandId: number,
  appliedClientSeq: number,
): ServerState {
  return {
    avatar: {
      player_id: PLAYER_ID,
      model: 'clockwork_mage',
      x,
      y,
      target_x: targetX,
      target_y: targetY,
      target_active: targetActive,
      command_id: commandId,
    },
    player_id: PLAYER_ID,
    server_tick: serverTick,
    authority: {
      last_applied_client_seq: appliedClientSeq,
      applied_ack_bits: appliedClientSeq > 0 ? 1 : 0,
    },
  } as unknown as ServerState;
}

// Drives the prediction runner exactly as MobaRadClient does: per real frame it
// consumes wall-clock into fixed ticks, advances the clock, and calls
// advanceToTick(clock.tick). A move is issued the same way the client does
// (inputBaseTick + sequencer + prediction.recordMoveInput + markActive).
class PredictionRig {
  readonly clock = new FixedTickClock();
  readonly prediction = new PredictionBuffer();
  readonly sequencer = new ClientInputSequencer();
  readonly session = new FakeRadSession();
  readonly scene = new FakeScene();
  readonly runner: ClientPredictionRunner;
  private readonly reservation = createClientInputReservation();
  private serverTickEstimate = 0;

  constructor() {
    this.runner = new ClientPredictionRunner(
      this.session as never,
      PLAYER_ID,
      this.scene as never,
      this.prediction,
    );
  }

  start(nowMs: number): void {
    // FixedTickClock.consume divides (now - lastNow) by 1000 internally, so it
    // takes milliseconds — exactly what MobaRadClient passes from performance.now().
    this.clock.reset(nowMs);
    this.runner.refreshSceneSample(this.clock.tick);
  }

  // Mirror MobaRadClient.applyAuthorityState -> ClientAuthorityApplier.apply for
  // the parts that drive local prediction: advance the clock by the lead and,
  // when reconciling, replay unacked inputs from the authoritative tick.
  syncAuthority(state: ServerState, reconcile: boolean): void {
    const serverTick = Math.trunc(state.server_tick);
    this.serverTickEstimate = Math.max(this.serverTickEstimate, serverTick);
    this.clock.setTick(serverTick + PREDICTION_LEAD_TICKS);
    this.prediction.clearAppliedInputs(
      state.authority.last_applied_client_seq,
      state.authority.applied_ack_bits,
    );
    if (reconcile) {
      this.runner.applyAuthoritativeStateAndReplay(state, serverTick + 1, this.clock.tick);
    }
  }

  issueMove(targetX: number, targetY: number): void {
    const baseTick = Math.max(this.clock.tick, this.serverTickEstimate);
    const input = this.sequencer.reserveInputCommand(baseTick, INPUT_DELAY_TICKS, this.reservation);
    this.prediction.recordMoveInput(
      input.targetTick,
      input.clientSeq,
      input.commandId,
      targetX,
      targetY,
    );
    this.runner.markActive();
  }

  frame(nowMs: number): void {
    const ticks = this.clock.consume(nowMs);
    for (let i = 0; i < ticks; i += 1) this.clock.advanceOne();
    this.runner.advanceToTick(this.clock.tick);
  }
}

const FRAME_MS = 1000 / 144;

test('pure local prediction moves the champion away from spawn after a move order', () => {
  const rig = new PredictionRig();
  rig.start(0);
  // Pre-sync to a running server tick so the clock starts with a realistic lead,
  // exactly like a freshly connected client.
  rig.syncAuthority(makeAuthoritativeState(500, 0, 0, 0, 0, false, 0, 0), true);

  let issued = false;
  for (let now = FRAME_MS; now <= 1500; now += FRAME_MS) {
    if (!issued && now >= 200) {
      rig.issueMove(120, 0);
      issued = true;
    }
    rig.frame(now);
  }

  assert.ok(rig.session.moveOrders > 0, 'the queued move must reach the sim');
  assert.ok(
    rig.scene.maxDistanceFromSpawn > 1,
    `champion must leave spawn; maxDistance=${rig.scene.maxDistanceFromSpawn.toFixed(3)} ` +
      `movingSamples=${rig.scene.movingSamples} nullSamples=${rig.scene.nullSamples} ` +
      `simTicks=${rig.session.ticksSimulated}`,
  );
});

test('champion keeps moving across periodic authoritative snapshots', () => {
  const rig = new PredictionRig();
  rig.start(0);
  rig.syncAuthority(makeAuthoritativeState(500, 0, 0, 0, 0, false, 0, 0), true);

  let issued = false;
  let serverTick = 500;
  let lastSnapshotMs = 0;
  const SNAPSHOT_INTERVAL_MS = 50;

  for (let now = FRAME_MS; now <= 2000; now += FRAME_MS) {
    if (!issued && now >= 200) {
      rig.issueMove(200, 0);
      issued = true;
    }
    // Periodic authoritative snapshots, paced like the real poll/echo loop. The
    // server has NOT necessarily applied the client move yet (appliedSeq=0),
    // matching the early snapshots that arrive right after issuing a move.
    if (now - lastSnapshotMs >= SNAPSHOT_INTERVAL_MS) {
      serverTick = Math.floor(now / (1000 / 128));
      const snap = makeAuthoritativeState(serverTick, 0, 0, 0, 0, false, 0, 0);
      // Matched prediction -> no reconcile (the common steady-state path).
      rig.syncAuthority(snap, false);
      lastSnapshotMs = now;
    }
    rig.frame(now);
  }

  assert.ok(
    rig.scene.maxDistanceFromSpawn > 1,
    `champion must leave spawn even with snapshots arriving; ` +
      `maxDistance=${rig.scene.maxDistanceFromSpawn.toFixed(3)} ` +
      `movingSamples=${rig.scene.movingSamples} nullSamples=${rig.scene.nullSamples} ` +
      `simTicks=${rig.session.ticksSimulated}`,
  );
});
