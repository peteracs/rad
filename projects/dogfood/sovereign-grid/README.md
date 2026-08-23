# Sovereign Grid

Sovereign Grid is RAD's cumulative production acceptance service. It dispatches
energy-grid missions across owned state, atomic lifecycle transactions,
incremental views, ordered indexes, explicit phases, provenance, deterministic
replay, stateful models, relations, and a native scoring extension. It is also
the executable denominator for every canonical syntax rule and all 257 runtime
builtins.

This project is accepted by evidence, not by `main.rad` merely exiting zero.

## Ten-minute walkthrough

Build the release tools and native extension with serialized Rust compilation:

```powershell
cargo build -p rad-cli -p rad-ffi-worker --release -j 1
python tooling/build_sovereign_grid_plugin.py
```

Run the strict stable workflow:

```powershell
target/release/rad.exe projects/dogfood/sovereign-grid/main.rad --strict-types --deny-warnings
```

The run prints a source-state digest, completed-event order, field and removal
provenance, and the reason the completed mission is absent from the active
materialized view.

Inspect architecture without adding logging:

```powershell
target/release/rad.exe effects AccountActiveMissions --file projects/dogfood/sovereign-grid/main.rad --json
target/release/rad.exe path execute_cycle '->' CompleteMission --file projects/dogfood/sovereign-grid/main.rad --json
target/release/rad.exe query-plan ActiveMissions --file projects/dogfood/sovereign-grid/main.rad --json
target/release/rad.exe cost-path AccountActiveMissions --file projects/dogfood/sovereign-grid/main.rad --json
target/release/rad.exe why-removed 1002 ActiveMission --file projects/dogfood/sovereign-grid/main.rad --json
```

Run one deliberate defect and observe the transitive repair path:

```powershell
target/release/rad.exe projects/dogfood/sovereign-grid/negative/effects_hidden_write.rad --strict-types --deny-warnings
```

The complete acceptance run is one command:

```powershell
projects/dogfood/sovereign-grid/accept.ps1
```

It executes stable and experimental workflows, all intended failures, the
10,000-history model campaign, native/reference/recorded FFI, replay under 1,
2, 4, and logical-CPU workers, operational inspection, and a 30-sample release
benchmark. Its source-bound JSON report is written under
`target/sovereign-grid-acceptance/`.
Every RAD child has a strict end-to-end deadline below one second; aggregate
Python, native-build, Rust-build, and documentation orchestrators are reported
separately. The report records the actual Cargo profile inferred from the
binary path, so a `runtime-bench` diagnosis cannot be mislabeled as release.

## Architecture

`schema.rad` owns exact-width and ABI-facing domain types. `owners/grid_owner.rad`
is the sole mutation boundary for lifecycle state. `systems/control_loop.rad`
contains scheduled behavior and phase contracts. `scenarios/production.rad`
builds realistic load. `language/` and `builtins/` are executable coverage
harnesses. `negative/manifest.toml` assigns every rejected implementation to an
exact diagnostic contract. Experimental causal and relation programs remain
separate from the stable feature boundary.

The native oracle in `plugins/grid-oracle/` is an independent `cdylib`; it does
not link RAD internals. `rad ffi verify` checks ABI version, calling convention,
opaque field identity, offsets, alignment, effects, determinism, and replay
support before execution.

## Performance contract

`bench.rad` compares a correct offline full scan with an equivalent
materialized-view traversal over 50,000 entities, matching the production asset
scale used by the dispatch portfolio. Fixture construction is
excluded from the measured entry. Acceptance requires equal world digests,
at least 100x fewer VM instructions, at least 100x lower median measured-entry
wall time over 30 release samples, zero guest allocations, and zero host-boundary
allocations for the indexed root. Runtime allocator bytes are measured and
reported separately; they are never described as zero when nonzero.

The JSON distinguishes `medianElapsedNs`, measured by RAD around `bench_run`,
from `medianProcessCpuNs`, sampled for the complete CLI child including load and
setup. `peakRssBytes` is likewise process-wide. This prevents setup costs from
being presented as hot-root costs while retaining an operational memory bound.

## Complete references

- [Compiler-owned syntax rules](../../../docs/src/reference/generated/syntax-rules.md)
- [Complete builtin API](../../../docs/src/reference/generated/builtin-api.md)
- [Executable language tour](../../../docs/src/reference/executable-language-tour.md)
- [CLI](../../../docs/src/reference/cli.md)
- [Embedding](../../../docs/src/reference/embedding.md)
- [Native extension ABI](../../../docs/src/reference/ffi.md)
- [WASM and JavaScript](../../../docs/src/reference/wasm-phase3.md)
