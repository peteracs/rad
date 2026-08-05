export interface WebGpuDeviceHostOptions {
  readonly powerPreference?: GPUPowerPreference;
  readonly requiredFeatures?: readonly GPUFeatureName[];
  readonly requiredLimits?: Record<string, number>;
  readonly alphaMode?: GPUCanvasAlphaMode;
  readonly maxDevicePixelRatio?: number;
  readonly onError?: (error: Error) => void;
}

export interface WebGpuDeviceSession {
  readonly device: GPUDevice;
  readonly context: GPUCanvasContext;
  readonly format: GPUTextureFormat;
  readonly epoch: number;
}

type SessionListener = (session: WebGpuDeviceSession) => void;

/** Owns browser GPU lifecycle. RAD state is never stored here. */
export class WebGpuDeviceHost {
  private sessionValue: WebGpuDeviceSession | null = null;
  private readonly listeners = new Set<SessionListener>();
  private resizeObserver: ResizeObserver | null = null;
  private destroyed = false;
  private epoch = 0;
  private recovery: Promise<void> | null = null;

  private constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly options: WebGpuDeviceHostOptions,
  ) {}

  static async create(
    canvas: HTMLCanvasElement,
    options: WebGpuDeviceHostOptions = {},
  ): Promise<WebGpuDeviceHost> {
    if (
      options.maxDevicePixelRatio !== undefined
      && (!Number.isFinite(options.maxDevicePixelRatio) || options.maxDevicePixelRatio <= 0)
    ) {
      throw new Error('webgpu.invalid_max_device_pixel_ratio');
    }
    const host = new WebGpuDeviceHost(canvas, options);
    await host.initialize();
    host.installResizeObserver();
    return host;
  }

  get session(): WebGpuDeviceSession | null {
    return this.sessionValue;
  }

  onSession(listener: SessionListener): () => void {
    if (this.sessionValue) listener(this.sessionValue);
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  resize(): void {
    const session = this.sessionValue;
    if (!session) return;
    const ratio = Math.min(
      globalThis.devicePixelRatio || 1,
      this.options.maxDevicePixelRatio ?? 2,
    );
    const max = session.device.limits.maxTextureDimension2D;
    const width = Math.max(1, Math.min(max, Math.round(this.canvas.clientWidth * ratio)));
    const height = Math.max(1, Math.min(max, Math.round(this.canvas.clientHeight * ratio)));
    if (this.canvas.width !== width) this.canvas.width = width;
    if (this.canvas.height !== height) this.canvas.height = height;
  }

  destroy(): void {
    this.destroyed = true;
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    const session = this.sessionValue;
    this.sessionValue = null;
    session?.context.unconfigure();
    session?.device.destroy();
    this.listeners.clear();
  }

  private async initialize(): Promise<void> {
    if (this.destroyed) return;
    if (!navigator.gpu) throw new Error('webgpu.unavailable');
    const adapter = await navigator.gpu.requestAdapter({
      powerPreference: this.options.powerPreference ?? 'high-performance',
    });
    if (!adapter) throw new Error('webgpu.adapter_unavailable');

    const requiredFeatures = [...(this.options.requiredFeatures ?? [])];
    for (const feature of requiredFeatures) {
      if (!adapter.features.has(feature)) throw new Error(`webgpu.feature_unavailable:${feature}`);
    }
    validateRequiredLimits(adapter, this.options.requiredLimits);
    const requiredLimits = this.options.requiredLimits;
    const device = await adapter.requestDevice({
      requiredFeatures,
      ...(requiredLimits ? { requiredLimits } : {}),
    });
    const context = this.canvas.getContext('webgpu');
    if (!context) {
      device.destroy();
      throw new Error('webgpu.canvas_context_unavailable');
    }
    const format = navigator.gpu.getPreferredCanvasFormat();
    context.configure({
      device,
      format,
      alphaMode: this.options.alphaMode ?? 'opaque',
    });
    const session = Object.freeze({ device, context, format, epoch: ++this.epoch });
    this.sessionValue = session;
    device.onuncapturederror = (event) => this.report(new Error(event.error.message));
    void device.lost.then((info) => this.handleLoss(device, info));
    this.resize();
    try {
      for (const listener of this.listeners) listener(session);
    } catch (error) {
      this.sessionValue = null;
      context.unconfigure();
      device.destroy();
      throw error;
    }
  }

  private handleLoss(device: GPUDevice, info: GPUDeviceLostInfo): void {
    if (this.destroyed || this.sessionValue?.device !== device) return;
    this.sessionValue = null;
    if (info.reason === 'destroyed') return;
    this.report(new Error(`webgpu.device_lost:${info.reason}:${info.message}`));
    if (!this.recovery) {
      this.recovery = this.recover().finally(() => {
        this.recovery = null;
      });
    }
  }

  private async recover(): Promise<void> {
    for (const delay of [0, 100, 500, 2_000]) {
      if (this.destroyed) return;
      if (delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));
      try {
        await this.initialize();
        return;
      } catch (error) {
        this.report(asError(error));
      }
    }
    this.report(new Error('webgpu.device_recovery_exhausted'));
  }

  private installResizeObserver(): void {
    if (typeof ResizeObserver === 'undefined') return;
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(this.canvas);
  }

  private report(error: Error): void {
    this.options.onError?.(error);
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function validateRequiredLimits(
  adapter: GPUAdapter,
  requiredLimits: Record<string, number> | undefined,
): void {
  for (const [name, required] of Object.entries(requiredLimits ?? {})) {
    if (!Number.isSafeInteger(required) || required < 0) {
      throw new Error(`webgpu.invalid_required_limit:${name}`);
    }
    const supported = Reflect.get(adapter.limits, name) as unknown;
    if (typeof supported !== 'number' || required > supported) {
      throw new Error(`webgpu.limit_unavailable:${name}`);
    }
  }
}
