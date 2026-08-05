import { AvatarRenderer, type AvatarRendererOptions } from './avatarRenderer.js';
import {
  WebGpuDeviceHost,
  type WebGpuDeviceHostOptions,
} from './deviceHost.js';
import {
  WasmAvatarPresentationSource,
  type RadPresentationRuntime,
  type WasmAvatarSourceOptions,
} from './source.js';

export interface RadWebGpuAppOptions {
  readonly source?: WasmAvatarSourceOptions;
  readonly renderer?: AvatarRendererOptions;
  readonly device?: WebGpuDeviceHostOptions;
}

/** Convenience composition root; the source, GPU host, and renderer remain independently usable. */
export class RadWebGpuApp {
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
    const requiredBytes = source.maxRecords
      * source.descriptor.recordWords
      * Uint32Array.BYTES_PER_ELEMENT;
    const requestedLimits = options.device?.requiredLimits ?? {};
    const deviceHost = await WebGpuDeviceHost.create(canvas, {
      ...options.device,
      requiredLimits: {
        ...requestedLimits,
        maxBufferSize: Math.max(requestedLimits.maxBufferSize ?? 0, requiredBytes),
        maxStorageBufferBindingSize: Math.max(
          requestedLimits.maxStorageBufferBindingSize ?? 0,
          requiredBytes,
        ),
      },
    });
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

  render(): boolean {
    return this.renderer.render(this.source.refresh());
  }

  destroy(): void {
    this.renderer.destroy();
    this.deviceHost.destroy();
  }
}
