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
  const app = await RadWebGpuApp.create(canvas, runtime, wasm.memory, {
    source: { maxRecords: 4096, maxEntitiesScanned: 16_384 },
    renderer: { worldWidth: 200, worldHeight: 120, avatarRadius: 4 },
    device: {
      maxDevicePixelRatio: 2,
      onError(error) {
        status.textContent = error.message;
        status.dataset.kind = 'error';
      },
    },
  });

  let previous = performance.now();
  let frameHandle = 0;
  const frame = (now: number): void => {
    const dt = Math.min((now - previous) / 1000, 0.05);
    previous = now;
    runtime.session_emit('Tick', JSON.stringify({ dt }));
    runtime.session_pump();
    if (app.render()) {
      status.textContent = `RAD frame materialized on GPU device epoch ${app.deviceHost.session?.epoch ?? 0}`;
      status.dataset.kind = 'ok';
    }
    frameHandle = requestAnimationFrame(frame);
  };
  frameHandle = requestAnimationFrame(frame);
  addEventListener('pagehide', () => {
    cancelAnimationFrame(frameHandle);
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
