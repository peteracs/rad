import type { ServerState } from './serverState';

// Game code depends on this interface only. Browser builds use WebTransport;
// native tools can implement the same contract over raw UDP without touching
// prediction, rendering, or RAD session code. Inputs are datagram sends;
// authoritative snapshots are consumed as a latest-state stream.
export interface MatchTransport {
  sendMoveOrder(
    clientSeq: number,
    targetTick: number,
    commandId: number,
    targetX: number,
    targetY: number,
  ): Promise<void>;
  sendCast(
    clientSeq: number,
    targetTick: number,
    commandId: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
  ): Promise<void>;
  latestState(): ServerState | null;
  droppedStateCount(): number;
  // Sync/poll only: implementations should wait for a state whose receipt ACK
  // covers clientSeq, not return an arbitrary snapshot from the stream backlog.
  state(clientSeq: number): Promise<ServerState>;
  disconnect(clientSeq: number): Promise<void>;
  close(): void;
}
