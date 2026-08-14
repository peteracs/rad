import init, { RadRuntime } from '../../../core/vm/pkg/rad_vm.js';
import { RadWebGpuApp } from '../src/index.js';
import { DemoSession } from './session/demoSession.js';
import { startFrameLoop } from './session/frameLoop.js';
import { installBrowserHarness } from './testing/browserHarness.js';
import { createStatusPresenter } from './ui/statusPresenter.js';
import worldSource from './world.rad?raw';

const canvas = requiredElement(HTMLCanvasElement, '#viewport');
const status = createStatusPresenter(requiredElement(HTMLElement, '#status'));

void start().catch((error: unknown) => {
  status.showError(errorMessage(error));
});

/** Browser composition root: construct, connect, run, and release. */
async function start(): Promise<void> {
  if (!navigator.gpu) {
    status.showError('WebGPU is unavailable in this browser.');
    return;
  }

  const wasm = await init();
  const runtime = new RadRuntime();
  const errors: string[] = [];
  let session: DemoSession | null = null;

  runtime.session_start(worldSource);
  try {
    const app = await RadWebGpuApp.create(canvas, runtime, wasm.memory, {
      source: { maxRecords: 4_096, maxEntitiesScanned: 16_384 },
      renderer: { worldWidth: 200, worldHeight: 120, avatarRadius: 4 },
      device: {
        maxDevicePixelRatio: 2,
        onError(error) {
          errors.push(error.message);
          status.showError(error.message);
        },
      },
    });
    session = new DemoSession(canvas, runtime, app, worldSource, errors);
  } catch (error) {
    runtime.free();
    throw error;
  }

  const activeSession = session;
  activeSession.publishCurrentWorld();
  const initialEpoch = activeSession.renderLatestPresentation();
  if (initialEpoch !== null) {
    status.showSuccess(`RAD frame materialized on GPU device epoch ${initialEpoch}`);
  }
  const uninstallHarness = installBrowserHarness(activeSession);
  const stopFrameLoop = startFrameLoop({
    onSimulationStep(stepSeconds) {
      activeSession.advanceSimulation(stepSeconds);
    },
    onRender() {
      const epoch = activeSession.renderLatestPresentation();
      if (epoch !== null) {
        status.showSuccess(`RAD frame materialized on GPU device epoch ${epoch}`);
      }
    },
    onError(error) {
      status.showError(errorMessage(error));
    },
  });

  addEventListener('pagehide', () => {
    stopFrameLoop();
    uninstallHarness();
    activeSession.destroy();
  }, { once: true });
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function requiredElement<T extends Element>(
  constructor: { new (): T },
  selector: string,
): T {
  const element = document.querySelector(selector);
  if (!(element instanceof constructor)) {
    throw new Error(`demo element missing: ${selector}`);
  }
  return element;
}
