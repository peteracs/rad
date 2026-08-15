import {
  parseServerStatePacket,
  type MatchClientIdentity,
} from './matchProtocol';
import type { ServerState } from './serverState';
import {
  copyServerState,
  createServerStateBuffer,
} from './serverStateBuffer';
import { SERVER_STATE_INBOX_CAPACITY, ServerStateInbox } from './stateInbox';

const DEFAULT_STATE_TIMEOUT_MS = 1000;
const STATE_PARSE_POOL_SIZE = SERVER_STATE_INBOX_CAPACITY * 2;
const MAX_SYNC_WAITERS = 4;

interface StateWaiter {
  resolve: (state: ServerState) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
  accepts: (state: ServerState) => boolean;
  state: ServerState;
}

export class WebTransportStateRouter {
  private readonly inbox = new ServerStateInbox();
  private readonly stateParsePool = Array.from(
    { length: STATE_PARSE_POOL_SIZE },
    () => createServerStateBuffer(),
  );
  private readonly syncWaiterStatePool = Array.from(
    { length: MAX_SYNC_WAITERS },
    () => createServerStateBuffer(),
  );
  private readonly waiters: StateWaiter[] = [];
  private stateParsePoolIndex = 0;
  private syncWaiterStatePoolIndex = 0;

  latestState(): ServerState | null {
    return this.inbox.takeLatest();
  }

  droppedStateCount(): number {
    return this.inbox.droppedCount;
  }

  discardQueued(): void {
    this.inbox.discardQueued();
  }

  clear(error = new Error('WebTransport state router closed')): void {
    this.rejectWaiters(error);
    this.inbox.clear();
  }

  rejectWaiters(error: Error): void {
    while (this.waiters.length > 0) {
      const waiter = this.waiters.shift();
      if (!waiter) continue;
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
  }

  nextAcknowledgedState(
    identity: MatchClientIdentity,
    clientSeq: number,
    timeoutMs = DEFAULT_STATE_TIMEOUT_MS,
  ): Promise<ServerState> {
    return this.nextState(
      timeoutMs,
      (state) => state.session_id === identity.sessionId
        && state.player_id === identity.playerId
        && stateAcknowledgesClientSeq(state, clientSeq),
    );
  }

  routePacket(packet: Uint8Array): void {
    const state = parseServerStatePacket(packet, this.stateParsePool[this.stateParsePoolIndex]);
    if (!state) return;

    this.stateParsePoolIndex += 1;
    if (this.stateParsePoolIndex >= this.stateParsePool.length) {
      this.stateParsePoolIndex = 0;
    }
    this.pushState(state);
  }

  private nextState(
    timeoutMs: number,
    accepts: (state: ServerState) => boolean,
  ): Promise<ServerState> {
    if (this.waiters.length >= MAX_SYNC_WAITERS) {
      return Promise.reject(new Error('Too many pending WebTransport sync waiters'));
    }

    return new Promise((resolve, reject) => {
      const waiter: StateWaiter = {
        resolve,
        reject,
        accepts,
        state: this.nextSyncWaiterState(),
        timer: setTimeout(() => {
          this.removeWaiter(waiter);
          reject(new Error('Timed out waiting for WebTransport state packet'));
        }, timeoutMs),
      };
      this.waiters.push(waiter);
    });
  }

  private pushState(state: ServerState): void {
    for (let i = 0; i < this.waiters.length; i += 1) {
      const waiter = this.waiters[i];
      if (!waiter.accepts(state)) continue;

      this.waiters.splice(i, 1);
      clearTimeout(waiter.timer);
      waiter.resolve(copyServerState(state, waiter.state));
      return;
    }

    this.inbox.push(state);
  }

  private nextSyncWaiterState(): ServerState {
    const state = this.syncWaiterStatePool[this.syncWaiterStatePoolIndex];
    this.syncWaiterStatePoolIndex += 1;
    if (this.syncWaiterStatePoolIndex >= this.syncWaiterStatePool.length) {
      this.syncWaiterStatePoolIndex = 0;
    }
    return state;
  }

  private removeWaiter(waiter: StateWaiter): void {
    const index = this.waiters.indexOf(waiter);
    if (index >= 0) this.waiters.splice(index, 1);
  }
}

function stateAcknowledgesClientSeq(state: ServerState, clientSeq: number): boolean {
  const seq = Math.trunc(clientSeq);
  const highest = Math.trunc(state.ack_client_seq);
  if (seq <= 0 || highest <= 0 || seq > highest) return false;

  const offset = highest - seq;
  if (offset >= 32) return false;
  return (((Math.trunc(state.ack_bits) >>> 0) >>> offset) & 1) === 1;
}
