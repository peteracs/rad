# WebGPU presentation host

RAD can drive a WebGPU application today without making GPU handles part of
the world. The boundary is intentionally asymmetric:

```text
RAD VM                              browser host
------                              ------------
authoritative state                 GPUDevice
causal changes                      GPUBuffer / GPUTexture
replay and entity lifetimes   ->    pipelines and bind groups
bounded presentation packet         command encoding and submission
```

The host resources are disposable. If the device is lost, the browser drops
them, requests a new device, and materializes the next packet again. RAD state
does not roll back or change because a GPU disappeared.

## Use it

The reusable host lives in
[`projects/rad-webgpu`](../../../projects/rad-webgpu/README.md).
Build the VM package and the host:

```bash
wasm-pack build --target web core/vm
npm ci --prefix projects/rad-webgpu
npm test --prefix projects/rad-webgpu
npm run build --prefix projects/rad-webgpu
```

The dogfood is emitted to `projects/rad-webgpu/demo-dist/`. In development,
run `npm run dev --prefix projects/rad-webgpu`.

An embed composes the already-running RAD session with a canvas:

```ts
const app = await RadWebGpuApp.create(canvas, runtime, wasm.memory, {
  source: { maxRecords: 100_000 },
  renderer: { worldWidth: 200, worldHeight: 120 },
});

runtime.session_emit('Tick', '{"dt":0.016}');
runtime.session_pump();
app.render();
```

`RadWebGpuApp` is only a composition root. Applications can independently use
`WasmAvatarPresentationSource`, `WebGpuDeviceHost`, `GpuBufferMirror`, and
`AvatarRenderer` when their frame scheduler or renderer owns those layers.

## Exact packet contract

`RadRuntime.runtime_features()` publishes the descriptor for the current
`avatar_instances` stream. It includes the magic, version, header and record
widths, every field offset, and default/hard record and entity-scan ceilings.
Browser code validates this descriptor rather than copying Rust constants.

The packet is an array of `u32` words. Entity slot, generation, player ID,
model ID, flags, and both halves of an `i64` command ID remain exact. Position
values occupy words containing their IEEE-754 `f32` bits. The record-only view
can therefore be uploaded directly to a WebGPU storage buffer and decoded with
WGSL `bitcast<f32>`.

The encoder:

- charges host-selected record and entity-scan ceilings capped by runtime hard limits;
- enforces the scan ceiling before allocating and sorting the entity-ID view;
- uses checked, fallible allocation;
- clears the entire packet on any encoding failure;
- rejects non-finite or non-`f32` coordinates;
- binds entity generation so reused slots are different lifetimes.

The WASM view is reacquired after every runtime call. A call can grow WebAssembly
memory and detach all older typed-array views.

## GPU lifecycle and limits

The host performs these checks before allocating or writing:

- requested features must exist on the selected adapter;
- storage size is capped by `maxBufferSize` and
  `maxStorageBufferBindingSize`;
- buffers grow geometrically and replaced buffers are destroyed;
- dirty word ranges, when supplied by a future stream, are bounds-checked;
- canvas dimensions are capped by `maxTextureDimension2D` and device-pixel
  ratio is capped by the embedder;
- stale presentation frame numbers reject.

The implementation observes `GPUDevice.lost`, discards every device-owned
resource, retries device creation with bounded backoff, and rebuilds on the
next RAD packet. This follows WebGPU's device-loss model: resources created by
the old device are no longer usable and must be recreated on a new device.

## Authority rule

Never put these in an authoritative component, relation, snapshot, or replay:

```text
GPUDevice
GPUBuffer
GPUTexture
GPURenderPipeline
GPUBindGroup
```

Stable asset IDs and presentation values belong in RAD. Their GPU realizations
belong in a host cache. GPU compute should remain presentation-only unless its
result re-enters RAD through an explicit, validated event and normal atomic
settlement.

## Scaling beyond the first stream

The current packet is a deliberately narrow avatar materializer replacing the
old lossy all-`f32` MOBA bridge. It is not a claim that every scene belongs in
one avatar record.

New streams should be generated from sealed schemas and normally sourced from
read-only derived relations. Each stream needs its own descriptor, limits,
stable asset identity, and packet version. Large scenes can then add stable
instance slots and runtime-produced dirty ranges without changing the GPU host
or turning rendering into an authoritative writer.

The intended evolution is:

```text
authoritative components and relations
        -> derived presentation facts
        -> compiler-described bounded streams
        -> full packet reference path
        -> independently checked dirty-range maintenance
        -> replaceable WebGPU materializers
```

Full packets remain the semantic reference. Incremental presentation must be
differential-tested against them before becoming the default.
