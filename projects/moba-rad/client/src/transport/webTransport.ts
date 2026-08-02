import {
  CAST_PACKET_BYTES,
  DISCONNECT_PACKET_BYTES,
  MOVE_PACKET_BYTES,
  SYNC_PACKET_BYTES,
  encodeCastPacket,
  encodeDisconnectPacket,
  encodeMoveOrderPacket,
  encodeSyncPacket,
  type MatchClientIdentity,
} from './matchProtocol';
import type { MatchTransport } from './matchTransport';
import type { ServerState } from './serverState';
import { WebTransportStateRouter } from './webTransportStateRouter';

const WEBTRANSPORT_URL_FROM_ENV = import.meta.env.VITE_MOBA_RAD_WEBTRANSPORT_URL;
// Target 127.0.0.1 explicitly rather than `localhost`: on Windows `localhost`
// resolves to ::1 (IPv6) first, but the edge proxy binds IPv4 127.0.0.1:4433, so
// a `localhost` URL sends QUIC to [::1]:4433 where nothing listens and the
// handshake dies with QUIC_NETWORK_IDLE_TIMEOUT. The loopback family must match
// the proxy's MOBA_RAD_WEBTRANSPORT_BIND.
const DEFAULT_WEBTRANSPORT_URL =
  typeof WEBTRANSPORT_URL_FROM_ENV === 'string' && WEBTRANSPORT_URL_FROM_ENV.length > 0
    ? WEBTRANSPORT_URL_FROM_ENV
    : 'https://127.0.0.1:4433/match';
// Pins the proxy's self-signed cert so Chromium accepts the QUIC handshake. The
// Vite dev server auto-derives this from the proxy's persisted cert file (see
// `webTransportCertHashPlugin` in vite.config.ts), so it is normally injected
// for you. An explicit env value overrides the auto-derived hash.
const DEFAULT_CERT_HASH_HEX = import.meta.env.VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH;

interface WebTransportDatagrams {
  readable: ReadableStream<Uint8Array>;
  writable: WritableStream<Uint8Array>;
}

interface WebTransportSession {
  readonly ready: Promise<void>;
  readonly closed: Promise<unknown>;
  readonly datagrams: WebTransportDatagrams;
  close?: (closeInfo?: { closeCode?: number; reason?: string }) => void;
}

interface WebTransportCertificateHash {
  algorithm: 'sha-256';
  value: BufferSource;
}

interface WebTransportOptions {
  allowPooling?: boolean;
  congestionControl?: 'default' | 'throughput' | 'low-latency';
  requireUnreliable?: boolean;
  serverCertificateHashes?: WebTransportCertificateHash[];
}

interface WebTransportConstructor {
  new(url: string, options?: WebTransportOptions): WebTransportSession;
}

// Production browser transport: WebTransport datagrams carry the same compact
// binary match packets as native UDP. The Rust edge proxy terminates
// HTTP/3/QUIC and forwards packet payloads to the RAD UDP authority; HTTP
// polling is not a supported path for this project. Future reliability work
// belongs in matchProtocol.ts and server/src/protocol/match_protocol.rad, not
// here. Input packets are sent as datagrams; parsed state packets are exposed
// as a bounded latest-state stream for the app frame loop.
export class MobaRadWebTransport implements MatchTransport {
  private transport: WebTransportSession | null = null;
  private writer: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private readonly movePacket = new Uint8Array(MOVE_PACKET_BYTES);
  private readonly castPacket = new Uint8Array(CAST_PACKET_BYTES);
  private readonly syncPacket = new Uint8Array(SYNC_PACKET_BYTES);
  private readonly disconnectPacket = new Uint8Array(DISCONNECT_PACKET_BYTES);
  private readonly stateRouter = new WebTransportStateRouter();
  private sendChain: Promise<void> = Promise.resolve();
  private readLoopStarted = false;
  private closed = false;

  constructor(
    private readonly identity: MatchClientIdentity,
    private readonly url = DEFAULT_WEBTRANSPORT_URL,
  ) {}

  async sendMoveOrder(
    clientSeq: number,
    targetTick: number,
    commandId: number,
    targetX: number,
    targetY: number,
  ): Promise<void> {
    await this.send(() => encodeMoveOrderPacket(
      this.identity,
      clientSeq,
      targetTick,
      commandId,
      targetX,
      targetY,
      this.movePacket,
    ));
  }

  async sendCast(
    clientSeq: number,
    targetTick: number,
    commandId: number,
    dirX: number,
    dirY: number,
    fireViewTick: number,
  ): Promise<void> {
    await this.send(() => encodeCastPacket(
      this.identity,
      clientSeq,
      targetTick,
      commandId,
      dirX,
      dirY,
      fireViewTick,
      this.castPacket,
    ));
  }

  latestState(): ServerState | null {
    return this.stateRouter.latestState();
  }

  droppedStateCount(): number {
    return this.stateRouter.droppedStateCount();
  }

  async state(clientSeq: number): Promise<ServerState> {
    if (this.closed) throw new Error('WebTransport match transport is closed');
    this.stateRouter.discardQueued();
    const response = this.stateRouter.nextAcknowledgedState(this.identity, clientSeq);
    try {
      await this.send(() => encodeSyncPacket(this.identity, clientSeq, this.syncPacket));
    } catch (error) {
      this.stateRouter.rejectWaiters(errorAsError(error));
      await response.catch(() => undefined);
      throw error;
    }
    return response;
  }

  async disconnect(clientSeq: number): Promise<void> {
    if (this.closed || !this.transport || !this.writer) return;
    await this.send(() => encodeDisconnectPacket(this.identity, clientSeq, this.disconnectPacket));
  }

  close(): void {
    this.closed = true;
    this.stateRouter.clear(new Error('WebTransport match transport closed'));

    const writer = this.writer;
    this.writer = null;
    if (writer) void writer.close().catch(() => {});

    this.transport?.close?.({ closeCode: 0, reason: 'moba-rad client closed' });
    this.transport = null;
    this.sendChain = Promise.resolve();
    this.readLoopStarted = false;
  }

  private send(encode: () => Uint8Array): Promise<void> {
    const write = this.sendChain.then(async () => {
      if (this.closed) throw new Error('WebTransport match transport is closed');
      await this.connect();
      const writer = this.writer;
      if (!writer) throw new Error('WebTransport datagram writer is not ready');
      await writer.write(encode());
    });
    this.sendChain = write.catch(() => {});
    return write;
  }

  private async connect(): Promise<void> {
    if (this.closed) throw new Error('WebTransport match transport is closed');
    if (this.transport && this.writer) return;

    const Transport = (globalThis as unknown as { WebTransport?: WebTransportConstructor })
      .WebTransport;
    if (!Transport) {
      throw new Error('WebTransport is not available in this browser');
    }

    const transport = new Transport(this.url, webTransportOptions());
    await transport.ready;
    if (this.closed) {
      transport.close?.({ closeCode: 0, reason: 'moba-rad client closed before ready' });
      throw new Error('WebTransport match transport is closed');
    }
    this.transport = transport;
    this.writer = transport.datagrams.writable.getWriter();
    this.startReadLoop(transport);
    void transport.closed.then(
      () => this.releaseTransport(transport, new Error('WebTransport match transport closed')),
      (error: unknown) => this.releaseTransport(
        transport,
        error instanceof Error ? error : new Error(String(error)),
      ),
    );
  }

  private startReadLoop(transport: WebTransportSession): void {
    if (this.readLoopStarted) return;
    this.readLoopStarted = true;

    void (async () => {
      const reader = transport.datagrams.readable.getReader();
      try {
        while (true) {
          const { value, done } = await reader.read();
          if (done) {
            this.releaseTransport(transport, new Error('WebTransport datagram stream ended'));
            return;
          }
          if (!value) continue;
          if (this.closed || this.transport !== transport) return;

          this.stateRouter.routePacket(value);
        }
      } catch (error) {
        this.releaseTransport(transport, error instanceof Error ? error : new Error(String(error)));
      }
    })();
  }

  private releaseTransport(transport: WebTransportSession, error: Error): void {
    if (this.transport !== transport) return;
    this.stateRouter.clear(error);
    this.writer = null;
    this.transport = null;
    this.readLoopStarted = false;
    this.sendChain = Promise.resolve();
  }
}

function errorAsError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function webTransportOptions(): WebTransportOptions {
  const options: WebTransportOptions = {
    allowPooling: false,
    congestionControl: 'low-latency',
    requireUnreliable: true,
  };
  const hash = certificateHashFromEnv(DEFAULT_CERT_HASH_HEX);
  if (hash) {
    options.serverCertificateHashes = [{ algorithm: 'sha-256', value: hash }];
  }
  return options;
}

function certificateHashFromEnv(value: unknown): ArrayBuffer | null {
  if (typeof value !== 'string' || value.trim() === '') return null;

  const hex = value.trim().replace(/[:\s]/g, '').toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(hex)) {
    throw new Error('VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH must be a SHA-256 hex digest');
  }

  const bytes: Uint8Array<ArrayBuffer> = new Uint8Array(new ArrayBuffer(32));
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes.buffer;
}
