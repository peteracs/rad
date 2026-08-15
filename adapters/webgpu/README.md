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

The update and display paths are deliberately separate:

```text
fixed RAD settlement -> publish() -> owned logical publication
display cadence      -> renderLatest() -> persistent GPU target -> canvas
diagnostics          -> captureLastFrame() -> bounded staging copy
```

`publish()` is the only path that touches WASM. `renderLatest()` never advances
RAD, and readback never refreshes, uploads, or redraws the scene. Device loss
discards only GPU residency; logical stream lineage and the latest owned
publication remain available for rematerialization.

```ts
const app = await RadWebGpuApp.create(canvas, runtime, wasm.memory, {
  source: { maxRecords: 100_000 },
});

runtime.session_emit('Tick', '{"dt":0.016}');
runtime.session_pump();
app.publish();
app.renderLatest();
```

Run the dogfood after building `core/vm/pkg` with `wasm-pack`:

```text
npm ci
npm run test
npm run build
npm run dev
```

GPU handles never enter RAD snapshots or replay. Publications are copied out of
borrowed WASM memory and retained independently from GPU residency. A lost
device is replaced and `renderLatest()` rematerializes the last accepted
publication on the new device without advancing RAD.
Async adapter/device requests are lifecycle-generation checked, so a destroyed
host cannot be resurrected by an in-flight request.

The dogfood runs fixed simulation steps independently from display cadence.
Each presentation is rendered once into a persistent target and copied to the
transient canvas texture. `npm run test:browser` passively reads that exact
target through a caller-bounded staging buffer, then proves pixels, resize,
session restart, and device recovery in Chromium's SwiftShader test adapter.
