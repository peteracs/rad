import type { AckDiagnostics } from '../netcode/ackDiagnostics';
import { INPUT_RETRANSMIT_INTERVAL_MS } from '../netcode/constants.js';
import type { ClientInputSequencer } from '../netcode/inputSequencer';
import {
  PREDICTION_INPUT_KIND_CAST,
  PREDICTION_INPUT_KIND_MOVE,
  type PredictionInputSnapshot,
  type PredictionBuffer,
} from '../netcode/predictionBuffer.js';
import type { MatchTransport } from '../transport/matchTransport';
import type { ClientNetcodeTelemetry } from './clientNetcodeTelemetry';

export interface ReceiptAckBitsSource {
  lastReceiptAckBits: number;
}

export class ClientInputTransport {
  private readonly resendInputScratch: PredictionInputSnapshot = {
    kind: 0,
    tick: 0,
    clientSeq: 0,
    commandId: 0,
    targetX: 0,
    targetY: 0,
    dirX: 0,
    dirY: 0,
    fireViewTick: 0,
  };
  private nextInputRetransmit = 0;
  private closed = false;

  constructor(
    private readonly transport: MatchTransport,
    private readonly prediction: PredictionBuffer,
    private readonly inputSequencer: ClientInputSequencer,
    private readonly receiptAckBits: ReceiptAckBitsSource,
    private readonly ackDiagnostics: AckDiagnostics,
    private readonly telemetry: ClientNetcodeTelemetry,
  ) {}

  close(): void {
    this.closed = true;
  }

  scheduleRetransmit(now: number): void {
    this.nextInputRetransmit = now + INPUT_RETRANSMIT_INTERVAL_MS;
  }

  sendFreshMoveOrder(
    seq: number,
    targetTick: number,
    id: number,
    targetX: number,
    targetY: number,
  ): void {
    this.inputSequencer.noteInputPacketSent();
    this.sendMoveDatagram(seq, targetTick, id, targetX, targetY);
  }

  sendFreshCast(
    seq: number,
    targetTick: number,
    id: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
  ): void {
    this.inputSequencer.noteInputPacketSent();
    this.sendCastDatagram(seq, targetTick, id, dirX, dirY, fireViewTick);
  }

  maybeRetransmit(now: number, authoritySynced: boolean, serverTickEstimate: number): void {
    if (this.closed || !authoritySynced) return;
    if (now < this.nextInputRetransmit) return;
    this.nextInputRetransmit = now + INPUT_RETRANSMIT_INTERVAL_MS;

    if (!this.prediction.writeOldestUnackedInputAfter(
      this.ackDiagnostics.highestAckSeq(),
      this.receiptAckBits.lastReceiptAckBits,
      serverTickEstimate + 1,
      this.resendInputScratch,
    )) {
      return;
    }

    this.telemetry.noteInputResendPacket();
    this.inputSequencer.noteInputPacketSent();
    if (this.resendInputScratch.kind === PREDICTION_INPUT_KIND_MOVE) {
      this.sendMoveDatagram(
        this.resendInputScratch.clientSeq,
        this.resendInputScratch.tick,
        this.resendInputScratch.commandId,
        this.resendInputScratch.targetX,
        this.resendInputScratch.targetY,
      );
      return;
    }

    if (this.resendInputScratch.kind === PREDICTION_INPUT_KIND_CAST) {
      this.sendCastDatagram(
        this.resendInputScratch.clientSeq,
        this.resendInputScratch.tick,
        this.resendInputScratch.commandId,
        this.resendInputScratch.dirX,
        this.resendInputScratch.dirY,
        this.resendInputScratch.fireViewTick,
      );
    }
  }

  private sendMoveDatagram(
    seq: number,
    targetTick: number,
    id: number,
    targetX: number,
    targetY: number,
  ): void {
    if (this.closed) return;
    void this.transport
      .sendMoveOrder(seq, targetTick, id, targetX, targetY)
      .catch((error: unknown) => this.telemetry.noteTransportFailure(error));
  }

  private sendCastDatagram(
    seq: number,
    targetTick: number,
    id: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
  ): void {
    if (this.closed) return;
    void this.transport
      .sendCast(seq, targetTick, id, dirX, dirY, fireViewTick)
      .catch((error: unknown) => this.telemetry.noteTransportFailure(error));
  }
}
