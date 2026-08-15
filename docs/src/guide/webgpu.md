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
[`adapters/webgpu`](../../../adapters/webgpu/README.md).
Build the VM package and the host:

```bash
wasm-pack build --target web core/vm
npm ci --prefix adapters/webgpu
npm test --prefix adapters/webgpu
npm run build --prefix adapters/webgpu
```

The dogfood is emitted to `adapters/webgpu/demo-dist/`. In development,
run `npm run dev --prefix adapters/webgpu`.

An embed composes the already-running RAD session with a canvas:

```ts
const app = await RadWebGpuApp.create(canvas, runtime, wasm.memory, {
  source: { maxRecords: 100_000 },
  renderer: { worldWidth: 200, worldHeight: 120 },
});

runtime.session_emit('Tick', '{"dt":0.016}');
runtime.session_pump();
app.publish();
app.renderLatest();
```

`RadWebGpuApp` is only a composition root. Applications can independently use
`WasmAvatarPresentationSource`, `WebGpuDeviceHost`, `GpuBufferMirror`, and
`AvatarRenderer` when their frame scheduler or renderer owns those layers.

## Exact packet contract

`RadRuntime.runtime_features()` publishes the descriptor for the current
`avatar_instances` stream. It includes the magic, version, header and record
widths, every header/record field offset, packet-kind identities, and
default/hard record and entity-scan ceilings.
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

Every successful `session_start()` begins a new presentation stream. Packets
bind a `stream_id`, monotonic `packet_sequence`, full/delta kind, causal frame,
and (for deltas) the exact baseline sequence. The current encoder emits only
full packets. A host accepts a new stream only at full sequence zero; future
deltas must extend the exact last accepted sequence. A session restart
therefore discards the old GPU mirror instead of being confused with a stale
frame from the prior world.

The WASM view is reacquired after every runtime call because memory growth can
detach older typed-array views. `publish()` copies the bounded packet before it
returns, so logical publication, display cadence, readback, and device recovery
all consume one owned snapshot without refreshing RAD again.

## GPU lifecycle and limits

The host performs these checks before allocating or writing:

- requested features must exist on the selected adapter;
- storage size is capped by `maxBufferSize` and
  `maxStorageBufferBindingSize`;
- buffers grow geometrically and replaced buffers are destroyed;
- dirty word ranges, when supplied by a future delta stream, are bounds-checked;
- canvas dimensions are capped by `maxTextureDimension2D` and device-pixel
  ratio is capped by the embedder;
- stale packet sequences and wrong-stream delta baselines reject.

The implementation observes `GPUDevice.lost`, discards every device-owned
resource, retries device creation with bounded backoff, and rebuilds from the
last accepted publication. This follows WebGPU's device-loss model: resources
created by the old device are no longer usable and must be recreated on a new
device.
Lifecycle generations are checked after every asynchronous adapter/device
request, so destroying the host cannot be undone by an older promise. Rapid
losses coalesce into recovery work without dropping the newest loss, and
exceptions in error/session observers cannot interrupt recovery.

The renderer draws once into a persistent presentation target, then copies that
exact texture into the transient canvas texture. CI launches Chromium with its
documented SwiftShader WebGPU test adapter and passively copies the persistent
target into a caller-bounded staging buffer. It proves pixels, resize, session
restart, forced device loss, and rematerialization on the replacement device
without issuing a second RAD refresh or avatar draw. This software-adapter smoke
complements, rather than replaces, hardware-browser testing.

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

New stream types should be generated from sealed schemas and normally sourced from
read-only derived relations. Each stream needs its own descriptor, limits,
stable asset identity, packet version, and independent lineage. Large scenes
can then add stable instance slots and runtime-produced dirty ranges without
changing the GPU host or turning rendering into an authoritative writer.

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
