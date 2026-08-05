export type RadWebGpuRequiredLimitName =
  | 'maxBufferSize'
  | 'maxStorageBufferBindingSize';

export type RadWebGpuRequiredLimits = Readonly<
  Partial<Record<RadWebGpuRequiredLimitName, number>>
>;

export interface WebGpuDeviceHostOptions {
  readonly powerPreference?: GPUPowerPreference;
  readonly requiredFeatures?: readonly GPUFeatureName[];
  readonly requiredLimits?: RadWebGpuRequiredLimits;
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
  private lifecycle = 0;
  private epoch = 0;
  private recovery: Promise<void> | null = null;
  private recoveryRequested = false;

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
    try {
      if (!await host.initialize(host.lifecycle)) {
        throw new Error('webgpu.host_destroyed_during_initialization');
      }
      host.installResizeObserver();
      return host;
    } catch (error) {
      host.destroy();
      throw error;
    }
  }

  get session(): WebGpuDeviceSession | null {
    return this.sessionValue;
  }

  onSession(listener: SessionListener): () => void {
    this.listeners.add(listener);
    if (this.sessionValue) this.notifyListener(listener, this.sessionValue);
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
    if (this.destroyed) return;
    this.destroyed = true;
    this.lifecycle += 1;
    this.recoveryRequested = false;
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    const session = this.sessionValue;
    this.sessionValue = null;
    session?.context.unconfigure();
    session?.device.destroy();
    this.listeners.clear();
  }

  private async initialize(attempt: number): Promise<boolean> {
    if (!this.isCurrent(attempt)) return false;
    if (!navigator.gpu) throw new Error('webgpu.unavailable');
    const adapter = await navigator.gpu.requestAdapter({
      powerPreference: this.options.powerPreference ?? 'high-performance',
    });
    if (!this.isCurrent(attempt)) return false;
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
    if (!this.isCurrent(attempt)) {
      device.destroy();
      return false;
    }

    const context = this.canvas.getContext('webgpu');
    if (!context) {
      device.destroy();
      throw new Error('webgpu.canvas_context_unavailable');
    }
    const format = navigator.gpu.getPreferredCanvasFormat();
    try {
      context.configure({
        device,
        format,
        alphaMode: this.options.alphaMode ?? 'opaque',
      });
    } catch (error) {
      device.destroy();
      throw error;
    }
    if (!this.isCurrent(attempt)) {
      context.unconfigure();
      device.destroy();
      return false;
    }

    const session = Object.freeze({ device, context, format, epoch: ++this.epoch });
    this.sessionValue = session;
    device.onuncapturederror = (event) => this.report(new Error(event.error.message));
    void device.lost.then((info) => this.handleLoss(device, info));
    this.resize();
    for (const listener of this.listeners) this.notifyListener(listener, session);

    // Let an already-resolved `device.lost` callback invalidate this session
    // before a recovery attempt reports success.
    await Promise.resolve();
    return this.isCurrent(attempt) && this.sessionValue?.device === device;
  }

  private handleLoss(device: GPUDevice, info: GPUDeviceLostInfo): void {
    if (this.destroyed || this.sessionValue?.device !== device) return;
    this.sessionValue.context.unconfigure();
    this.sessionValue = null;
    this.report(new Error(`webgpu.device_lost:${info.reason}:${info.message}`));
    this.requestRecovery();
  }

  private requestRecovery(): void {
    if (this.destroyed) return;
    this.recoveryRequested = true;
    if (this.recovery) return;
    this.recovery = this.recoverUntilStable().finally(() => {
      this.recovery = null;
      if (this.recoveryRequested && !this.destroyed && !this.sessionValue) {
        this.requestRecovery();
      }
    });
  }

  private async recoverUntilStable(): Promise<void> {
    while (!this.destroyed && this.recoveryRequested) {
      this.recoveryRequested = false;
      const attempt = this.lifecycle;
      let installed = false;
      for (const delay of [0, 100, 500, 2_000]) {
        if (!this.isCurrent(attempt)) return;
        if (delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));
        if (!this.isCurrent(attempt)) return;
        this.recoveryRequested = false;
        try {
          installed = await this.initialize(attempt);
          if (installed) break;
        } catch (error) {
          this.report(asError(error));
        }
      }
      if (installed && this.sessionValue && !this.recoveryRequested) return;
      if (!installed && !this.recoveryRequested) {
        this.report(new Error('webgpu.device_recovery_exhausted'));
        return;
      }
    }
  }

  private isCurrent(attempt: number): boolean {
    return !this.destroyed && attempt === this.lifecycle;
  }

  private installResizeObserver(): void {
    if (typeof ResizeObserver === 'undefined') return;
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(this.canvas);
  }

  private notifyListener(listener: SessionListener, session: WebGpuDeviceSession): void {
    try {
      listener(session);
    } catch (error) {
      this.report(new Error('webgpu.session_listener_failed', { cause: error }));
    }
  }

  private report(error: Error): void {
    try {
      this.options.onError?.(error);
    } catch {
      // Observers never own or interrupt the GPU lifecycle.
    }
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function validateRequiredLimits(
  adapter: GPUAdapter,
  requiredLimits: RadWebGpuRequiredLimits | undefined,
): void {
  for (const [name, required] of Object.entries(requiredLimits ?? {})) {
    if (name !== 'maxBufferSize' && name !== 'maxStorageBufferBindingSize') {
      throw new Error(`webgpu.unsupported_required_limit:${name}`);
    }
    if (!Number.isSafeInteger(required) || required < 0) {
      throw new Error(`webgpu.invalid_required_limit:${name}`);
    }
    const supported = adapter.limits[name];
    if (required > supported) throw new Error(`webgpu.limit_unavailable:${name}`);
  }
}
