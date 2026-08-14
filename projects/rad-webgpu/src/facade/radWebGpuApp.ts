import {
  AvatarRenderer,
  type AvatarFrameReadback,
  type AvatarRendererOptions,
  type AvatarRenderSubmission,
} from '../presentation/avatarRenderer.js';
import type { AvatarPacketHeader } from '../presentation/contract.js';
import {
  WebGpuDeviceHost,
  type WebGpuDeviceHostOptions,
} from '../gpu/deviceHost.js';
import {
  WasmAvatarPresentationSource,
  type RadPresentationRuntime,
  type WasmAvatarSourceOptions,
} from '../runtime/wasmAvatarPresentationSource.js';

export interface RadWebGpuAppOptions {
  readonly source?: WasmAvatarSourceOptions;
  readonly renderer?: AvatarRendererOptions;
  readonly device?: WebGpuDeviceHostOptions;
}

/** High-level boundary between RAD publications and disposable GPU state. */
export class RadWebGpuApp {
  private destroyed = false;

  private constructor(
    readonly source: WasmAvatarPresentationSource,
    readonly deviceHost: WebGpuDeviceHost,
    readonly renderer: AvatarRenderer,
  ) {}

  static async create(
    canvas: HTMLCanvasElement,
    runtime: RadPresentationRuntime,
    memory: WebAssembly.Memory,
    options: RadWebGpuAppOptions = {},
  ): Promise<RadWebGpuApp> {
    const source = new WasmAvatarPresentationSource(runtime, memory, options.source);
    const deviceHost = await WebGpuDeviceHost.create(
      canvas,
      withPresentationBufferLimits(options.device, source),
    );
    try {
      const renderer = new AvatarRenderer(deviceHost, source.descriptor, {
        ...options.renderer,
        maxRecords: source.maxRecords,
      });
      return new RadWebGpuApp(source, deviceHost, renderer);
    } catch (error) {
      deviceHost.destroy();
      throw error;
    }
  }

  get deviceEpoch(): number {
    return this.deviceHost.session?.epoch ?? 0;
  }

  /** Publishes the current RAD state exactly once without drawing it. */
  publish(): AvatarPacketHeader {
    this.ensureActive();
    const packet = this.source.refresh();
    this.renderer.accept(packet);
    return packet.header;
  }

  /** Presents the latest publication without another RAD or WASM call. */
  renderLatest(): AvatarRenderSubmission | null {
    this.ensureActive();
    return this.renderer.renderLatest();
  }

  /** One-shot convenience for simple hosts. */
  render(): AvatarRenderSubmission | null {
    this.publish();
    return this.renderLatest();
  }

  /** Passively copies the target produced by the last successful render. */
  captureLastFrame(maxBytes: number): Promise<AvatarFrameReadback | null> {
    this.ensureActive();
    return this.renderer.captureLastFrame(maxBytes);
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.renderer.destroy();
    this.deviceHost.destroy();
  }

  private ensureActive(): void {
    if (this.destroyed) throw new Error('presentation.app_destroyed');
  }
}

function withPresentationBufferLimits(
  options: WebGpuDeviceHostOptions | undefined,
  source: WasmAvatarPresentationSource,
): WebGpuDeviceHostOptions {
  const requiredBytes = source.maxRecords
    * source.descriptor.recordWords
    * Uint32Array.BYTES_PER_ELEMENT;
  if (!Number.isSafeInteger(requiredBytes) || requiredBytes <= 0) {
    throw new Error('presentation.buffer_size_overflow');
  }
  const requested = options?.requiredLimits;
  return {
    ...options,
    requiredLimits: {
      ...requested,
      maxBufferSize: Math.max(requested?.maxBufferSize ?? 0, requiredBytes),
      maxStorageBufferBindingSize: Math.max(
        requested?.maxStorageBufferBindingSize ?? 0,
        requiredBytes,
      ),
    },
  };
}
