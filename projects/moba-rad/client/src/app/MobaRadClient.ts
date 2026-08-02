import * as THREE from 'three';
import {
  AckDiagnostics,
  type AckDiagnosticsSnapshot,
} from '../netcode/ackDiagnostics';
import {
  INPUT_DELAY_TICKS,
  MAX_INPUT_DELAY_TICKS,
  PREDICTION_LEAD_TICKS,
  PROJECTILE_SPEED,
  REMOTE_INTERPOLATION_DELAY_TICKS,
} from '../netcode/constants';
import { FixedTickClock } from '../netcode/fixedTickClock';
import { ClientInputSequencer } from '../netcode/inputSequencer';
import { PredictionBuffer } from '../netcode/predictionBuffer';
import { NetcodeLogger, type NetcodeLoggerOptions } from '../netcode/netcodeLogger';
import type { NetcodeDiagnosticsSnapshot } from '../netcode/runtimeDiagnostics';
import type { RadGameSession } from '../radHost';
import { MobaRadScene, type ProjectileVisualState } from '../scene';
import {
  createAvatarRenderState,
  DEFAULT_SCENE,
  readSceneConfig,
} from '../render/worldView';
import type { MatchTransport } from '../transport/matchTransport';
import type { MatchClientIdentity } from '../transport/matchProtocol';
import type { ServerState } from '../transport/serverState';
import { ClientAuthorityApplier } from './clientAuthorityApplier';
import { ClientAuthorityRequester } from './clientAuthorityRequester';
import { ClientCommandDispatcher } from './clientCommandDispatcher';
import { ClientInputController } from './clientInputController';
import { ClientInputTransport } from './clientInputTransport';
import { ClientNetcodeTelemetry } from './clientNetcodeTelemetry';
import { ClientPredictionRunner } from './clientPredictionRunner';

const SKILLSHOT_AIM_RANGE = 48;

interface MobaRadClientOptions {
  canvas: HTMLCanvasElement;
  identity: MatchClientIdentity;
  session: RadGameSession;
  transport: MatchTransport;
  netcodeLogger?: NetcodeLoggerOptions;
}

export class MobaRadClient {
  private readonly scene: MobaRadScene;
  private readonly telemetry = new ClientNetcodeTelemetry();
  private readonly clock = new FixedTickClock();
  private readonly prediction = new PredictionBuffer();
  private readonly inputSequencer = new ClientInputSequencer();
  private readonly ackDiagnostics = new AckDiagnostics(INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS);
  private readonly authorityApplier: ClientAuthorityApplier;
  private readonly authorityRequester: ClientAuthorityRequester;
  private readonly inputTransport: ClientInputTransport;
  private readonly commandDispatcher: ClientCommandDispatcher;
  private readonly inputController: ClientInputController;
  private readonly predictionRunner: ClientPredictionRunner;
  private readonly pointerTarget = new THREE.Vector2();
  private readonly projectileScratch: ProjectileVisualState = {
    projectileId: 0,
    x: 0,
    y: 0,
    velocityX: 0,
    velocityY: 0,
  };
  private readonly localStateScratch = createAvatarRenderState();
  private readonly frameCallback = (frameNow: number) => this.frame(frameNow);
  private frameRequest = 0;
  private disposed = false;
  private lastFrameNow = 0;
  private projectileFrameDt = 0;
  private readonly netcodeLogger: NetcodeLogger | null;

  constructor(private readonly options: MobaRadClientOptions) {
    this.scene = new MobaRadScene(options.canvas);
    this.predictionRunner = new ClientPredictionRunner(
      options.session,
      options.identity.playerId,
      this.scene,
      this.prediction,
    );
    this.authorityApplier = new ClientAuthorityApplier(
      options.identity.sessionId,
      options.identity.playerId,
      this.scene,
      this.clock,
      this.prediction,
      this.inputSequencer,
      this.ackDiagnostics,
      this.predictionRunner,
      this.telemetry,
    );
    this.authorityRequester = new ClientAuthorityRequester(
      options.transport,
      this.inputSequencer,
      this.ackDiagnostics,
      this.telemetry,
    );
    this.inputTransport = new ClientInputTransport(
      options.transport,
      this.prediction,
      this.inputSequencer,
      this.authorityApplier,
      this.ackDiagnostics,
      this.telemetry,
    );
    this.commandDispatcher = new ClientCommandDispatcher(
      this.inputSequencer,
      this.ackDiagnostics,
      this.prediction,
      this.inputTransport,
    );
    this.inputController = new ClientInputController(options.canvas, {
      onResize: () => this.renderScene(),
      onMoveCommand: (clientX, clientY) => this.submitMoveAt(clientX, clientY),
      onAimPreview: (clientX, clientY) => this.updateSkillshotAimReticle(clientX, clientY),
      onAimCancel: () => this.scene.hideAimReticle(),
      onCastCommand: (clientX, clientY) => this.castLinearProjectile(clientX, clientY),
      onDebugToggle: (enabled) => this.scene.setDebugVisualsVisible(enabled),
    });
    this.netcodeLogger = options.netcodeLogger
      ? new NetcodeLogger(this, options.netcodeLogger)
      : null;
    this.scene.configure(DEFAULT_SCENE);
    this.renderScene();
  }

  start(now = performance.now()): void {
    this.scene.configure(readSceneConfig(this.predictionRunner.world.resources));
    this.clock.reset(now);
    this.lastFrameNow = now;
    this.predictionRunner.refreshSceneSample(this.clock.tick);
    this.renderScene(now);
    this.inputController.bind();
    this.requestAuthorityState(now);
    this.frameRequest = requestAnimationFrame(this.frameCallback);
  }

  writeNetworkDiagnostics(out: AckDiagnosticsSnapshot): AckDiagnosticsSnapshot {
    return this.ackDiagnostics.writeSnapshot(out);
  }

  writeNetcodeDiagnostics(out: NetcodeDiagnosticsSnapshot): NetcodeDiagnosticsSnapshot {
    this.ackDiagnostics.writeSnapshot(out);
    out.authoritySynced = this.authorityApplier.synced;
    out.authorityStateInFlight = this.authorityRequester.inFlight;
    out.authorityMayBeMoving = this.authorityApplier.authorityMayBeMoving;
    out.localSimulationActive = this.predictionRunner.active;
    out.localTick = this.clock.tick;
    out.serverTickEstimate = this.authorityApplier.serverTickEstimate;
    out.predictionLeadTicks = this.inputSequencer.predictionLeadTicks;
    this.authorityApplier.writeGateSnapshot(out);
    out.droppedStatePackets = this.options.transport.droppedStateCount();
    out.inputPacketsSent = this.inputSequencer.inputPacketsSent;
    this.telemetry.writeSnapshot(out, performance.now());
    this.scene.writeMeshPoolDiagnostics(out);
    return out;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.authorityRequester.close();
    this.inputTransport.close();
    this.inputController.dispose();
    if (this.frameRequest !== 0) cancelAnimationFrame(this.frameRequest);
    this.netcodeLogger?.close(performance.now());
    void this.options.transport
      .disconnect(this.inputSequencer.reserveClientSeq())
      .finally(() => this.options.transport.close());
    this.scene.dispose();
  }

  private submitMoveAt(clientX: number, clientY: number): void {
    if (!this.scene.writeWorldPointFromCanvas(clientX, clientY, this.pointerTarget)) {
      return;
    }

    this.commandDispatcher.queueMove(
      this.inputBaseTick(),
      this.pointerTarget.x,
      this.pointerTarget.y,
      performance.now(),
    );
    this.predictionRunner.markActive();
    this.authorityApplier.markAuthorityMayBeMoving();
  }

  private updateSkillshotAimReticle(clientX: number, clientY: number): void {
    if (!this.scene.writeWorldPointFromCanvas(clientX, clientY, this.pointerTarget)) {
      this.scene.hideAimReticle();
      return;
    }
    if (!this.predictionRunner.writeLocalAvatarState(this.localStateScratch)) {
      this.scene.hideAimReticle();
      return;
    }

    const dx = this.pointerTarget.x - this.localStateScratch.x;
    const dy = this.pointerTarget.y - this.localStateScratch.y;
    const mag = Math.hypot(dx, dy);
    if (mag <= 0.0001) {
      this.scene.hideAimReticle();
      return;
    }

    const length = Math.min(SKILLSHOT_AIM_RANGE, mag);
    const endX = this.localStateScratch.x + (dx / mag) * length;
    const endY = this.localStateScratch.y + (dy / mag) * length;
    this.scene.setAimReticle(this.localStateScratch.x, this.localStateScratch.y, endX, endY);
  }

  private castLinearProjectile(clientX: number, clientY: number): void {
    if (!this.authorityApplier.synced) {
      this.requestAuthorityState(performance.now());
    }
    if (!this.scene.writeWorldPointFromCanvas(clientX, clientY, this.pointerTarget)) {
      return;
    }
    if (!this.predictionRunner.writeLocalAvatarState(this.localStateScratch)) {
      return;
    }

    const dx = this.pointerTarget.x - this.localStateScratch.x;
    const dy = this.pointerTarget.y - this.localStateScratch.y;
    const mag = Math.hypot(dx, dy);
    if (mag <= 0.0001) return;

    const dirX = dx / mag;
    const dirY = dy / mag;
    const fireViewTick = Math.max(0, Math.trunc(this.remoteRenderTick()));
    const input = this.commandDispatcher.queueCast(
      this.inputBaseTick(),
      dirX,
      dirY,
      fireViewTick,
      performance.now(),
    );
    const projectileId = projectileIdFor(this.options.identity.playerId, input.commandId, input.targetTick);

    this.projectileScratch.projectileId = projectileId;
    this.projectileScratch.x = this.localStateScratch.x;
    this.projectileScratch.y = this.localStateScratch.y;
    this.projectileScratch.velocityX = dirX * PROJECTILE_SPEED;
    this.projectileScratch.velocityY = dirY * PROJECTILE_SPEED;
    this.scene.spawnPredictedProjectile(this.projectileScratch);
    this.authorityApplier.markAuthorityMayBeMoving();
  }

  private frame(now: number): void {
    if (this.disposed) return;
    this.projectileFrameDt = Math.min(0.05, Math.max(0, (now - this.lastFrameNow) / 1000));
    this.lastFrameNow = now;
    this.applyLatestAuthorityState();
    this.maybePollAuthority(now);
    this.inputTransport.maybeRetransmit(
      now,
      this.authorityApplier.synced,
      this.authorityApplier.serverTickEstimate,
    );

    const ticks = this.clock.consume(now);
    for (let i = 0; i < ticks; i += 1) this.clock.advanceOne();
    this.predictionRunner.advanceToTick(this.clock.tick);

    this.renderScene(now);

    this.netcodeLogger?.sample(now);
    this.frameRequest = requestAnimationFrame(this.frameCallback);
  }

  private renderScene(now = performance.now()): void {
    this.scene.render(this.clock.interpolationAlpha, this.remoteRenderTick(), now, this.projectileFrameDt);
  }

  private applyLatestAuthorityState(): void {
    const state = this.options.transport.latestState();
    if (state) this.applyAuthorityState(state);
  }

  private maybePollAuthority(now: number): void {
    if (this.disposed) return;
    const shouldPoll = this.predictionRunner.active || this.authorityApplier.authorityMayBeMoving;
    const request = this.authorityRequester.maybeRequest(now, shouldPoll);
    if (request) this.consumeAuthorityRequest(request);
  }

  private requestAuthorityState(now: number): void {
    const request = this.authorityRequester.request(now);
    if (request) this.consumeAuthorityRequest(request);
  }

  private consumeAuthorityRequest(request: Promise<ServerState | null>): void {
    void request
      .then((state) => {
        if (this.disposed) return;
        if (state) {
          this.applyAuthorityState(state);
          return;
        }
        this.authorityApplier.clearAuthorityMayBeMovingIfIdle(this.predictionRunner.active);
      })
      .catch((error: unknown) => {
        this.telemetry.noteTransportFailure(error);
        this.authorityApplier.clearAuthorityMayBeMovingIfIdle(this.predictionRunner.active);
      });
  }

  private applyAuthorityState(state: ServerState): void {
    if (this.disposed) return;
    const renderNow = this.authorityApplier.apply(state);
    if (renderNow > 0) {
      this.renderScene(renderNow);
    }
  }

  private remoteRenderTick(): number {
    // The local clock runs PREDICTION_LEAD_TICKS ahead of the authority; undo
    // that lead here so remote-avatar interpolation keeps its full delay buffer
    // and is paced against authoritative time rather than the predicted frontier.
    const estimatedServerTick = Math.max(
      this.authorityApplier.serverTickEstimate,
      this.clock.tick - PREDICTION_LEAD_TICKS,
    );
    return Math.max(0, estimatedServerTick - REMOTE_INTERPOLATION_DELAY_TICKS + this.clock.interpolationAlpha);
  }

  private inputBaseTick(): number {
    return Math.max(this.clock.tick, this.authorityApplier.serverTickEstimate);
  }
}

function projectileIdFor(playerId: number, commandId: number, spawnTick: number): number {
  if (commandId > 0) return playerId * 1_000_000 + commandId;
  return playerId * 1_000_000 + spawnTick;
}
