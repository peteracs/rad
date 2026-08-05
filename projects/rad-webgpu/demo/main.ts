import init, { RadRuntime } from '../../../core/vm/pkg/rad_vm.js';
import { RadWebGpuApp } from '../src/index.js';
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
  globalThis.__radWebGpuDogfood = {
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
