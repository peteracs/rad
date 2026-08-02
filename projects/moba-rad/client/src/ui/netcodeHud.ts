import {
  createNetcodeDiagnosticsSnapshot,
  type NetcodeDiagnosticsSnapshot,
} from '../netcode/runtimeDiagnostics';

const HUD_UPDATE_MS = 250;
const TOGGLE_KEYS = new Set(['F3', 'Backquote']);

export interface NetcodeDiagnosticsSource {
  writeNetcodeDiagnostics(out: NetcodeDiagnosticsSnapshot): NetcodeDiagnosticsSnapshot;
}

export interface NetcodeHudOptions {
  // The minimal always-on indicator (ping/loss/state dot) shown to every player.
  mini?: HTMLElement | null;
  // Fired when the developer HUD is toggled, so the scene can mirror the same
  // visibility on its debug-only overlays (e.g. the server-position ghost).
  onDebugVisibilityChange?: (visible: boolean) => void;
}

export interface NetcodeHud {
  start(source: NetcodeDiagnosticsSource): void;
  stop(): void;
}

export function createNetcodeHud(root: HTMLElement | null, options: NetcodeHudOptions = {}): NetcodeHud {
  return new DomNetcodeHud(root, options);
}

class DomNetcodeHud implements NetcodeHud {
  private readonly snapshot = createNetcodeDiagnosticsSnapshot();
  private readonly authorityEl: HTMLElement | null;
  private readonly tickEl: HTMLElement | null;
  private readonly pingEl: HTMLElement | null;
  private readonly ackEl: HTMLElement | null;
  private readonly lossEl: HTMLElement | null;
  private readonly snapshotsEl: HTMLElement | null;
  private readonly correctionsEl: HTMLElement | null;
  private readonly resendsEl: HTMLElement | null;
  private readonly peersEl: HTMLElement | null;
  private readonly queueEl: HTMLElement | null;
  private readonly rejectsEl: HTMLElement | null;
  private readonly seqEl: HTMLElement | null;
  private readonly rosterEl: HTMLElement | null;
  private readonly poolsEl: HTMLElement | null;
  private readonly statusEl: HTMLElement | null;
  private readonly miniDotEl: HTMLElement | null;
  private readonly miniPingEl: HTMLElement | null;
  private readonly miniLossEl: HTMLElement | null;
  private readonly keyHandler = (event: KeyboardEvent) => this.onKeyDown(event);
  private intervalId = 0;
  private debugVisible = false;

  constructor(
    private readonly root: HTMLElement | null,
    private readonly options: NetcodeHudOptions = {},
  ) {
    this.miniDotEl = this.findIn(options.mini ?? null, 'dot');
    this.miniPingEl = this.findIn(options.mini ?? null, 'ping');
    this.miniLossEl = this.findIn(options.mini ?? null, 'loss');
    this.authorityEl = this.find('authority');
    this.tickEl = this.find('tick');
    this.pingEl = this.find('ping');
    this.ackEl = this.find('ack');
    this.lossEl = this.find('loss');
    this.snapshotsEl = this.find('snapshots');
    this.correctionsEl = this.find('corrections');
    this.resendsEl = this.find('resends');
    this.peersEl = this.find('peers');
    this.queueEl = this.find('queue');
    this.rejectsEl = this.find('rejects');
    this.seqEl = this.find('seq');
    this.rosterEl = this.find('roster');
    this.poolsEl = this.find('pools');
    this.statusEl = this.find('status');
  }

  start(source: NetcodeDiagnosticsSource): void {
    if (this.intervalId !== 0) return;
    window.addEventListener('keydown', this.keyHandler);
    this.applyDebugVisibility();
    this.write(source);
    this.intervalId = window.setInterval(() => this.write(source), HUD_UPDATE_MS);
  }

  stop(): void {
    window.removeEventListener('keydown', this.keyHandler);
    if (this.intervalId === 0) return;
    window.clearInterval(this.intervalId);
    this.intervalId = 0;
  }

  private onKeyDown(event: KeyboardEvent): void {
    if (event.repeat || !TOGGLE_KEYS.has(event.code)) return;
    event.preventDefault();
    this.debugVisible = !this.debugVisible;
    this.applyDebugVisibility();
  }

  private applyDebugVisibility(): void {
    this.root?.classList.toggle('netcode-hud--hidden', !this.debugVisible);
    this.options.onDebugVisibilityChange?.(this.debugVisible);
  }

  private write(source: NetcodeDiagnosticsSource): void {
    source.writeNetcodeDiagnostics(this.snapshot);
    this.writeMini();
    this.set(this.authorityEl, authorityText(this.snapshot));
    this.set(
      this.tickEl,
      `${this.snapshot.localTick} / ${this.snapshot.serverTickEstimate}`
        + ` lead ${this.snapshot.predictionLeadTicks}`,
    );
    this.set(
      this.pingEl,
      `${this.snapshot.roundTripMs.toFixed(0)}ms +/-${this.snapshot.jitterMs.toFixed(0)}ms`,
    );
    this.set(this.ackEl, String(this.snapshot.highestAck));
    this.set(this.lossEl, `${(this.snapshot.lossRatio * 100).toFixed(1)}%`);
    this.set(
      this.snapshotsEl,
      `${this.snapshot.acceptedStatePackets}/${this.snapshot.statePacketsReceived}`
        + ` stale ${this.snapshot.staleStatePackets}`
        + ` drop ${this.snapshot.droppedStatePackets}`,
    );
    this.set(
      this.correctionsEl,
      `${this.snapshot.reconciliationRatePerSecond.toFixed(1)}/s`
        + ` total ${this.snapshot.correctionCount}`
        + ` smooth ${this.snapshot.smoothedCorrectionCount}`,
    );
    this.set(
      this.resendsEl,
      `${this.snapshot.inputResendPackets} fail ${this.snapshot.transportFailures}`,
    );
    this.set(this.peersEl, `${this.snapshot.peerCount}/${this.snapshot.maxPeers} r ${this.snapshot.peerRecordCount}`);
    this.set(
      this.queueEl,
      `m ${this.snapshot.pendingMoveInputs} c ${this.snapshot.pendingCastInputs}`
        + ` / ${this.snapshot.inputQueueSlots}`,
    );
    this.set(
      this.rejectsEl,
      `l ${this.snapshot.lateInputs} f ${this.snapshot.futureInputs}`
        + ` d ${this.snapshot.duplicateInputs} o ${this.snapshot.overwrittenInputs}`,
    );
    this.set(
      this.seqEl,
      `${this.snapshot.lastAuthorityAppliedSeq} / ${this.snapshot.lastAuthorityClientSeq}`
        + ` ${this.snapshot.lastAuthorityAppliedAckBits.toString(16)}`,
    );
    this.set(
      this.rosterEl,
      `${this.snapshot.remoteAvatarCount}/${this.snapshot.avatarRecordCount}`
        + ` p ${this.snapshot.projectileRecordCount}`
        + ` i ${this.snapshot.projectileImpactRecordCount}`,
    );
    this.set(
      this.poolsEl,
      `a ${this.snapshot.remoteAvatarPoolActive}/${this.snapshot.remoteAvatarPoolIdle}`
        + ` p ${this.snapshot.projectilePoolActive}/${this.snapshot.projectilePoolIdle}`
        + ` fx ${this.snapshot.impactPoolActive}/${this.snapshot.impactPoolIdle}`,
    );
    this.set(
      this.statusEl,
      `${this.snapshot.lastAuthorityStatus} / ${this.snapshot.lastCorrectionReason}`
        + ` / ${this.snapshot.lastTransportError}`,
    );
  }

  private writeMini(): void {
    this.set(this.miniPingEl, `Ping: ${this.snapshot.roundTripMs.toFixed(0)}ms`);
    this.set(this.miniLossEl, `Loss: ${(this.snapshot.lossRatio * 100).toFixed(1)}%`);
    if (this.miniDotEl) {
      this.miniDotEl.className = `net-mini__dot net-mini__dot--${miniState(this.snapshot)}`;
    }
  }

  private find(name: string): HTMLElement | null {
    return this.findIn(this.root, name, 'netcode');
  }

  private findIn(root: HTMLElement | null, name: string, attr = 'mini'): HTMLElement | null {
    if (!root) return null;
    return root.querySelector<HTMLElement>(`[data-${attr}="${name}"]`);
  }

  private set(element: HTMLElement | null, value: string): void {
    if (element) element.textContent = value;
  }
}

function miniState(snapshot: NetcodeDiagnosticsSnapshot): 'live' | 'syncing' | 'offline' | 'idle' {
  if (snapshot.authoritySynced) return 'live';
  if (snapshot.authorityStateInFlight) return 'syncing';
  if (snapshot.transportFailures > 0) return 'offline';
  return 'idle';
}

function authorityText(snapshot: NetcodeDiagnosticsSnapshot): string {
  if (snapshot.authorityStateInFlight) return 'syncing';
  if (snapshot.authoritySynced) return snapshot.authorityMayBeMoving ? 'live moving' : 'live idle';
  if (snapshot.transportFailures > 0) return 'offline';
  return 'booting';
}
