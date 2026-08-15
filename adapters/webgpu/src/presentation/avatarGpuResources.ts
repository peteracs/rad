import { GpuBufferMirror } from '../gpu/bufferMirror.js';
import type { WebGpuDeviceSession } from '../gpu/deviceHost.js';
import type { AvatarPresentationDescriptor, WordRange } from './contract.js';
import { AvatarRenderTarget } from './avatarRenderTarget.js';
import { createAvatarShader } from './avatarShader.js';
import {
  readTexturePixels,
  type AvatarFrameReadback,
  type NormalizedRgbaColor,
} from './frameReadback.js';

export interface AvatarGpuResourcesOptions {
  readonly worldWidth?: number;
  readonly worldHeight?: number;
  readonly avatarRadius?: number;
  readonly clearColor?: GPUColor;
  readonly maxRecords?: number;
}

export interface PresentedSurface {
  readonly width: number;
  readonly height: number;
}

/** Owns every disposable avatar resource for one WebGPU device epoch. */
export class AvatarGpuResources {
  readonly epoch: number;
  readonly clearColor: NormalizedRgbaColor;

  private readonly records: GpuBufferMirror;
  private readonly uniform: GPUBuffer;
  private readonly pipeline: GPURenderPipeline;
  private readonly target: AvatarRenderTarget;
  private bindGroup: GPUBindGroup | null = null;
  private boundRecords: GPUBuffer | null = null;

  constructor(
    private readonly session: WebGpuDeviceSession,
    descriptor: AvatarPresentationDescriptor,
    options: AvatarGpuResourcesOptions = {},
  ) {
    this.epoch = session.epoch;
    this.clearColor = normalizeColor(options.clearColor);
    const maxBytes = resolveRecordBufferLimit(session.device, descriptor, options.maxRecords);

    let records: GpuBufferMirror | null = null;
    let uniform: GPUBuffer | null = null;
    try {
      records = new GpuBufferMirror(session.device, {
        label: 'RAD avatar presentation records',
        usage: GPUBufferUsage.STORAGE,
        maxBytes,
      });
      uniform = createViewUniform(session.device, options);
      const shader = session.device.createShaderModule({
        label: 'RAD avatar shader',
        code: createAvatarShader(descriptor),
      });
      this.pipeline = session.device.createRenderPipeline({
        label: 'RAD avatar pipeline',
        layout: 'auto',
        vertex: { module: shader, entryPoint: 'vertex_main' },
        fragment: {
          module: shader,
          entryPoint: 'fragment_main',
          targets: [{ format: session.format }],
        },
        primitive: { topology: 'triangle-list' },
      });
      this.records = records;
      this.uniform = uniform;
      this.target = new AvatarRenderTarget(session.device, session.format);
    } catch (error) {
      records?.destroy();
      uniform?.destroy();
      throw error;
    }
  }

  uploadRecords(words: Uint32Array, dirtyRanges?: readonly WordRange[]): void {
    const buffer = this.records.upload(words, dirtyRanges);
    if (this.boundRecords === buffer) return;
    this.bindGroup = this.session.device.createBindGroup({
      label: 'RAD avatar presentation bindings',
      layout: this.pipeline.getBindGroupLayout(0),
      entries: [
        { binding: 0, resource: { buffer } },
        { binding: 1, resource: { buffer: this.uniform } },
      ],
    });
    this.boundRecords = buffer;
  }

  /** Draws once to the persistent target, then copies that exact image to canvas. */
  present(recordCount: number): PresentedSurface {
    const canvasTexture = this.session.context.getCurrentTexture();
    const width = Number(canvasTexture.width);
    const height = Number(canvasTexture.height);
    const target = this.target.ensure(width, height);
    const encoder = this.session.device.createCommandEncoder({
      label: 'RAD avatar presentation',
    });
    const pass = encoder.beginRenderPass({
      label: 'RAD avatar pass',
      colorAttachments: [{
        view: target.texture.createView(),
        clearValue: this.clearColor,
        loadOp: 'clear',
        storeOp: 'store',
      }],
    });
    if (recordCount > 0 && this.bindGroup) {
      pass.setPipeline(this.pipeline);
      pass.setBindGroup(0, this.bindGroup);
      pass.draw(6, recordCount);
    }
    pass.end();
    this.target.encodeCopyToCanvas(encoder, canvasTexture);
    this.session.device.queue.submit([encoder.finish()]);
    return Object.freeze({ width, height });
  }

  captureLastFrame(maxBytes: number): Promise<AvatarFrameReadback | null> {
    const target = this.target.snapshot;
    return target
      ? readTexturePixels(this.session.device, target, this.clearColor, maxBytes)
      : Promise.resolve(null);
  }

  resetRecordBuffer(): void {
    this.records.destroy();
    this.bindGroup = null;
    this.boundRecords = null;
  }

  destroy(): void {
    this.records.destroy();
    this.uniform.destroy();
    this.target.destroy();
    this.bindGroup = null;
    this.boundRecords = null;
  }
}

function resolveRecordBufferLimit(
  device: GPUDevice,
  descriptor: AvatarPresentationDescriptor,
  requestedRecords: number | undefined,
): number {
  const records = requestedRecords ?? descriptor.defaultMaxRecords;
  if (!Number.isInteger(records) || records <= 0) {
    throw new Error('webgpu.invalid_avatar_record_limit');
  }
  if (records > descriptor.hardMaxRecords) {
    throw new Error('webgpu.avatar_record_limit_exceeds_runtime');
  }
  const requestedBytes = records * descriptor.recordWords * Uint32Array.BYTES_PER_ELEMENT;
  if (!Number.isSafeInteger(requestedBytes) || requestedBytes <= 0) {
    throw new Error('presentation.buffer_size_overflow');
  }
  const maxBytes = Math.min(
    requestedBytes,
    Number(device.limits.maxBufferSize),
    Number(device.limits.maxStorageBufferBindingSize),
  );
  if (maxBytes < descriptor.recordWords * Uint32Array.BYTES_PER_ELEMENT) {
    throw new Error('webgpu.avatar_storage_limit_too_small');
  }
  return maxBytes;
}

function createViewUniform(
  device: GPUDevice,
  options: AvatarGpuResourcesOptions,
): GPUBuffer {
  const uniform = device.createBuffer({
    label: 'RAD avatar view uniform',
    size: 16,
    usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
  });
  try {
    device.queue.writeBuffer(uniform, 0, new Float32Array([
      positive(options.worldWidth ?? 200, 'world_width'),
      positive(options.worldHeight ?? 120, 'world_height'),
      positive(options.avatarRadius ?? 3.5, 'avatar_radius'),
      0,
    ]));
    return uniform;
  } catch (error) {
    uniform.destroy();
    throw error;
  }
}

function normalizeColor(color: GPUColor | undefined): NormalizedRgbaColor {
  if (color === undefined) {
    return Object.freeze({ r: 0.025, g: 0.035, b: 0.07, a: 1 });
  }
  const sequence = Symbol.iterator in Object(color) ? Array.from(color as Iterable<number>) : null;
  const value = sequence
    ? { r: sequence[0] ?? 0, g: sequence[1] ?? 0, b: sequence[2] ?? 0, a: sequence[3] ?? 1 }
    : color as GPUColorDict;
  return Object.freeze({
    r: colorComponent(value.r, 'r'),
    g: colorComponent(value.g, 'g'),
    b: colorComponent(value.b, 'b'),
    a: colorComponent(value.a, 'a'),
  });
}

function colorComponent(value: number, name: string): number {
  if (!Number.isFinite(value) || value < 0 || value > 1) {
    throw new Error(`webgpu.invalid_clear_color_${name}`);
  }
  return value;
}

function positive(value: number, name: string): number {
  if (!Number.isFinite(value) || value <= 0) throw new Error(`webgpu.invalid_${name}`);
  return value;
}
