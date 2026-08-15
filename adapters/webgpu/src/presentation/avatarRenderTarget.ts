export interface AvatarRenderTargetSnapshot {
  readonly texture: GPUTexture;
  readonly width: number;
  readonly height: number;
  readonly format: GPUTextureFormat;
}

/** Owns the persistent color target produced by the avatar pass. */
export class AvatarRenderTarget {
  private texture: GPUTexture | null = null;
  private width = 0;
  private height = 0;

  constructor(
    private readonly device: GPUDevice,
    private readonly format: GPUTextureFormat,
  ) {}

  get snapshot(): AvatarRenderTargetSnapshot | null {
    if (!this.texture) return null;
    return Object.freeze({
      texture: this.texture,
      width: this.width,
      height: this.height,
      format: this.format,
    });
  }

  ensure(width: number, height: number): AvatarRenderTargetSnapshot {
    validateDimension(width, 'width');
    validateDimension(height, 'height');
    if (!this.matches(width, height)) {
      const replacement = this.device.createTexture({
        label: 'RAD avatar presentation color target',
        size: { width, height },
        format: this.format,
        usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
      });
      this.texture?.destroy();
      this.texture = replacement;
      this.width = width;
      this.height = height;
    }
    const snapshot = this.snapshot;
    if (!snapshot) throw new Error('webgpu.avatar_render_target_unavailable');
    return snapshot;
  }

  encodeCopyToCanvas(
    encoder: GPUCommandEncoder,
    canvasTexture: GPUTexture,
  ): void {
    const source = this.snapshot;
    if (!source) throw new Error('webgpu.avatar_render_target_unavailable');
    encoder.copyTextureToTexture(
      { texture: source.texture },
      { texture: canvasTexture },
      { width: source.width, height: source.height, depthOrArrayLayers: 1 },
    );
  }

  destroy(): void {
    this.texture?.destroy();
    this.texture = null;
    this.width = 0;
    this.height = 0;
  }

  private matches(width: number, height: number): boolean {
    return this.texture !== null && this.width === width && this.height === height;
  }
}

function validateDimension(value: number, name: string): void {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`webgpu.invalid_render_target_${name}`);
  }
}
