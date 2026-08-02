import { test } from 'node:test';
import assert from 'node:assert/strict';

import { FixedTickClock } from '../src/netcode/fixedTickClock.js';
import { PredictionBuffer } from '../src/netcode/predictionBuffer.js';
import { PredictedMoveApplier } from '../src/netcode/predictedMoveApplier.js';
import { ClientInputSequencer, createClientInputReservation } from '../src/netcode/inputSequencer.js';
import { FIXED_DT } from '../src/netcode/constants.js';
import {
  createReconciliationDecision,
  ReconciliationPolicy,
} from '../src/netcode/reconciliationPolicy.js';

// ---------------------------------------------------------------------------
// Faithful, deterministic reproduction of the authoritative movement integrator
// shared by sim/movement.rad (the RAD VM) and the browser prediction client.
// Both the client predictor and the server authority in this harness run THIS
// exact integrator, so any divergence between them is a pure tick-scheduling /
// reconciliation artifact, never a physics-math mismatch.
//   speed = 58 units/sec (components.rad MoveSpeed), dt = 1/128 (FIXED_DT),
//   snap-to-target within one step, open field (no static colliders).
// ---------------------------------------------------------------------------
const MOVE_SPEED_UNITS_PER_SEC = 58;
const PLAYER_ID = 1;
// Prediction lead: how many ticks the client simulates ahead of the latest
// authoritative tick so reconciliation always finds the server tick in history.
const PREDICTION_LEAD_TICKS = 4;

interface SimPosition {
  x: number;
  y: number;
}

class FakeSim {
  x = 0;
  y = 0;
  targetX = 0;
  targetY = 0;
  targetActive = false;
  commandId = 0;
  ticksSimulated = 0;

  moveOrder(_playerId: number, commandId: number, targetX: number, targetY: number): void {
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

  applyAuthoritativeState(
    x: number,
    y: number,
    targetX: number,
    targetY: number,
    targetActive: boolean,
    commandId: number,
  ): void {
    this.x = x;
    this.y = y;
    this.targetX = targetX;
    this.targetY = targetY;
    this.targetActive = targetActive;
    this.commandId = commandId;
  }

  readPosition(out: SimPosition): void {
    out.x = this.x;
    out.y = this.y;
  }
}

interface AuthoritySnapshot {
  serverTick: number;
  serverSeq: number;
  x: number;
  y: number;
  targetX: number;
  targetY: number;
  targetActive: boolean;
  commandId: number;
  appliedSeq: number;
  appliedAckBits: number;
}

interface QueuedMove {
  targetTick: number;
  clientSeq: number;
  commandId: number;
  targetX: number;
  targetY: number;
}

// The authority advances its own fixed-tick clock, applies queued client moves
// at their target tick (inputs processed by tick index, not arrival time), and
// records per-tick state so it can answer "what was the world at tick T".
class AuthorityModel {
  private readonly sim = new FakeSim();
  private tick = 0;
  private appliedSeq = 0;
  private appliedAckBits = 0;
  private readonly queue: QueuedMove[] = [];
  private readonly history: AuthoritySnapshot[] = [];

  constructor() {
    this.captureHistory();
  }

  enqueueMove(move: QueuedMove): void {
    this.queue.push(move);
  }

  advanceTo(targetTick: number): void {
    while (this.tick < targetTick) {
      const next = this.tick + 1;
      this.applyQueuedForTick(next);
      this.sim.tickFixed();
      this.tick = next;
      this.captureHistory();
    }
  }

  // Newest snapshot at-or-before deliveredTick (models one-way network latency).
  snapshotAt(deliveredTick: number): AuthoritySnapshot | null {
    const clamped = Math.min(deliveredTick, this.tick);
    if (clamped < 0) return null;
    return this.history[clamped] ?? null;
  }

  get currentTick(): number {
    return this.tick;
  }

  private applyQueuedForTick(tick: number): void {
    for (let i = 0; i < this.queue.length; i += 1) {
      const move = this.queue[i];
      if (move.targetTick !== tick) continue;
      this.sim.moveOrder(PLAYER_ID, move.commandId, move.targetX, move.targetY);
      this.appliedSeq = move.clientSeq;
      this.appliedAckBits = 1; // bit0 acks the applied seq itself
    }
  }

  private captureHistory(): void {
    this.history[this.tick] = {
      serverTick: this.tick,
      serverSeq: this.tick + 1,
      x: this.sim.x,
      y: this.sim.y,
      targetX: this.sim.targetX,
      targetY: this.sim.targetY,
      targetActive: this.sim.targetActive,
      commandId: this.sim.commandId,
      appliedSeq: this.appliedSeq,
      appliedAckBits: this.appliedAckBits,
    };
  }
}

interface ScenarioResult {
  frames: number;
  acceptedSnapshots: number;
  reconciles: number;
  movingSnapshots: number;
  movingReconciles: number;
  reconcileNoPrediction: number;
  reconcileError: number;
  reconcileTargetMismatch: number;
  maxCorrectionDistance: number;
  lateMoves: number;
  sampleCorrections: string[];
}

interface HarnessConfig {
  leadTicks: number;
  recordEveryTick: boolean;
  // When false, ticks that setTick() jumps the clock over are NOT simulated or
  // recorded (models recording only inside the consume/advanceOne loop). Those
  // skipped ticks become permanent gaps in the prediction ring.
  catchUpAfterSync: boolean;
}

// Faithful mirror of MobaRadClient's local-player predict + reconcile loop,
// using the real netcode units. `config` toggles the candidate fixes so we can
// compare broken vs corrected behaviour under an identical timeline.
class ClientHarness {
  private readonly clock = new FixedTickClock();
  private readonly prediction = new PredictionBuffer();
  private readonly applier = new PredictedMoveApplier(this.prediction);
  private readonly reconciliationPolicy = new ReconciliationPolicy();
  private readonly reconciliationDecision = createReconciliationDecision();
  private readonly sequencer = new ClientInputSequencer();
  private readonly reservation = createClientInputReservation();
  private readonly sim = new FakeSim();
  private readonly posScratch: SimPosition = { x: 0, y: 0 };
  private readonly authority: AuthorityModel;
  private readonly result: ScenarioResult;
  private readonly config: HarnessConfig;

  private localSimulationActive = false;
  private serverTickEstimate = 0;
  private lastServerSeq = 0;
  private lastRecordedTick = -1;
  // Bookkeeping: the tick up to which the local sim has actually run.
  private simulatedTick = 0;

  constructor(authority: AuthorityModel, result: ScenarioResult, config: HarnessConfig) {
    this.authority = authority;
    this.result = result;
    this.config = config;
  }

  issueMove(targetX: number, targetY: number): void {
    const input = this.sequencer.reserveInputCommand(this.inputBaseTick(), 3, this.reservation);
    this.prediction.recordMoveInput(input.targetTick, input.clientSeq, input.commandId, targetX, targetY);
    this.localSimulationActive = true;
    if (input.targetTick <= this.authority.currentTick) {
      this.result.lateMoves += 1;
    }
    this.authority.enqueueMove({
      targetTick: input.targetTick,
      clientSeq: input.clientSeq,
      commandId: input.commandId,
      targetX,
      targetY,
    });
  }

  frame(nowMs: number, deliveredTick: number): void {
    this.result.frames += 1;
    const snapshot = this.authority.snapshotAt(deliveredTick);
    if (snapshot) this.applyAuthorityState(snapshot);

    const ticks = this.clock.consume(nowMs / 1000);
    let advanced = false;
    for (let i = 0; i < ticks; i += 1) {
      const tick = this.clock.advanceOne();
      this.stepSimToTick(tick);
      if (this.localSimulationActive) advanced = true;
    }

    if (advanced || this.simulatedTick < this.clock.tick) {
      this.refreshWorldAndScene();
    }
  }

  // Normal-path per-tick advance.
  private stepSimToTick(tick: number): void {
    this.applyLocalInputForTick(tick);
    if (this.localSimulationActive) {
      this.sim.tickFixed();
      this.simulatedTick = tick;
      // FIX: record a prediction sample for EVERY simulated tick, not just the
      // final tick of the frame, so reconciliation always finds the server tick
      // in history.
      if (this.config.recordEveryTick) this.recordPredictionSample(tick);
    }
  }

  private recordPredictionSample(tick: number): void {
    this.sim.readPosition(this.posScratch);
    this.prediction.recordPosition(tick, this.posScratch.x, this.posScratch.y);
    this.lastRecordedTick = tick;
  }

  private applyLocalInputForTick(tick: number): void {
    if (this.applier.applyDueMovesAtOrBefore(tick, this.sim, PLAYER_ID) > 0) {
      this.localSimulationActive = true;
    }
  }

  // FIX: catch the local simulation up to clock.tick so that a server-driven
  // setTick() bump never leaves a recorded position labelled with a tick the
  // sim never actually stepped to.
  private catchUpSimulation(): void {
    if (!this.config.catchUpAfterSync) {
      // Engineer model: ticks the clock jumped over via setTick are neither
      // simulated nor recorded -- they become permanent gaps in the ring.
      this.simulatedTick = Math.max(this.simulatedTick, this.clock.tick);
      return;
    }
    while (this.simulatedTick < this.clock.tick) {
      const tick = this.simulatedTick + 1;
      this.applyLocalInputForTick(tick);
      if (this.localSimulationActive) this.sim.tickFixed();
      this.simulatedTick = tick;
      if (this.config.recordEveryTick) this.recordPredictionSample(tick);
    }
  }

  private refreshWorldAndScene(): void {
    this.catchUpSimulation();
    this.sim.readPosition(this.posScratch);
    this.prediction.recordPosition(this.clock.tick, this.posScratch.x, this.posScratch.y);
    this.lastRecordedTick = this.clock.tick;
    this.localSimulationActive =
      this.sim.targetActive || this.prediction.hasPendingMoveAtOrAfter(this.clock.tick + 1);
  }

  private applyAuthorityState(state: AuthoritySnapshot): void {
    if (state.serverSeq <= this.lastServerSeq) return;
    this.lastServerSeq = state.serverSeq;
    this.result.acceptedSnapshots += 1;

    const serverTick = state.serverTick;
    this.serverTickEstimate = Math.max(this.serverTickEstimate, serverTick);
    // FIX: the client must run a prediction LEAD ahead of the authority so that
    // the (past) server tick it reconciles against is always already simulated
    // and recorded. Pinning clock.tick to serverTick (zero lead) is what causes
    // the off-by-one no-prediction reconcile storm during movement.
    this.clock.setTick(serverTick + this.config.leadTicks);
    this.catchUpSimulation();
    this.prediction.clearAppliedInputs(state.appliedSeq, state.appliedAckBits);

    const localActive = this.sim.targetActive;
    const localCmd = this.sim.commandId;
    if (state.commandId < this.sequencer.currentCommandId && localActive) {
      return;
    }

    const hasPrediction = this.prediction.hasPositionAt(serverTick);
    const errorSq = hasPrediction
      ? this.prediction.positionErrorSq(serverTick, state.x, state.y)
      : Number.POSITIVE_INFINITY;
    const reconciliation = this.reconciliationPolicy.decide(
      this.sequencer.currentCommandId,
      true,
      localCmd,
      localActive,
      state.commandId,
      state.targetActive,
      hasPrediction,
      errorSq,
      this.reconciliationDecision,
    );

    const moving = state.targetActive || localActive;
    if (moving) this.result.movingSnapshots += 1;
    if (reconciliation.ignoreOlderCommand || !reconciliation.shouldReconcile) return;

    this.result.reconciles += 1;
    if (moving) this.result.movingReconciles += 1;
    if (!hasPrediction) this.result.reconcileNoPrediction += 1;
    else if (reconciliation.positionMismatch) this.result.reconcileError += 1;
    else if (reconciliation.targetMismatch) this.result.reconcileTargetMismatch += 1;
    if (reconciliation.smoothCorrection) {
      this.result.maxCorrectionDistance = Math.max(
        this.result.maxCorrectionDistance,
        reconciliation.correctionDistance,
      );
    }
    if (moving && this.result.sampleCorrections.length < 8) {
      this.result.sampleCorrections.push(
        `serverTick=${serverTick} clockTick=${this.clock.tick} lastRecordedTick=${this.lastRecordedTick} ` +
          `tickLead=${this.clock.tick - serverTick} hasPred=${hasPrediction} ` +
          `delta=${reconciliation.smoothCorrection ? reconciliation.correctionDistance.toFixed(4) : 'no-pred-sample'}`,
      );
    }

    this.sim.applyAuthoritativeState(
      state.x,
      state.y,
      state.targetX,
      state.targetY,
      state.targetActive,
      state.commandId,
    );
    this.simulatedTick = serverTick;
    this.applier.replayWindow(serverTick + 1, this.clock.tick, this.sim, PLAYER_ID);
    this.simulatedTick = Math.max(this.simulatedTick, this.clock.tick);
    this.refreshWorldAndScene();
  }

  private inputBaseTick(): number {
    return Math.max(this.clock.tick, this.serverTickEstimate);
  }
}

function runScenario(config: HarnessConfig): ScenarioResult {
  const STEP_MS = 1000 / 128; // server fixed tick
  const FRAME_MS = 1000 / 144; // 144 Hz gaming display
  const LATENCY_MS = 8; // one-way (~16 ms RTT, matches reported ping)
  const DURATION_MS = 3000;
  const MOVE_AT_MS = 500;

  const authority = new AuthorityModel();
  const result: ScenarioResult = {
    frames: 0,
    acceptedSnapshots: 0,
    reconciles: 0,
    movingSnapshots: 0,
    movingReconciles: 0,
    reconcileNoPrediction: 0,
    reconcileError: 0,
    reconcileTargetMismatch: 0,
    maxCorrectionDistance: 0,
    lateMoves: 0,
    sampleCorrections: [],
  };
  const client = new ClientHarness(authority, result, config);

  let issued = false;
  for (let now = 0; now <= DURATION_MS; now += FRAME_MS) {
    // Advance the authority to its true current tick for this wall-clock time.
    authority.advanceTo(Math.floor(now / STEP_MS));
    // A continuous long move so the avatar is in motion for the whole window.
    if (!issued && now >= MOVE_AT_MS) {
      client.issueMove(260, 0);
      issued = true;
    }
    const deliveredTick = Math.floor((now - LATENCY_MS) / STEP_MS);
    client.frame(now, deliveredTick);
  }

  return result;
}

function rate(r: ScenarioResult): number {
  return r.movingSnapshots > 0 ? r.movingReconciles / r.movingSnapshots : 0;
}

function reasons(r: ScenarioResult): string {
  return `noPred=${r.reconcileNoPrediction} posErr=${r.reconcileError} targetMismatch=${r.reconcileTargetMismatch}`;
}

function report(label: string, r: ScenarioResult): void {
  // eslint-disable-next-line no-console
  console.log(
    `[repro] ${label.padEnd(16)} moving reconciles ${r.movingReconciles}/${r.movingSnapshots} ` +
      `(${(rate(r) * 100).toFixed(1)}%) maxCorrection=${r.maxCorrectionDistance.toFixed(3)} ` +
      `lateMoves=${r.lateMoves} | reasons: ${reasons(r)}`,
  );
}

test('client prediction stays in sync with the authority during continuous movement', () => {
  // The bug: client clock pinned to serverTick (zero lead) AND the authority
  // snapshot for tick T is processed before the frame records its prediction
  // for T -> hasPositionAt(T) is always false during movement -> a !hasPrediction
  // reconcile on every processed snapshot. The math is identical (posErr stays 0).
  const broken = runScenario({ leadTicks: 0, recordEveryTick: false, catchUpAfterSync: true });
  // Models the concurrent half-fix: per-tick recording inside the consume loop
  // only, no catch-up over setTick jumps, zero lead.
  const engineerHalfFix = runScenario({ leadTicks: 0, recordEveryTick: true, catchUpAfterSync: false });
  // Lead alone keeps serverTick in the recorded past, but per-frame recording
  // still drops the intermediate tick of any multi-tick (catch-up) frame.
  const leadOnly = runScenario({ leadTicks: PREDICTION_LEAD_TICKS, recordEveryTick: false, catchUpAfterSync: true });
  // Lead + per-tick recording + catch-up over sync jumps is the robust fix.
  const fixed = runScenario({ leadTicks: PREDICTION_LEAD_TICKS, recordEveryTick: true, catchUpAfterSync: true });

  // eslint-disable-next-line no-console
  console.log('');
  report('BROKEN', broken);
  for (const sample of broken.sampleCorrections) console.log(`  broken: ${sample}`);
  report('ENGINEER-HALFFIX', engineerHalfFix);
  report('LEAD-ONLY', leadOnly);
  report('LEAD+PERTICK', fixed);

  assert.equal(broken.lateMoves, 0, 'sanity: the move must not be late on the authority');
  assert.equal(fixed.lateMoves, 0, 'sanity: the move must not be late on the authority');

  // Documents the bug: the broken path reconciles on essentially every snapshot
  // while moving, and exclusively because the prediction sample is missing.
  assert.ok(rate(broken) > 0.8, `broken path should show the reconcile storm, got ${(rate(broken) * 100).toFixed(1)}%`);
  assert.equal(broken.reconcileError, 0, 'the desync is never a position-math error (rules out dt/speed/slide)');

  // The robust fix eliminates the movement-time reconcile storm.
  assert.ok(
    rate(fixed) < 0.05,
    `lead + per-tick recording should keep moving reconciles under 5%, got ${(rate(fixed) * 100).toFixed(1)}%`,
  );
});
