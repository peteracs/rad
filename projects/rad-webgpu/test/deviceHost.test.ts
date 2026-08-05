import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import {
  WebGpuDeviceHost,
  type RadWebGpuRequiredLimits,
} from '../src/deviceHost.js';

test('device loss installs a fresh epoch without retaining old GPU state', async () => {
  const first = new FakeDevice();
  const second = new FakeDevice();
  const devices = [first, second];
  const restoreNavigator = installNavigatorGpu(gpuFromDevices(devices));

  try {
    const errors: string[] = [];
    const host = await WebGpuDeviceHost.create(fakeCanvas(), {
      onError: (error) => errors.push(error.message),
    });
    const epochs: number[] = [];
    host.onSession(() => { throw new Error('listener rejected session'); });
    host.onSession((session) => epochs.push(session.epoch));
    assert.deepEqual(epochs, [1]);
    assert.ok(errors.includes('webgpu.session_listener_failed'));

    first.lose({ reason: 'unknown', message: 'adapter reset' });
    await eventually(() => host.session?.epoch === 2);
    assert.deepEqual(epochs, [1, 2]);
    assert.ok(errors.some((message) => message.includes('device_lost:unknown:adapter reset')));

    host.destroy();
    assert.equal(second.destroyed, true);
  } finally {
    restoreNavigator();
  }
});

test('throwing error and session observers cannot interrupt recovery', async () => {
  const first = new FakeDevice();
  const second = new FakeDevice();
  const restoreNavigator = installNavigatorGpu(gpuFromDevices([first, second]));
  try {
    const host = await WebGpuDeviceHost.create(fakeCanvas(), {
      onError() { throw new Error('observer failure'); },
    });
    host.onSession(() => { throw new Error('listener failure'); });
    first.lose({ reason: 'unknown', message: 'test loss' });
    await eventually(() => host.session?.epoch === 2);
    host.destroy();
  } finally {
    restoreNavigator();
  }
});

test('a device lost during recovery cannot strand the host', async () => {
  const first = new FakeDevice();
  const immediatelyLost = new FakeDevice();
  immediatelyLost.lose({ reason: 'unknown', message: 'lost before install settled' });
  const third = new FakeDevice();
  const restoreNavigator = installNavigatorGpu(gpuFromDevices([first, immediatelyLost, third]));
  try {
    const host = await WebGpuDeviceHost.create(fakeCanvas());
    first.lose({ reason: 'unknown', message: 'first loss' });
    await eventually(() => host.session?.epoch === 3);
    assert.equal(host.session?.device, third as unknown as GPUDevice);
    host.destroy();
  } finally {
    restoreNavigator();
  }
});

test('destroy invalidates a pending recovery adapter request', async () => {
  const first = new FakeDevice();
  const adapter = adapterFor(new FakeDevice());
  const pendingAdapter = deferred<GPUAdapter | null>();
  let adapterRequests = 0;
  const restoreNavigator = installNavigatorGpu({
    requestAdapter() {
      adapterRequests += 1;
      return adapterRequests === 1
        ? Promise.resolve(adapterFor(first))
        : pendingAdapter.promise;
    },
    getPreferredCanvasFormat: () => 'bgra8unorm',
  });
  try {
    const host = await WebGpuDeviceHost.create(fakeCanvas());
    first.lose({ reason: 'unknown', message: 'recover' });
    await eventually(() => adapterRequests === 2);
    host.destroy();
    pendingAdapter.resolve(adapter);
    await Promise.resolve();
    assert.equal(adapter.requestDeviceCalls, 0);
    assert.equal(host.session, null);
  } finally {
    restoreNavigator();
  }
});

test('destroy cleans up a device returned by a pending recovery request', async () => {
  const first = new FakeDevice();
  const provisional = new FakeDevice();
  const pendingDevice = deferred<GPUDevice>();
  let adapterRequests = 0;
  const restoreNavigator = installNavigatorGpu({
    requestAdapter() {
      adapterRequests += 1;
      if (adapterRequests === 1) return Promise.resolve(adapterFor(first));
      return Promise.resolve(adapterFor(provisional, pendingDevice.promise));
    },
    getPreferredCanvasFormat: () => 'bgra8unorm',
  });
  try {
    const host = await WebGpuDeviceHost.create(fakeCanvas());
    first.lose({ reason: 'unknown', message: 'recover' });
    await eventually(() => adapterRequests === 2);
    host.destroy();
    pendingDevice.resolve(provisional as unknown as GPUDevice);
    await eventually(() => provisional.destroyed);
    assert.equal(host.session, null);
  } finally {
    restoreNavigator();
  }
});

test('required limits are whitelisted and reject before creating a device', async () => {
  const adapter = adapterFor(new FakeDevice(), undefined, { maxBufferSize: 1024 });
  const restoreNavigator = installNavigatorGpu({
    requestAdapter: async () => adapter,
    getPreferredCanvasFormat: () => 'bgra8unorm',
  });
  try {
    await assert.rejects(
      WebGpuDeviceHost.create(fakeCanvas(), { requiredLimits: { maxBufferSize: 2048 } }),
      /limit_unavailable:maxBufferSize/,
    );
    assert.equal(adapter.requestDeviceCalls, 0);

    await assert.rejects(
      WebGpuDeviceHost.create(fakeCanvas(), {
        requiredLimits: { maxBindGroups: 4 } as unknown as RadWebGpuRequiredLimits,
      }),
      /unsupported_required_limit:maxBindGroups/,
    );
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
    if (this.destroyed) return;
    this.destroyed = true;
    this.lose({ reason: 'destroyed', message: 'destroyed' });
  }
}

interface FakeAdapter extends GPUAdapter {
  requestDeviceCalls: number;
}

function adapterFor(
  device: FakeDevice,
  request: Promise<GPUDevice> = Promise.resolve(device as unknown as GPUDevice),
  limits: Record<string, number> = {
    maxBufferSize: 1 << 24,
    maxStorageBufferBindingSize: 1 << 24,
  },
): FakeAdapter {
  const adapter = {
    features: new Set(),
    limits,
    requestDeviceCalls: 0,
    requestDevice() {
      adapter.requestDeviceCalls += 1;
      return request;
    },
  } as unknown as FakeAdapter;
  return adapter;
}

function gpuFromDevices(devices: FakeDevice[]) {
  return {
    async requestAdapter() {
      const device = devices.shift();
      return device ? adapterFor(device) : null;
    },
    getPreferredCanvasFormat: () => 'bgra8unorm',
  };
}

function fakeCanvas(): HTMLCanvasElement {
  return new FakeCanvas() as unknown as HTMLCanvasElement;
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => { resolve = accept; });
  return { promise, resolve };
}

async function eventually(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
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
