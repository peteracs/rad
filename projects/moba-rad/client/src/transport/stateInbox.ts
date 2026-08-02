import type { ServerState } from './serverState';

export const SERVER_STATE_INBOX_CAPACITY = 8;

export class ServerStateInbox {
  private readonly states: Array<ServerState | null>;
  private head = 0;
  private count = 0;
  private dropped = 0;

  constructor(private readonly capacity = SERVER_STATE_INBOX_CAPACITY) {
    if (capacity <= 0) {
      throw new Error('ServerStateInbox capacity must be positive');
    }
    this.states = new Array<ServerState | null>(capacity).fill(null);
  }

  get size(): number {
    return this.count;
  }

  get droppedCount(): number {
    return this.dropped;
  }

  push(state: ServerState): void {
    if (this.count === this.capacity) {
      this.states[this.head] = state;
      this.head = this.next(this.head);
      this.dropped += 1;
      return;
    }

    const index = (this.head + this.count) % this.capacity;
    this.states[index] = state;
    this.count += 1;
  }

  takeLatest(): ServerState | null {
    if (this.count === 0) return null;

    const latestIndex = (this.head + this.count - 1) % this.capacity;
    const state = this.states[latestIndex];
    this.dropped += this.count - 1;
    this.clear();
    return state;
  }

  discardQueued(): void {
    this.dropped += this.count;
    this.clear();
  }

  clear(): void {
    let index = this.head;
    for (let i = 0; i < this.count; i += 1) {
      this.states[index] = null;
      index = this.next(index);
    }
    this.head = 0;
    this.count = 0;
  }

  private next(index: number): number {
    const value = index + 1;
    return value === this.capacity ? 0 : value;
  }
}
