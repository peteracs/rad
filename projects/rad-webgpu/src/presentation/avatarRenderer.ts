import type {
  WebGpuDeviceHost,
  WebGpuDeviceSession,
} from '../gpu/deviceHost.js';
import {
  AVATAR_FIELD_NAMES,
  AVATAR_HEADER_FIELD_NAMES,
  validateWordRanges,
  type AvatarPresentationDescriptor,
  type AvatarPresentationPacket,
  type WordRange,
} from './contract.js';
import {
  AvatarGpuResources,
  type AvatarGpuResourcesOptions,
} from './avatarGpuResources.js';
import type { AvatarFrameReadback } from './frameReadback.js';
import { PresentationLineage } from './lineage.js';

export type AvatarRendererOptions = AvatarGpuResourcesOptions;
export type { AvatarFrameReadback } from './frameReadback.js';

export interface AvatarRenderSubmission {
  readonly deviceEpoch: number;
  readonly submitted: boolean;
  readonly width: number;
  readonly height: number;
}

interface PacketIdentity {
  readonly streamId: bigint;
  readonly sequence: bigint;
}

interface RenderedIdentity extends PacketIdentity {
  readonly deviceEpoch: number;
  readonly width: number;
  readonly height: number;
}

/**
 * Accepts RAD publications and independently materializes the latest one.
 * Logical lineage survives device loss; every GPU resource does not.
 */
export class AvatarRenderer {
  private resources: AvatarGpuResources | null = null;
  private readonly lineage = new PresentationLineage();
  private readonly removeSessionListener: () => void;
  private latestPacket: AvatarPresentationPacket | null = null;
  private uploadedPacket: PacketIdentity | null = null;
  private renderedPacket: RenderedIdentity | null = null;
  private destroyed = false;

  constructor(
    private readonly host: WebGpuDeviceHost,
    private readonly descriptor: AvatarPresentationDescriptor,
    private readonly options: AvatarRendererOptions = {},
  ) {
    const session = host.session;
    if (!session) throw new Error('webgpu.renderer_requires_device_session');
    this.installDevice(session);
    this.removeSessionListener = host.onSession((next) => {
      if (this.resources?.epoch !== next.epoch) this.installDevice(next);
    });
  }

  /** Copies and accepts one bounded publication into the logical stream. */
  accept(packet: AvatarPresentationPacket): void {
    this.ensureActive();
    assertSameDescriptor(packet.descriptor, this.descriptor);
    assertPacketUpdateShape(packet);
    if (packet.dirtyRanges) validateWordRanges(packet.dirtyRanges, packet.records.length);

    const owned = ownPacket(packet);
    this.lineage.accept(owned.header);
    this.latestPacket = owned;
  }

  /** Materializes the latest accepted publication when identity or surface changed. */
  renderLatest(): AvatarRenderSubmission | null {
    this.ensureActive();
    const packet = this.latestPacket;
    if (!packet) throw new Error('presentation.no_accepted_frame');
    const session = this.host.session;
    if (!session) return null;

    const resources = this.resourcesFor(session);
    const identity = identityOf(packet);
    const surface = this.host.surfaceSize;
    const uploaded = identitiesMatch(this.uploadedPacket, identity);
    if (
      uploaded
      && this.renderedPacket?.deviceEpoch === session.epoch
      && this.renderedPacket.width === surface.width
      && this.renderedPacket.height === surface.height
    ) {
      return Object.freeze({
        deviceEpoch: session.epoch,
        submitted: false,
        width: surface.width,
        height: surface.height,
      });
    }

    try {
      if (!uploaded) {
        resources.uploadRecords(
          packet.records,
          dirtyRangesForGpuBaseline(packet, this.uploadedPacket),
        );
        this.uploadedPacket = identity;
      }
      const presented = resources.present(packet.header.count);
      this.renderedPacket = {
        ...identity,
        deviceEpoch: session.epoch,
        width: presented.width,
        height: presented.height,
      };
      return Object.freeze({
        deviceEpoch: session.epoch,
        submitted: true,
        width: presented.width,
        height: presented.height,
      });
    } catch (error) {
      resources.resetRecordBuffer();
      this.uploadedPacket = null;
      this.renderedPacket = null;
      throw error;
    }
  }

  /** One-shot convenience for hosts without separate update/render cadence. */
  render(packet: AvatarPresentationPacket): AvatarRenderSubmission | null {
    this.accept(packet);
    return this.renderLatest();
  }

  /** Passively copies the exact persistent target produced by the last render. */
  captureLastFrame(maxBytes: number): Promise<AvatarFrameReadback | null> {
    this.ensureActive();
    const session = this.host.session;
    if (
      !session
      || !this.renderedPacket
      || this.renderedPacket.deviceEpoch !== session.epoch
    ) {
      return Promise.resolve(null);
    }
    return this.resourcesFor(session).captureLastFrame(maxBytes);
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.removeSessionListener();
    this.lineage.reset();
    this.latestPacket = null;
    this.uploadedPacket = null;
    this.renderedPacket = null;
    this.destroyResources();
  }

  private resourcesFor(session: WebGpuDeviceSession): AvatarGpuResources {
    if (!this.resources || this.resources.epoch !== session.epoch) {
      this.installDevice(session);
    }
    const resources = this.resources;
    if (!resources) throw new Error('webgpu.renderer_resources_unavailable');
    return resources;
  }

  private installDevice(session: WebGpuDeviceSession): void {
    this.destroyResources();
    this.uploadedPacket = null;
    this.renderedPacket = null;
    this.resources = new AvatarGpuResources(session, this.descriptor, this.options);
  }

  private destroyResources(): void {
    this.resources?.destroy();
    this.resources = null;
  }

  private ensureActive(): void {
    if (this.destroyed) throw new Error('webgpu.renderer_destroyed');
  }
}

function ownPacket(packet: AvatarPresentationPacket): AvatarPresentationPacket {
  const words = packet.words.slice();
  const dirtyRanges = packet.dirtyRanges?.map((range) => Object.freeze({ ...range }));
  return Object.freeze({
    words,
    records: words.subarray(packet.descriptor.headerWords),
    header: packet.header,
    descriptor: packet.descriptor,
    ...(dirtyRanges ? { dirtyRanges: Object.freeze(dirtyRanges) } : {}),
  });
}

function dirtyRangesForGpuBaseline(
  packet: AvatarPresentationPacket,
  uploaded: PacketIdentity | null,
): readonly WordRange[] | undefined {
  return packet.header.packetKind === 'delta'
    && uploaded?.streamId === packet.header.streamId
    && uploaded.sequence === packet.header.baseSequence
    ? packet.dirtyRanges
    : undefined;
}

function identityOf(packet: AvatarPresentationPacket): PacketIdentity {
  return Object.freeze({
    streamId: packet.header.streamId,
    sequence: packet.header.sequence,
  });
}

function identitiesMatch(left: PacketIdentity | null, right: PacketIdentity): boolean {
  return left?.streamId === right.streamId && left.sequence === right.sequence;
}

function assertPacketUpdateShape(packet: AvatarPresentationPacket): void {
  if (packet.header.packetKind === 'full' && packet.dirtyRanges !== undefined) {
    throw new Error('presentation.full_packet_has_dirty_ranges');
  }
  if (packet.header.packetKind === 'delta' && packet.dirtyRanges === undefined) {
    throw new Error('presentation.delta_packet_missing_dirty_ranges');
  }
}

function assertSameDescriptor(
  actual: AvatarPresentationDescriptor,
  expected: AvatarPresentationDescriptor,
): void {
  if (
    actual.magic !== expected.magic
    || actual.version !== expected.version
    || actual.headerWords !== expected.headerWords
    || actual.recordWords !== expected.recordWords
    || actual.supportedFlags !== expected.supportedFlags
    || actual.packetKinds.full !== expected.packetKinds.full
    || actual.packetKinds.delta !== expected.packetKinds.delta
    || AVATAR_HEADER_FIELD_NAMES.some(
      (name) => actual.headerFields[name] !== expected.headerFields[name],
    )
    || AVATAR_FIELD_NAMES.some((name) => actual.fields[name] !== expected.fields[name])
  ) {
    throw new Error('presentation.renderer_descriptor_mismatch');
  }
}
