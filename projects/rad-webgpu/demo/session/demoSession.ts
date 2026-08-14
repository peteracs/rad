import type { RadRuntime } from '../../../../core/vm/pkg/rad_vm.js';
import type {
  AvatarPresentationPacket,
  RadWebGpuApp,
} from '../../src/index.js';
import { countChangedPixels } from '../testing/frameAnalysis.js';

const READBACK_LIMIT_BYTES = 16 * 1024 * 1024;

export interface DemoCapture {
  readonly changedPixels: number;
  readonly height: number;
  readonly recordCount: number;
  readonly width: number;
}

export interface DemoSnapshot {
  readonly canvasWidth: number;
  readonly deviceEpoch: number;
  readonly errors: readonly string[];
  readonly recordCount: number;
  readonly renderedFrames: number;
  readonly sequence: string;
  readonly streamId: string;
}

export class DemoSession {
  private renderedFrames = 0;
  private recordCount = 0;
  private streamId = 0n;
  private sequence = 0n;
  private destroyed = false;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly runtime: RadRuntime,
    private readonly app: RadWebGpuApp,
    private readonly source: string,
    private readonly errors: string[],
  ) {}

  recordError(message: string): void {
    this.errors.push(message);
  }

  advanceSimulation(dt: number): void {
    this.assertAlive();
    this.runtime.session_emit('Tick', JSON.stringify({ dt }));
    this.runtime.session_pump();
    this.recordHeader(this.app.publish());
  }

  publishCurrentWorld(): void {
    this.assertAlive();
    this.recordHeader(this.app.publish());
  }

  renderLatestPresentation(): number | null {
    this.assertAlive();
    const submission = this.app.renderLatest();
    if (!submission) return null;
    if (submission.submitted) this.renderedFrames += 1;
    return submission.deviceEpoch;
  }

  async capture(): Promise<DemoCapture> {
    this.assertAlive();

    const readback = await this.app.captureLastFrame(READBACK_LIMIT_BYTES);

    if (!readback) {
      throw new Error('webgpu.readback_device_unavailable');
    }

    return {
      changedPixels: countChangedPixels(readback),
      height: readback.height,
      recordCount: this.recordCount,
      width: readback.width,
    };
  }

  loseDevice(): void {
    this.assertAlive();
    this.app.deviceHost.session?.device.destroy();
  }

  restart(): void {
    this.assertAlive();
    this.runtime.session_start(this.source);
    this.publishCurrentWorld();
  }

  snapshot(): DemoSnapshot {
    this.assertAlive();

    return {
      canvasWidth: this.canvas.width,
      deviceEpoch: this.app.deviceHost.session?.epoch ?? 0,
      errors: [...this.errors],
      recordCount: this.recordCount,
      renderedFrames: this.renderedFrames,
      sequence: this.sequence.toString(),
      streamId: this.streamId.toString(),
    };
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;

    this.app.destroy();
    this.runtime.free();
  }

  private recordHeader(header: AvatarPresentationPacket['header']): void {
    this.recordCount = header.count;
    this.streamId = header.streamId;
    this.sequence = header.sequence;
  }

  private assertAlive(): void {
    if (this.destroyed) {
      throw new Error('webgpu.demo_session_destroyed');
    }
  }
}
