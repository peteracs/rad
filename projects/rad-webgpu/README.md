# RAD WebGPU host

`@rad-lang/webgpu` turns RAD's read-only presentation packet into disposable
WebGPU resources. RAD remains the owner of simulation, causality, replay, and
entity lifetimes. The package owns browser GPU devices, storage buffers,
pipelines, canvas configuration, resizing, and device-loss recovery.

```text
RAD world -> bounded presentation packet -> WebGPU storage buffer -> draw
```

The packet descriptor comes from `RadRuntime.runtime_features()`. Consumers do
not copy layout constants. Integer identities are exact `u32` words; floats use
their IEEE-754 bit representation and upload directly to a storage buffer.
Stream IDs and packet sequences make session restarts explicit and bind future
deltas to one exact full-packet baseline.

```ts
const app = await RadWebGpuApp.create(canvas, runtime, wasm.memory, {
  source: { maxRecords: 100_000 },
});

runtime.session_emit('Tick', '{"dt":0.016}');
runtime.session_pump();
app.render();
```

Run the dogfood after building `core/vm/pkg` with `wasm-pack`:

```text
npm ci
npm run test
npm run build
npm run dev
```

GPU handles never enter RAD snapshots or replay. A lost device is replaced and
the next render call rematerializes the current full packet on the new device.
Async adapter/device requests are lifecycle-generation checked, so a destroyed
host cannot be resurrected by an in-flight request.

`npm run test:browser` runs the real Chromium/SwiftShader pixel, resize,
session-restart, and device-recovery smoke after the WASM package is built.
It uses an explicitly bounded offscreen GPU texture readback of the same render
pass, without changing ordinary canvas usage.
