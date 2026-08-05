import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { WebGpuDeviceHost } from '../src/deviceHost.js';

test('device loss installs a fresh epoch without retaining old GPU state', async () => {
  const first = new FakeDevice();
  const second = new FakeDevice();
  const devices = [first, second];
  const gpu = {
    async requestAdapter() {
      const device = devices.shift();
      if (!device) return null;
      return {
        features: new Set(),
        requestDevice: async () => device,
      };
    },
    getPreferredCanvasFormat: () => 'bgra8unorm',
  };
  const restoreNavigator = installNavigatorGpu(gpu);

  try {
    const errors: string[] = [];
    const canvas = new FakeCanvas();
    const host = await WebGpuDeviceHost.create(canvas as unknown as HTMLCanvasElement, {
      onError: (error) => errors.push(error.message),
    });
    const epochs: number[] = [];
    assert.throws(
      () => host.onSession(() => { throw new Error('listener rejected session'); }),
      /listener rejected session/,
    );
    host.onSession((session) => epochs.push(session.epoch));
    assert.deepEqual(epochs, [1]);

    first.lose({ reason: 'unknown', message: 'adapter reset' });
    await eventually(() => host.session?.epoch === 2);
    assert.deepEqual(epochs, [1, 2]);
    assert.match(errors[0] ?? '', /device_lost:unknown:adapter reset/);
    assert.equal(first.destroyed, false);

    host.destroy();
    assert.equal(second.destroyed, true);
  } finally {
    restoreNavigator();
  }
});

test('required limits reject before creating a device', async () => {
  let requestDeviceCalled = false;
  const restoreNavigator = installNavigatorGpu({
    async requestAdapter() {
      return {
        features: new Set(),
        limits: { maxBufferSize: 1024 },
        async requestDevice() {
          requestDeviceCalled = true;
          return new FakeDevice();
        },
      };
    },
    getPreferredCanvasFormat: () => 'bgra8unorm',
  });
  try {
    await assert.rejects(
      WebGpuDeviceHost.create(new FakeCanvas() as unknown as HTMLCanvasElement, {
        requiredLimits: { maxBufferSize: 2048 },
      }),
      /limit_unavailable:maxBufferSize/,
    );
    assert.equal(requestDeviceCalled, false);
  } finally {
    restoreNavigator();
  }
});

class FakeCanvas {
  clientWidth = 640;
  clientHeight = 360;
  width = 0;
  height = 0;
  readonly context = { configure() {}, unconfigure() {} };

  getContext(name: string): typeof this.context | null {
    return name === 'webgpu' ? this.context : null;
  }
}

class FakeDevice {
  readonly features = new Set<GPUFeatureName>();
  readonly limits = {
    maxTextureDimension2D: 8192,
  };
  readonly lost: Promise<GPUDeviceLostInfo>;
  onuncapturederror: ((event: GPUUncapturedErrorEvent) => void) | null = null;
  destroyed = false;
  private resolveLoss!: (info: GPUDeviceLostInfo) => void;

  constructor() {
    this.lost = new Promise((resolve) => {
      this.resolveLoss = resolve;
    });
  }

  lose(info: Pick<GPUDeviceLostInfo, 'reason' | 'message'>): void {
    this.resolveLoss(info as GPUDeviceLostInfo);
  }

  destroy(): void {
    this.destroyed = true;
    this.lose({ reason: 'destroyed', message: 'destroyed' });
  }
}

async function eventually(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  throw new Error('condition did not become true');
}

function installNavigatorGpu(gpu: unknown): () => void {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  Object.defineProperty(globalThis, 'navigator', {
    configurable: true,
    value: { gpu },
  });
  return () => {
    if (previous) Object.defineProperty(globalThis, 'navigator', previous);
    else Reflect.deleteProperty(globalThis, 'navigator');
  };
}
