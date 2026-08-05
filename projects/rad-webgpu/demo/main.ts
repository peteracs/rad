import init, { RadRuntime } from '../../../core/vm/pkg/rad_vm.js';
import { RadWebGpuApp, type AvatarFrameReadback } from '../src/index.js';
import source from './world.rad?raw';

const canvas = requiredElement(HTMLCanvasElement, '#viewport');
const status = requiredElement(HTMLElement, '#status');

async function start(): Promise<void> {
  if (!navigator.gpu) {
    status.textContent = 'WebGPU is unavailable in this browser.';
    status.dataset.kind = 'error';
    return;
  }

  const wasm = await init();
  const runtime = new RadRuntime();
  runtime.session_start(source);
  const errors: string[] = [];
  const app = await RadWebGpuApp.create(canvas, runtime, wasm.memory, {
    source: { maxRecords: 4096, maxEntitiesScanned: 16_384 },
    renderer: { worldWidth: 200, worldHeight: 120, avatarRadius: 4 },
    device: {
      allowCanvasReadback: true,
      maxDevicePixelRatio: 2,
      onError(error) {
        errors.push(error.message);
        status.textContent = error.message;
        status.dataset.kind = 'error';
      },
    },
  });

  let previous = performance.now();
  let frameHandle = 0;
  let renderedFrames = 0;
  let lastStreamId = 0n;
  let lastSequence = 0n;
  let lastRecordCount = 0;
  globalThis.__radWebGpuDogfood = {
    async capture() {
      const packet = app.source.refresh();
      const readback = await app.renderer.readback(packet, 16 * 1024 * 1024);
      if (!readback) throw new Error('webgpu.readback_device_unavailable');
      lastRecordCount = packet.header.count;
      lastStreamId = packet.header.streamId;
      lastSequence = packet.header.sequence;
      return {
        changedPixels: countChangedPixels(readback),
        height: readback.height,
        recordCount: packet.header.count,
        width: readback.width,
      };
    },
    loseDevice() {
      app.deviceHost.session?.device.destroy();
    },
    restart() {
      runtime.session_start(source);
    },
    snapshot() {
      return {
        canvasWidth: canvas.width,
        deviceEpoch: app.deviceHost.session?.epoch ?? 0,
        errors: [...errors],
        recordCount: lastRecordCount,
        renderedFrames,
        sequence: lastSequence.toString(),
        streamId: lastStreamId.toString(),
      };
    },
  };
  const frame = (now: number): void => {
    const dt = Math.min((now - previous) / 1000, 0.05);
    previous = now;
    runtime.session_emit('Tick', JSON.stringify({ dt }));
    runtime.session_pump();
    const packet = app.source.refresh();
    if (app.renderer.render(packet)) {
      renderedFrames += 1;
      lastRecordCount = packet.header.count;
      lastStreamId = packet.header.streamId;
      lastSequence = packet.header.sequence;
      status.textContent = `RAD frame materialized on GPU device epoch ${app.deviceHost.session?.epoch ?? 0}`;
      status.dataset.kind = 'ok';
    }
    frameHandle = requestAnimationFrame(frame);
  };
  frameHandle = requestAnimationFrame(frame);
  addEventListener('pagehide', () => {
    cancelAnimationFrame(frameHandle);
    globalThis.__radWebGpuDogfood = undefined;
    app.destroy();
  }, { once: true });
}

function countChangedPixels(readback: AvatarFrameReadback): number {
  const reference = readback.pixels.subarray(0, 3);
  let changed = 0;
  for (let y = 0; y < readback.height; y += 1) {
    for (let x = 0; x < readback.width; x += 1) {
      const offset = y * readback.bytesPerRow + x * 4;
      const distance = Math.abs((readback.pixels[offset] ?? 0) - (reference[0] ?? 0))
        + Math.abs((readback.pixels[offset + 1] ?? 0) - (reference[1] ?? 0))
        + Math.abs((readback.pixels[offset + 2] ?? 0) - (reference[2] ?? 0));
      if (distance > 60) changed += 1;
    }
  }
  return changed;
}

start().catch((error: unknown) => {
  status.textContent = error instanceof Error ? error.message : String(error);
  status.dataset.kind = 'error';
});

function requiredElement<T extends Element>(
  constructor: { new (): T },
  selector: string,
): T {
  const element = document.querySelector(selector);
  if (!(element instanceof constructor)) throw new Error(`demo element missing: ${selector}`);
  return element;
}
