import type { AvatarPacketHeader } from './contract.js';

/**
 * Validates logical publication order independently from disposable GPU state.
 * Device loss never rewinds the accepted RAD stream.
 */
export class PresentationLineage {
  private streamId: bigint | null = null;
  private sequence: bigint | null = null;

  accept(header: AvatarPacketHeader): void {
    const newStream = this.streamId === null || header.streamId !== this.streamId;
    if (newStream) {
      if (header.packetKind !== 'full') {
        throw new Error('presentation.new_stream_requires_full');
      }
      if (header.sequence !== 0n) {
        throw new Error('presentation.new_stream_sequence_not_zero');
      }
      this.commit(header);
      return;
    }

    const previous = this.sequence;
    if (previous === null) throw new Error('presentation.lineage_missing_sequence');
    if (header.sequence <= previous) throw new Error('presentation.stale_sequence');
    if (header.packetKind === 'delta') {
      if (header.baseSequence !== previous) {
        throw new Error('presentation.delta_base_mismatch');
      }
      if (header.sequence !== previous + 1n) {
        throw new Error('presentation.delta_sequence_gap');
      }
    }
    this.commit(header);
  }

  reset(): void {
    this.streamId = null;
    this.sequence = null;
  }

  private commit(header: AvatarPacketHeader): void {
    this.streamId = header.streamId;
    this.sequence = header.sequence;
  }
}
