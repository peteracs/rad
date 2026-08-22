# Rust embedding API

The generated item-level reference is published at
[rad_vm](https://peteracs.github.io/rad/rustdoc/rad_vm/). The mdBook page below
defines lifecycle and ownership contracts that rustdoc item signatures alone
cannot express.

The `rad-vm` crate is the native embedding surface. Exact item signatures and
intra-doc links are generated with:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps -j 1
```

Open `target/doc/rad_vm/index.html`. That checked output is authoritative for
the current commit; this chapter defines ownership and sequencing.

## Checked compilation

Use `pipeline::analyze_source` for one source or the module loader plus
`pipeline::analyze_program` for a graph. Successful analysis yields an opaque
`CheckedProgram` bound to the complete program/module graph, canonical module
identities, semantic configuration, and an integrity digest of checker-owned
lowering metadata.

Compile with `pipeline::compile_checked_program`. A product from another
program/configuration, without provenance, or with altered metadata is
rejected. Do not assemble a compiler from a program and independently supplied
semantic maps.

## Runtime ownership

`vm::VM` owns bytecode, globals, ECS world, resources, event buffers, tasks,
RNG state, indexes/views, transaction state, and causal ledger. Heap-backed
`Value`s belong to that VM's `GcHeap`; never reconstruct raw values or transfer
them between VMs. Use `host_value::FrozenValue` for detached values and the
wire/snapshot APIs for durable transfer.

`VM::load_compile_result` installs one checked result. `run`, `call_global`,
and `execute_transition` mutate that VM. `execute_transition` returns a
portable trace containing before/after digests, writes, events, result, and
causes for differential conformance.

## Persistence, replay, and concurrency

Use canonical snapshots/deltas rather than serializing internal structs. Import
validates format, schema, limits, nominal native tags, source identity, and
checksums before adoption; failure leaves the VM unchanged.

One `VM` is one mutable authority. Give each host thread/task its own VM or
coordinate access externally. RAD schedule parallelism is runtime-owned and
merges isolated worker patches deterministically.

## Errors

Public operations return structured `Result` values where recovery exists.
Source diagnostics retain file/span/code/hint data; flatten only at the UI
boundary. Host faults, candidate-constraint rejections, and RAD runtime errors
are distinct failure classes.
