export interface AuthorityStateGateInput {
  ok: boolean;
  session_id: number;
  player_id: number;
  server_tick: number;
  server_seq: number;
  ack_client_seq: number;
  ack_bits: number;
  status: string;
  correction_reason: string;
  avatar: {
    x: number;
    y: number;
    target_x: number;
    target_y: number;
    target_active: boolean;
    command_id: number;
  };
}

export interface AuthorityStateGateSnapshot {
  lastServerSeq: number;
  statePacketsReceived: number;
  acceptedStatePackets: number;
  staleStatePackets: number;
  rejectedStatePackets: number;
  lastAuthorityStatus: string;
  lastCorrectionReason: string;
}

export function createAuthorityStateGateSnapshot(): AuthorityStateGateSnapshot {
  return {
    lastServerSeq: 0,
    statePacketsReceived: 0,
    acceptedStatePackets: 0,
    staleStatePackets: 0,
    rejectedStatePackets: 0,
    lastAuthorityStatus: 'none',
    lastCorrectionReason: 'none',
  };
}

export class AuthorityStateGate {
  private lastSeq = 0;
  private packetsReceived = 0;
  private packetsAccepted = 0;
  private packetsStale = 0;
  private packetsRejected = 0;
  private status = 'none';
  private correctionReason = 'none';
  private receiptAckBits = 0;

  constructor(
    private readonly sessionId: number,
    private readonly playerId: number,
  ) {}

  get lastServerSeq(): number {
    return this.lastSeq;
  }

  get lastReceiptAckBits(): number {
    return this.receiptAckBits;
  }

  accept(state: AuthorityStateGateInput, highestAckSeqValue: number): boolean {
    this.packetsReceived += 1;
    this.status = state.status;
    this.correctionReason = state.correction_reason;

    const serverSeq = Math.trunc(state.server_seq);
    const ackClientSeq = Math.trunc(state.ack_client_seq);
    const ackBits = Math.trunc(state.ack_bits);
    if (
      !state.ok ||
      state.session_id !== this.sessionId ||
      state.player_id !== this.playerId ||
      !isU32Whole(state.server_tick) ||
      !isU32Whole(state.server_seq) ||
      !isU32Whole(state.ack_client_seq) ||
      !isU32Whole(state.ack_bits) ||
      !Number.isFinite(state.avatar.x) ||
      !Number.isFinite(state.avatar.y) ||
      !Number.isFinite(state.avatar.target_x) ||
      !Number.isFinite(state.avatar.target_y) ||
      typeof state.avatar.target_active !== 'boolean' ||
      !isU32Whole(state.avatar.command_id)
    ) {
      this.packetsRejected += 1;
      return false;
    }

    if (serverSeq <= this.lastSeq) {
      this.packetsStale += 1;
      return false;
    }

    this.packetsAccepted += 1;
    this.lastSeq = serverSeq;
    const highestAckSeq = isU32Whole(highestAckSeqValue)
      ? Math.trunc(highestAckSeqValue)
      : 0;
    if (ackClientSeq >= highestAckSeq) {
      this.receiptAckBits = ackBits >>> 0;
    }
    return true;
  }

  writeSnapshot<T extends AuthorityStateGateSnapshot>(out: T): T {
    out.lastServerSeq = this.lastSeq;
    out.statePacketsReceived = this.packetsReceived;
    out.acceptedStatePackets = this.packetsAccepted;
    out.staleStatePackets = this.packetsStale;
    out.rejectedStatePackets = this.packetsRejected;
    out.lastAuthorityStatus = this.status;
    out.lastCorrectionReason = this.correctionReason;
    return out;
  }
}

const MAX_U32 = 0xffffffff;

function isU32Whole(value: number): boolean {
  return Number.isFinite(value) && Math.trunc(value) === value && value >= 0 && value <= MAX_U32;
}
