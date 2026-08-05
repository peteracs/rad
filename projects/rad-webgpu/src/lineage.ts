import type { AvatarPacketHeader } from './contract.js';

export interface PresentationAdoption {
  /** The existing GPU mirror cannot be used as this packet's baseline. */
  readonly resetMirror: boolean;
}

/**
 * Validates packet lineage independently from rendering. Full packets may
 * skip sequences; deltas must extend the exact accepted baseline.
 */
export class PresentationLineage {
  private streamId: bigint | null = null;
  private sequence: bigint | null = null;
  private baselineValid = false;

  inspect(header: AvatarPacketHeader): PresentationAdoption {
    const newStream = this.streamId === null || header.streamId !== this.streamId;
    if (newStream) {
      if (header.packetKind !== 'full') throw new Error('presentation.new_stream_requires_full');
      if (header.sequence !== 0n) throw new Error('presentation.new_stream_sequence_not_zero');
      return Object.freeze({ resetMirror: true });
    }

    const previous = this.sequence;
    if (previous !== null && header.sequence <= previous) {
      throw new Error('presentation.stale_sequence');
    }
    if (header.packetKind === 'full') {
      return Object.freeze({ resetMirror: !this.baselineValid });
    }
    if (!this.baselineValid || previous === null) {
      throw new Error('presentation.delta_without_baseline');
    }
    if (header.baseSequence !== previous) {
      throw new Error('presentation.delta_base_mismatch');
    }
    if (header.sequence !== previous + 1n) {
      throw new Error('presentation.delta_sequence_gap');
    }
    return Object.freeze({ resetMirror: false });
  }

  commit(header: AvatarPacketHeader): void {
    this.streamId = header.streamId;
    this.sequence = header.sequence;
    this.baselineValid = true;
  }

  invalidateBaseline(): void {
    this.baselineValid = false;
  }

  reset(): void {
    this.streamId = null;
    this.sequence = null;
    this.baselineValid = false;
  }
}
