import type {
  AvatarPresentationDescriptor,
  AvatarPresentationPacket,
} from './contract.js';
import { AVATAR_FIELD_NAMES, AVATAR_HEADER_FIELD_NAMES } from './contract.js';
import { GpuBufferMirror } from './bufferMirror.js';
import {
  WebGpuDeviceHost,
  type WebGpuDeviceSession,
} from './deviceHost.js';
import { PresentationLineage } from './lineage.js';

export interface AvatarRendererOptions {
  readonly worldWidth?: number;
  readonly worldHeight?: number;
  readonly avatarRadius?: number;
  readonly clearColor?: GPUColor;
  readonly maxRecords?: number;
}

export interface AvatarFrameReadback {
  readonly width: number;
  readonly height: number;
  readonly bytesPerRow: number;
  readonly format: GPUTextureFormat;
  readonly pixels: Uint8Array;
}

interface DeviceResources {
  readonly epoch: number;
  readonly records: GpuBufferMirror;
  readonly uniform: GPUBuffer;
  readonly pipeline: GPURenderPipeline;
  bindGroup: GPUBindGroup | null;
  boundRecords: GPUBuffer | null;
}

interface ReadbackSubmission {
  readonly buffer: GPUBuffer;
  readonly width: number;
  readonly height: number;
  readonly bytesPerRow: number;
  readonly format: GPUTextureFormat;
}

/** Materializes avatar presentation records without owning simulation state. */
export class AvatarRenderer {
  private resources: DeviceResources | null = null;
  private readonly removeSessionListener: () => void;
  private readonly lineage = new PresentationLineage();
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

  render(packet: AvatarPresentationPacket): boolean {
    return this.submit(packet) !== null;
  }

  /** Renders and copies that exact canvas texture into a bounded CPU-visible buffer. */
  async readback(
    packet: AvatarPresentationPacket,
    maxBytes: number,
  ): Promise<AvatarFrameReadback | null> {
    if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
      throw new Error('webgpu.invalid_readback_limit');
    }
    const submission = this.submit(packet, maxBytes);
    if (!submission) return null;
    let mapped = false;
    try {
      await submission.buffer.mapAsync(GPUMapMode.READ);
      mapped = true;
      return Object.freeze({
        width: submission.width,
        height: submission.height,
        bytesPerRow: submission.bytesPerRow,
        format: submission.format,
        pixels: new Uint8Array(submission.buffer.getMappedRange()).slice(),
      });
    } finally {
      if (mapped) submission.buffer.unmap();
      submission.buffer.destroy();
    }
  }

  private submit(packet: AvatarPresentationPacket): true | null;
  private submit(packet: AvatarPresentationPacket, readbackLimit: number): ReadbackSubmission | null;
  private submit(
    packet: AvatarPresentationPacket,
    readbackLimit?: number,
  ): ReadbackSubmission | true | null {
    if (this.destroyed) throw new Error('webgpu.renderer_destroyed');
    assertSameDescriptor(packet.descriptor, this.descriptor);
    const session = this.host.session;
    if (!session) return null;
    if (readbackLimit !== undefined && !session.canvasReadbackEnabled) {
      throw new Error('webgpu.canvas_readback_not_enabled');
    }
    if (!this.resources || this.resources.epoch !== session.epoch) this.installDevice(session);
    const resources = this.resources;
    if (!resources) return null;
    if (packet.header.packetKind === 'full' && packet.dirtyRanges !== undefined) {
      throw new Error('presentation.full_packet_has_dirty_ranges');
    }
    if (packet.header.packetKind === 'delta' && packet.dirtyRanges === undefined) {
      throw new Error('presentation.delta_packet_missing_dirty_ranges');
    }

    const adoption = this.lineage.inspect(packet.header);
    if (adoption.resetMirror) {
      resources.records.destroy();
      resources.bindGroup = null;
      resources.boundRecords = null;
    }
    let readback: ReadbackSubmission | null = null;
    try {
      const recordsBuffer = resources.records.upload(packet.records, packet.dirtyRanges);
      if (resources.boundRecords !== recordsBuffer) {
        resources.bindGroup = session.device.createBindGroup({
          label: 'RAD avatar presentation bindings',
          layout: resources.pipeline.getBindGroupLayout(0),
          entries: [
            { binding: 0, resource: { buffer: recordsBuffer } },
            { binding: 1, resource: { buffer: resources.uniform } },
          ],
        });
        resources.boundRecords = recordsBuffer;
      }

      const texture = session.context.getCurrentTexture();
      const textureView = texture.createView();
      const encoder = session.device.createCommandEncoder({ label: 'RAD avatar frame' });
      const pass = encoder.beginRenderPass({
        label: 'RAD avatar pass',
        colorAttachments: [{
          view: textureView,
          clearValue: this.options.clearColor ?? { r: 0.025, g: 0.035, b: 0.07, a: 1 },
          loadOp: 'clear',
          storeOp: 'store',
        }],
      });
      if (packet.header.count > 0 && resources.bindGroup) {
        pass.setPipeline(resources.pipeline);
        pass.setBindGroup(0, resources.bindGroup);
        pass.draw(6, packet.header.count);
      }
      pass.end();
      readback = readbackLimit === undefined
        ? null
        : createReadbackSubmission(session, texture, encoder, readbackLimit);
      session.device.queue.submit([encoder.finish()]);
      this.lineage.commit(packet.header);
      return readback ?? true;
    } catch (error) {
      readback?.buffer.destroy();
      resources.records.destroy();
      resources.bindGroup = null;
      resources.boundRecords = null;
      this.lineage.invalidateBaseline();
      throw error;
    }
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.removeSessionListener();
    this.lineage.reset();
    this.destroyResources();
  }

  private installDevice(session: WebGpuDeviceSession): void {
    this.destroyResources();
    this.lineage.invalidateBaseline();
    const maxRecords = this.options.maxRecords ?? this.descriptor.defaultMaxRecords;
    if (!Number.isInteger(maxRecords) || maxRecords <= 0) {
      throw new Error('webgpu.invalid_avatar_record_limit');
    }
    if (maxRecords > this.descriptor.hardMaxRecords) {
      throw new Error('webgpu.avatar_record_limit_exceeds_runtime');
    }
    const requestedBytes = maxRecords * this.descriptor.recordWords * Uint32Array.BYTES_PER_ELEMENT;
    const maxBytes = Math.min(
      requestedBytes,
      Number(session.device.limits.maxBufferSize),
      Number(session.device.limits.maxStorageBufferBindingSize),
    );
    if (maxBytes < this.descriptor.recordWords * Uint32Array.BYTES_PER_ELEMENT) {
      throw new Error('webgpu.avatar_storage_limit_too_small');
    }

    let records: GpuBufferMirror | null = null;
    let uniform: GPUBuffer | null = null;
    try {
      records = new GpuBufferMirror(session.device, {
        label: 'RAD avatar presentation records',
        usage: GPUBufferUsage.STORAGE,
        maxBytes,
      });
      uniform = session.device.createBuffer({
        label: 'RAD avatar view uniform',
        size: 16,
        usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
      });
      session.device.queue.writeBuffer(uniform, 0, new Float32Array([
        positive(this.options.worldWidth ?? 200, 'world_width'),
        positive(this.options.worldHeight ?? 120, 'world_height'),
        positive(this.options.avatarRadius ?? 3.5, 'avatar_radius'),
        0,
      ]));
      const module = session.device.createShaderModule({
        label: 'RAD avatar shader',
        code: avatarShader(this.descriptor),
      });
      const pipeline = session.device.createRenderPipeline({
        label: 'RAD avatar pipeline',
        layout: 'auto',
        vertex: { module, entryPoint: 'vertex_main' },
        fragment: { module, entryPoint: 'fragment_main', targets: [{ format: session.format }] },
        primitive: { topology: 'triangle-list' },
      });
      this.resources = {
        epoch: session.epoch,
        records,
        uniform,
        pipeline,
        bindGroup: null,
        boundRecords: null,
      };
    } catch (error) {
      records?.destroy();
      uniform?.destroy();
      throw error;
    }
  }

  private destroyResources(): void {
    this.resources?.records.destroy();
    this.resources?.uniform.destroy();
    this.resources = null;
  }
}

function avatarShader(descriptor: AvatarPresentationDescriptor): string {
  const field = descriptor.fields;
  return /* wgsl */ `
struct ViewUniform {
  world_width: f32,
  world_height: f32,
  radius: f32,
  _padding: f32,
}

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec3<f32>,
}

@group(0) @binding(0) var<storage, read> records: array<u32>;
@group(0) @binding(1) var<uniform> view: ViewUniform;

const RECORD_WORDS: u32 = ${descriptor.recordWords}u;
const X_WORD: u32 = ${field.x}u;
const Y_WORD: u32 = ${field.y}u;
const PLAYER_WORD: u32 = ${field.player_id}u;
const MODEL_WORD: u32 = ${field.model_id}u;

@vertex
fn vertex_main(
  @builtin(vertex_index) vertex_index: u32,
  @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
  let corners = array<vec2<f32>, 6>(
    vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
  );
  let base = instance_index * RECORD_WORDS;
  let center = vec2(
    bitcast<f32>(records[base + X_WORD]),
    bitcast<f32>(records[base + Y_WORD]),
  );
  let half_world = vec2(view.world_width, view.world_height) * 0.5;
  let clip_center = vec2(center.x / half_world.x, -center.y / half_world.y);
  let clip_radius = vec2(view.radius / half_world.x, view.radius / half_world.y);
  let player = records[base + PLAYER_WORD];
  let model = records[base + MODEL_WORD];
  let palette = array<vec3<f32>, 6>(
    vec3(0.31, 0.64, 1.0), vec3(1.0, 0.38, 0.53), vec3(0.61, 0.91, 0.42),
    vec3(0.88, 0.68, 0.29), vec3(0.73, 0.53, 0.96), vec3(0.33, 0.85, 0.78),
  );
  var output: VertexOutput;
  output.position = vec4(clip_center + corners[vertex_index] * clip_radius, 0.0, 1.0);
  output.color = palette[(player + model) % 6u];
  return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
  return vec4(input.color, 1.0);
}
`;
}

function createReadbackSubmission(
  session: WebGpuDeviceSession,
  texture: GPUTexture,
  encoder: GPUCommandEncoder,
  maxBytes: number,
): ReadbackSubmission {
  if (!session.canvasReadbackEnabled) throw new Error('webgpu.canvas_readback_not_enabled');
  const width = Number(texture.width);
  const height = Number(texture.height);
  const unpaddedBytesPerRow = checkedProduct(width, 4, 'readback_row');
  const bytesPerRow = alignTo(unpaddedBytesPerRow, 256);
  const byteLength = checkedProduct(bytesPerRow, height, 'readback_size');
  if (byteLength > maxBytes || byteLength > Number(session.device.limits.maxBufferSize)) {
    throw new Error('webgpu.readback_limit_exceeded');
  }
  const buffer = session.device.createBuffer({
    label: 'RAD avatar frame readback',
    size: byteLength,
    usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
  });
  try {
    encoder.copyTextureToBuffer(
      { texture },
      { buffer, bytesPerRow, rowsPerImage: height },
      { width, height, depthOrArrayLayers: 1 },
    );
    return { buffer, width, height, bytesPerRow, format: session.format };
  } catch (error) {
    buffer.destroy();
    throw error;
  }
}

function checkedProduct(left: number, right: number, name: string): number {
  const product = left * right;
  if (!Number.isSafeInteger(product) || product <= 0) {
    throw new Error(`webgpu.invalid_${name}`);
  }
  return product;
}

function alignTo(value: number, alignment: number): number {
  return Math.ceil(value / alignment) * alignment;
}

function assertSameDescriptor(
  actual: AvatarPresentationDescriptor,
  expected: AvatarPresentationDescriptor,
): void {
  if (
    actual.magic !== expected.magic ||
    actual.version !== expected.version ||
    actual.headerWords !== expected.headerWords ||
    actual.recordWords !== expected.recordWords ||
    actual.supportedFlags !== expected.supportedFlags ||
    actual.packetKinds.full !== expected.packetKinds.full ||
    actual.packetKinds.delta !== expected.packetKinds.delta ||
    AVATAR_HEADER_FIELD_NAMES.some(
      (name) => actual.headerFields[name] !== expected.headerFields[name],
    ) ||
    AVATAR_FIELD_NAMES.some((name) => actual.fields[name] !== expected.fields[name])
  ) {
    throw new Error('presentation.renderer_descriptor_mismatch');
  }
}

function positive(value: number, name: string): number {
  if (!Number.isFinite(value) || value <= 0) throw new Error(`webgpu.invalid_${name}`);
  return value;
}
