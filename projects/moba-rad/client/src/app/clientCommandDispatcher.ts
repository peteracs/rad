import type { AckDiagnostics } from '../netcode/ackDiagnostics';
import {
  createClientInputReservation,
  type ClientInputReservation,
  type ClientInputSequencer,
} from '../netcode/inputSequencer.js';
import type { PredictionBuffer } from '../netcode/predictionBuffer';
import type { ClientInputTransport } from './clientInputTransport';

export class ClientCommandDispatcher {
  private readonly inputReservationScratch = createClientInputReservation();

  constructor(
    private readonly inputSequencer: ClientInputSequencer,
    private readonly ackDiagnostics: AckDiagnostics,
    private readonly prediction: PredictionBuffer,
    private readonly inputTransport: ClientInputTransport,
  ) {}

  queueMove(
    baseTick: number,
    targetX: number,
    targetY: number,
    nowMs: number,
  ): ClientInputReservation {
    const input = this.inputSequencer.reserveInputCommand(
      baseTick,
      this.ackDiagnostics.recommendedDelayTicks(),
      this.inputReservationScratch,
    );
    this.prediction.recordMoveInput(
      input.targetTick,
      input.clientSeq,
      input.commandId,
      targetX,
      targetY,
    );
    this.inputTransport.scheduleRetransmit(nowMs);
    this.inputTransport.sendFreshMoveOrder(
      input.clientSeq,
      input.targetTick,
      input.commandId,
      targetX,
      targetY,
    );
    return input;
  }

  queueCast(
    baseTick: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
    nowMs: number,
  ): ClientInputReservation {
    const input = this.inputSequencer.reserveInputCommand(
      baseTick,
      this.ackDiagnostics.recommendedDelayTicks(),
      this.inputReservationScratch,
    );
    this.prediction.recordCastInput(
      input.targetTick,
      input.clientSeq,
      input.commandId,
      dirX,
      dirY,
      fireViewTick,
    );
    this.inputTransport.scheduleRetransmit(nowMs);
    this.inputTransport.sendFreshCast(
      input.clientSeq,
      input.targetTick,
      input.commandId,
      dirX,
      dirY,
      fireViewTick,
    );
    return input;
  }
}
