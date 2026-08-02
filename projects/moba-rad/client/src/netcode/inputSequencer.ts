export const MAX_CLIENT_SEQ = 2_000_000_000;

export interface ClientInputReservation {
  clientSeq: number;
  commandId: number;
  targetTick: number;
  predictionLeadTicks: number;
}

export function createClientInputReservation(): ClientInputReservation {
  return {
    clientSeq: 0,
    commandId: 0,
    targetTick: 0,
    predictionLeadTicks: 0,
  };
}

export class ClientInputSequencer {
  private commandId = 0;
  private clientSeq = 0;
  private lastInputTargetTick = 0;
  private sentInputs = 0;
  private leadTicks = 0;

  constructor(private readonly maxClientSeq = MAX_CLIENT_SEQ) {}

  get currentCommandId(): number {
    return this.commandId;
  }

  get inputPacketsSent(): number {
    return this.sentInputs;
  }

  get predictionLeadTicks(): number {
    return this.leadTicks;
  }

  reserveInputCommand(
    baseTick: number,
    recommendedDelayTicks: number,
    out: ClientInputReservation,
  ): ClientInputReservation {
    this.commandId += 1;
    out.commandId = this.commandId;
    out.clientSeq = this.reserveClientSeq();
    out.targetTick = this.reserveInputTargetTick(baseTick, recommendedDelayTicks);
    out.predictionLeadTicks = this.leadTicks;
    return out;
  }

  reserveClientSeq(): number {
    if (this.clientSeq >= this.maxClientSeq) {
      throw new Error('Client sequence exhausted; start a new match session');
    }
    this.clientSeq += 1;
    return this.clientSeq;
  }

  noteInputPacketSent(): void {
    this.sentInputs += 1;
  }

  private reserveInputTargetTick(baseTickValue: number, recommendedDelayTicksValue: number): number {
    const baseTick = Math.max(0, Math.trunc(baseTickValue));
    const recommendedDelayTicks = Math.max(0, Math.trunc(recommendedDelayTicksValue));
    const scheduledTick = Math.max(
      baseTick + recommendedDelayTicks,
      this.lastInputTargetTick + 1,
    );
    this.lastInputTargetTick = scheduledTick;
    this.leadTicks = scheduledTick - baseTick;
    return scheduledTick;
  }
}
