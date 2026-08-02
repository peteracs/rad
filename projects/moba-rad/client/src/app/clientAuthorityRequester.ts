import {
  createAckDiagnosticsSnapshot,
  type AckDiagnostics,
} from '../netcode/ackDiagnostics.js';
import { NET_TICK_HZ } from '../netcode/constants.js';
import type { ClientInputSequencer } from '../netcode/inputSequencer';
import type { MatchTransport } from '../transport/matchTransport';
import type { ServerState } from '../transport/serverState';
import type { ClientNetcodeTelemetry } from './clientNetcodeTelemetry';

const AUTHORITY_POLL_MS = 100;

export class ClientAuthorityRequester {
  private readonly timingAckScratch = createAckDiagnosticsSnapshot();
  private authorityStateInFlight = false;
  private nextAuthorityPoll = 0;
  private closed = false;

  constructor(
    private readonly transport: MatchTransport,
    private readonly inputSequencer: ClientInputSequencer,
    private readonly ackDiagnostics: AckDiagnostics,
    private readonly telemetry: ClientNetcodeTelemetry,
  ) {}

  get inFlight(): boolean {
    return this.authorityStateInFlight;
  }

  close(): void {
    this.closed = true;
  }

  request(now: number): Promise<ServerState | null> | null {
    if (this.closed || this.authorityStateInFlight) return null;
    return this.runRequest(now);
  }

  maybeRequest(now: number, enabled: boolean): Promise<ServerState | null> | null {
    if (!enabled || now < this.nextAuthorityPoll) return null;
    return this.request(now);
  }

  private async runRequest(now: number): Promise<ServerState | null> {
    this.authorityStateInFlight = true;
    this.telemetry.noteAuthorityStateRequest();
    this.nextAuthorityPoll = now + AUTHORITY_POLL_MS;
    const started = performance.now();
    try {
      const state = await this.transport.state(this.inputSequencer.reserveClientSeq());
      this.telemetry.noteAuthorityRoundTrip(
        started,
        performance.now(),
        this.ackDiagnostics,
        NET_TICK_HZ,
        this.timingAckScratch,
      );
      return state;
    } catch (error) {
      this.telemetry.noteTransportFailure(error);
      return null;
    } finally {
      this.authorityStateInFlight = false;
    }
  }
}
