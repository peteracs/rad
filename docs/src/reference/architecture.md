# Architecture

## Compilation pipeline

```
source.rad → Lexer → Tokens → Parser → AST → Checker → Compiler → Bytecode → VM → Output
```

The browser playground compiles the VM to WebAssembly.

## Semantics notes

- List transforms are value-oriented (`push`, `sort`, `filter`, etc.). Rebind the returned value (`xs = push(xs, v)`).
- The VM uses: NaN-boxed 48-bit unboxed integers; **lists** as `Arc<Vec<Value>>` (copy-on-write when uniquely owned); **maps** as persistent HAMTs (`im::HashMap`) for structural sharing; **strings** as `Arc<str>` inside `Object::Str`; a strict Structure-of-Arrays (SoA) ECS with copy-on-write columns and `ValueColumn` wrappers for persistent object lifecycle; an ephemeral **`BumpArena`** for system temporaries; and a minimal backup **`GcHeap`** for closure/capture-cell graphs only.
- **Checker declaration and effect pass:** `core/vm/src/checker/declarations.rs` collects components, resources, structs, states, systems, events, functions, sum types, aliases, phases, and entities into checker tables before lowering. It enforces public API annotations, component/resource name separation, indexed-field hashability, resource defaults, system dependency cycles, explicit effect contracts, and conservative pure/readonly/effect inference. After all functions are known, it refines inferred effects so forward calls do not depend on declaration order.
- **Simulation-safety checking:** Systems passed to `simulate()` may emit events; those handlers run on the forked event queue. The checker follows the transitive handler chain and rejects IO, `commit()`, unsafe event-effect calls such as `transition`, and other effects that would escape the fork.
- **Checker diagnostics:** `core/vm/src/checker/diagnostics.rs` centralizes checker return merging, error/warning construction, builtin effect classification, immutable-transform detection, and small "did you mean" / type-conversion hints used across checker passes.
- **Checker orchestration and reachability:** `core/vm/src/checker/mod.rs` owns the checker state, built-in `Option`/`Result`/`Conflict` sum types, alias registration, alias body checking, checker output, and unused-system detection. `core/vm/src/checker/reachability.rs` enforces public API visibility and traces live functions, components, events, and structs from entry points, public events, systems, tests, top-level statements, emits, aliases, type annotations, and literals.
- **Checker type/scope support:** `core/vm/src/checker/resolve.rs` resolves annotations, generic aliases, surface types such as `world_fork` and `system`, and literal type hints. `core/vm/src/checker/scope.rs` tracks lexical scopes, variable reads, shadowing, builtin-name shadowing, uniqueness, and pipeline-local bindings.
- **Checker semantic typing:** `core/vm/src/checker/typeck.rs` is the main statement/expression type checker. It validates module contracts, plain-data declarations, systems/resources, migrations, handlers, `let`/`let-else`/destructuring, guard narrowing, query loops/selects, emits/schedules/updates, match exhaustiveness/destructuring, pipeline purity and readonly rules, async/await, spreads, unique bindings, module alias members, and typed builtin calls.
- **Rust VM compiler lowering:** `core/vm/src/compiler/decl.rs` lowers checked declarations into bytecode chunks and runtime metadata: component/resource/struct globals, entity spawns, state-machine transitions, systems, migrations, event handlers, functions, and tests. The compiler carries checker output so aliases, file-private scopes, ECS/read effects, resources, type redirects, and spread lengths stay canonical at emission time.
- **Compiler state and result assembly:** `core/vm/src/compiler/mod.rs` owns global slots, locals/upvalues, loop contexts, aliases, file-private name mangling, checker output, feature flags, release mode, and the final `CompileResult`. It also assembles component/event/resource/struct layouts, indexed fields, transient resources, variant layouts, system/handler/migration/state metadata, global names, warnings, and the compile-time heap that the VM merges on load.
- **Layout analysis and materialization planning:** `core/vm/src/compiler/layout_analysis.rs` votes each checked component/resource/struct toward SoA or AoS from system parameters, ECS/read-effect functions, IO-effect functions, and nested field containment. `core/vm/src/compiler/materialization.rs` records SoA types that still cross AoS boundaries so future lowering/diagnostics can know where materialization is required.
- **Expression lowering:** `core/vm/src/compiler/expr.rs` emits bytecode for literals, f-strings, calls and spread args, module aliases, system references, component/variant/state/entity literals, match/if/function expressions, queries, and pipeline forms. It folds safe constants, lowers f-strings and string-concat chains through `Op::ConcatN`, strips direct `debug_trace(x)` calls in release mode while still evaluating `x`, hoists simple query negations into structural `without` filters, and emits vectorized map/filter pipelines when closure shapes are simple enough.
- **Statement lowering:** `core/vm/src/compiler/stmt.rs` emits bytecode for `let`/`let rec`, tuple destructuring, assignments and nested container writeback, `if`/`while`/`for`, counted `range` loops, mutable query loops, `return`/`break`/`continue`, delayed and immediate `emit`, `schedule` and phase expansion, `update(...)` sugar, and match statements. It is also where mutable query component locals are written back on fallthrough, `return`, `break`, and `continue`.
- **Static escape analysis:** `core/vm/src/compiler/escape.rs` finds non-escaping bitset/buffer locals initialized with `bitset_new()` or `buffer_new()`. The statement compiler can then emit in-place bitset/buffer update opcodes for reassignment patterns such as `bs = bitset_set(bs, i)` or `buf = buffer_append(buf, s)`. This is an optimization only; language-level value semantics stay unchanged.
- **Compiler optimization passes:** Systems and ECS/read-effect functions run through the e-graph optimizer in `core/vm/src/compiler/egraph.rs`. It rewrites integer/boolean expression trees and logical field/index loads/stores with algebraic identities such as `x + 0 -> x`, `x * 1 -> x`, `x - x -> 0`, double-negation removal, and common-factor extraction. Bitwise operations are intentionally opaque to this pass and lower directly to bytecode.
- **Bytecode peepholes:** `core/vm/src/compiler/emit.rs` emits label-safe superinstructions: adjacent local reads can fuse into `Op::GetLocal2`, compare-plus-branch can fuse into `EqJF`/`NeqJF`/`LtJF`/`LteJF`/`GtJF`/`GteJF`, and scope exits use `Op::PopN` for batches. Label high-water tracking prevents a jump from landing inside a fused instruction.
- **Verified bytecode boundary:** Every chunk is fully decoded and control-flow verified before it can execute. Verification rejects malformed operands and jump targets, settlement-region crossings, mismatched control-flow joins, unmatched or nested settlement markers, and terminal paths inside a settlement; the immutable chunk caches the resulting proof. The VM independently binds each active settlement to its owner frame/chunk and applies one centralized opcode/builtin effect firewall, so malformed or compiler-corrupted bytecode cannot close a caller-owned settlement or make durable effects by skipping `EndSettlement`.
- **Candidate validation boundary:** After resolver patches are conflict-checked, RFC-0002 selects attached and watched constraints against one complete immutable candidate view. Invocations are deduplicated by constraint identity and subject, receive isolated deterministic resource limits, and return canonical violations or evaluation failures. Rejection discards the patch and durable provenance; typed host/WASM results carry bounded capability-filtered explanations and an attempt-replay fingerprint.
- **Scalar pipeline fallback:** `core/vm/src/compiler/pipeline.rs` lowers non-vectorizable map/filter chains to a single explicit loop when the statement compiler grants stack-safe fusion. It keeps one source list, one mutable result list, one loop index, and one item local, inlining simple closure bodies where possible and appending through `Op::ListPushLocal`.
- **In-place list mutation:** The compiler optimizes list appends (`list << item` and `push(list, item)`) and pipeline accumulators into `Op::ListPushLocal`, which mutates the `Arc<Vec<Value>>` directly in its stack slot. This avoids stack traffic and allocation overhead while preserving strict value semantics.
- Component literals support update-style spreads: `Stats { hp: 10, ..old_stats }`.
- String repetition is supported via multiplication: `"=" * 40` (also `40 * "="`).
- **Global resources:** `resource` declarations create singleton data stored in a separate `Arc<HashMap<String, ComponentData>>` alongside the entity-component world. Resources participate in fork/snapshot/commit (CoW via `Arc::make_mut`), GC tracing, and parallel conflict analysis. Systems with resource parameters inject the singleton directly; resource-only systems run once per schedule.
- **Copy-on-Write ECS:** All SoA column data, entity bookkeeping, archetype maps, **indexed field hash maps**, and **resource storage** are `Arc`-wrapped. `fork()` performs O(A) refcount bumps (A = archetype column `Arc`s); actual data cloning is deferred to `Arc::make_mut` on first mutation, which triggers an O(E) retain scan over `ValueColumn` to manage persistent `Arc<Object>` refcounts. This makes speculative execution (fork/simulate/peek/commit) nearly free for read-only scenarios. Indexed field indices participate in CoW: they are snapshotted on fork and independently mutable in the fork without affecting the original world.
- **Indexed component fields:** Fields declared with the `indexed` keyword maintain a runtime hash index (`HashMap<IndexKey, Vec<u32>>`) for O(1) entity lookup via the `lookup()` builtin. The index uses `IndexValue` (int, str, bool, entity, float) for deterministic hashing. Indices are updated on `add_component`, `remove_component`, and `destroy_entity`.
- **Air-gap isolation:** Strict separation between ECS persistent storage and the execution stack (arena/GC). ECS writes (`set`/`spawn`) deep-copy values into persistent storage; ECS reads (`get`/`peek`) deep-copy values out into the GC heap. String fields cross the air gap in O(1) via `Arc<str>` sharing (only the `Arc` shell is cloned, not the bytes). This guarantees no dangling pointers survive fork/commit/despawn.
- **`ValueColumn`:** Custom wrapper around `Vec<Value>` in SoA columns with `Clone` (retains persistent `Arc<Object>` refs) and `Drop` (releases them). This ensures correct reference counting when columns are cloned via `Arc::make_mut` or dropped on despawn.
- **Bytecode constants and VM state:** The compiler still carries an internal [`GcHeap`](../../../core/vm/src/gc.rs) in [`CompileResult`](../../../core/vm/src/compiler/mod.rs), and [`VM::load_compile_result`](../../../core/vm/src/vm/mod.rs) verifies every chunk before merging it into the VM. Mutable `ChunkBuilder` construction artifacts never contain verification state; verification seals private immutable bytes, constants, and line metadata together with their proof. Converting a sealed chunk back to a builder discards the proof. Safe raw chunk loading accepts immediate constants only, while the internal heap-backed bundle transfer is explicitly unsafe. Raw ABI reconstruction is likewise unsafe because object-tagged values contain GC pointers. Chunk constants (including GC-allocated string literal parts from f-strings) are traced as roots by `collect_cycles` to prevent use-after-free during garbage collection. Large read-only runtime state (chunks, system maps, handler maps) is `Arc`-wrapped for cheap sharing across workers and snapshots.
- **Guarded `once` event handlers:** Guards remain an `if` wrapper in the AST/bytecode. The VM defers marking a `once` handler as **fired** until the guarded then-branch runs (signaled via `Op::OnceGuardPass`), and `dispatch_event` saves/restores guard bookkeeping across nested handler runs so inner flushes cannot retire an outer guarded `once` handler incorrectly. Implementation: [`dispatch_event`](../../../core/vm/src/vm/exec.rs).
- **Parallel ECS batches:** Conflict-free system batches may run in parallel on worker VMs with per-worker scratch memory (`BumpArena`). Worker results are deep-copied into persistent ECS storage (no worker heap merge step).
- **Collector scope:** The backup collector (`gc_collect`) traces stack, globals, captures, and bytecode chunk constants, and skips ECS world columns, snapshots, and event timelines. Chunk constants are roots because the compiler allocates string literal fragments (e.g. f-string parts) on the `GcHeap`. ECS data lifecycle is managed entirely by `Arc` refcounting via `ValueColumn`.
- **WASM / native integration:** The browser bridge translates JS data at its boundary. Rust hosts use owned [`FrozenValue`](../../../core/vm/src/host_value.rs) data or a `ValueHandle<'vm>` borrowed from one living VM; neither exposes a GC pointer.
- **Embeds / hosts:** `VM::import_value`, `call_global`, `export_global`, `component_value`, `resource_value`, and `enqueue_frozen_event` are the safe Rust surface. The internal NaN-boxed `Value`, `GcHeap`, raw calls, globals, component rows, and value codecs are crate-private. All floating NaNs are canonicalized before boxing so no IEEE payload can overlap an object tag.
- **Possible future layout:** Splitting stack/frames from `GcHeap` in the Rust VM could reduce borrow-checker friction in a few hot paths. That would be a larger internal refactor; behavior and bytecode would stay the same.

### Frozen legacy C backend

`core/c-backend/` is preserved historical code, not active architecture. It is
not the source of truth for parser, checker, runtime, WASM, playground, or
language support. See [C Backend Freeze](../project/c-backend-freeze.md).

The notes below describe the old C/AOT experiment for archaeology only. Normal
architecture work should use `core/vm/`.

### Historical C backend: `rad_dispatch_system` and Fast-Path Unrolling

The self-hosted C emitter (`core/c-backend/src/emit_c.rad`) generates **`rad_dispatch_system`** as an **O(1) sparse hash table** keyed by a **32-bit DJB2-style hash** of the system name, modulo a compile-time table size chosen so every zero-argument “system” function maps to a **distinct slot**. Dispatch is **hash → modulo → function pointer + expected name → one `strcmp` to verify** the string at that slot. This is not a classical minimal perfect hash with zero verification; the bounded `strcmp` confirms the slot matches the runtime string (including rejecting collisions from arbitrary input strings).

For maximum performance, the C backend also implements **Fast-Path Unrolling**. If `simulate()` is called with a static literal list of `system::…` references (e.g., `simulate(fork, [system::physics, system::render], 1)`), the compiler completely bypasses the dynamic dispatcher and emits inline, direct C function calls (`rad_u_physics(); rad_u_render();`), resulting in zero-overhead speculative execution.

### Historical C backend: variant canonicalization

The `rad_variant_of` function generated by the C emitter deduplicates variant map entries at emit time and prefers **short names** (e.g., `"Kw"`) over fully-qualified names (e.g., `"TokenKind::Kw"`) when both map to the same internal component ID. This ensures consistency between the Rust VM and C backend — the self-hosted parser's `variant_of` checks match regardless of which backend produced the token stream.

### Historical C backend: memory management

The C runtime (`runtime.c`) uses stack-local buffers and `malloc`/`free` for temporary allocations (string concatenation, float formatting, etc.) instead of a global scratch arena. The scratch-arena infrastructure remains available behind `#ifdef RAD_SCRATCH_ARENA` for potential future use. `runtime.h` exposes `RAD_DECL` declarations for separate-compilation ABI symbols (`g_rad_call_depth`, `RAD_MAX_CALL_DEPTH`, `rad_intern_string`).

### Historical C backend: file map and examples

The frozen C backend lives entirely under `core/c-backend/` and has three
categories: historical compiler/runtime source, old validation harnesses, and
focused reproductions.
Generated C and executables belong under `core/c-backend/target/`.

| Path | Category | Purpose |
|---|---|---|
| `core/c-backend/src/lexer.rad` | Frozen source | Historical self-hosted lexer and token definitions. |
| `core/c-backend/src/parser.rad` | Frozen source | Historical AST component schema and parser. |
| `core/c-backend/src/checker.rad` | Frozen source | Historical static checker used before C emission. |
| `core/c-backend/src/emit_c.rad` | Frozen source | Historical Rad-to-C emitter. |
| `core/c-backend/src/main.rad` | Frozen source | Historical CLI-style wrapper for `compile` and `compile-separate`. |
| `core/c-backend/src/runtime.c`, `runtime.h` | Frozen source | Historical C runtime for emitted programs. |
| `core/c-backend/src/tcc_compat.c` | Frozen source | TCC-on-Windows shim for the old conformance runner. |
| `core/c-backend/src/emit_wasm.rad`, `wasm_encode.rad` | Frozen source | Historical WASM lowering experiment. |
| `core/c-backend/test_conformance_c.py` | Frozen harness | Old batch C conformance runner; requires `RAD_RUN_FROZEN_C_BACKEND=1`. |
| `core/c-backend/test_c_backend.py` | Frozen harness | Old stress harness; requires `RAD_RUN_FROZEN_C_BACKEND=1`. |
| `core/c-backend/repro/` | Frozen repro | Historical reproductions. |

Example: emit one checked C file and compile it manually:

```powershell
cargo build -p rad-vm --release
target\release\rad.exe core\c-backend\src\main.rad compile tests\conformance\basic_arithmetic.rad core\c-backend\target\basic_arithmetic.c
gcc -O2 core\c-backend\target\basic_arithmetic.c -I core\c-backend\src -o core\c-backend\target\basic_arithmetic.exe
core\c-backend\target\basic_arithmetic.exe
```

Example: emit a separate-compilation bundle:

```powershell
target\release\rad.exe core\c-backend\src\main.rad compile-separate tests\conformance\test_separate_multi.rad core\c-backend\target\separate_test
```

Example: run the historical C-backend checks, only when intentionally
investigating the frozen backend:

```powershell
$env:RAD_RUN_FROZEN_C_BACKEND = "1"
py core\c-backend\test_conformance_c.py --compiler auto
py core\c-backend\test_c_backend.py --debug-arena
```

## Source layout

| Path | Purpose |
|---|---|
| `core/vm/src/main.rs` | `rad` CLI entry point |
| `rad` | The main Rad CLI (run, test, fmt, lint, new, snapshot, play, `build --target wasm`, `lsp`) |
| `core/vm/src/wasm_compiler_host.rs` | Phase 3: wasmtime host, `vfs_read`, guest `rad_*` exports (requires `native-wasm-phase3`) |
| [`docs/src/reference/wasm-phase3.md`](wasm-phase3.md) | Phase 3 ABI, `rad build`, LSP env vars (`RAD_WASM_PHASE3`, `RAD_COMPILER_WASM`, `RAD_VFS_ROOT`) |
| `core/vm/src/lexer.rs` | Tokenizer |
| `core/vm/src/parser.rs` | Token stream to AST |
| `core/vm/src/ast.rs` | AST node definitions (`Expr`, `Decl`, `ComponentEntry`, etc.) |
| `core/vm/src/checker/` | Static type checker |
| `core/vm/src/compiler/` | AST to bytecode compiler |
| `core/vm/src/vm/` | Bytecode virtual machine |
| `core/vm/src/world.rs` | ECS world (archetypes, SoA columns, CoW snapshots, indexed field hash maps, global resources) |
| `core/vm/src/wasm.rs` | WebAssembly bindings for browser |
| `core/vm/src/formatter.rs` | Native formatter used by `rad fmt` and LSP formatting |
| `core/vm/src/lsp.rs` | Language server implementation |
| `core/c-backend/` | Frozen legacy C/AOT backend experiment. Not part of normal development or health checks. |
| `rad lsp` | Language server (diagnostics, hover, go-to-def, completions, formatting) |
| `tooling/editors/vscode/` | VS Code extension (TextMate grammar, language config) |
| `projects/playground/` | Browser playground (HTML + WASM) |
| `rad snapshot` | Snapshot test runner |
| `tooling/scripts/rust_vm_locator.py` | Resolves the `rad` binary for Python tooling |

## Developer tooling

| Tool | What it does |
|---|---|
| `rad fmt` | Format `.rad` files natively |
| `rad fmt --check` | CI-friendly formatting check |
| `rad lint --preset P` | Lint with enterprise, strict, or teaching preset |
| `rad snapshot` | Verify `.rad` script output against stored `.snap` baselines |
| `rad build --target wasm` | After type-check, emit `compiler.wasm` (stub or copy from `RAD_COMPILER_WASM`) — see [Phase 3 WASM](wasm-phase3.md) |
| LSP (`rad lsp`) | Real-time diagnostics, hover, completions, go-to-def, formatting; optional WASM diagnostics via env vars in [Phase 3 WASM](wasm-phase3.md) |

## Testing

```bash
# Rust unit tests (from repository root; shared workspace target/)
cargo test -p rad-vm
# Alternative: cargo test --manifest-path core/vm/Cargo.toml
#
# Includes parser/checker/VM tests and DX/unit coverage for formatter (`formatter.rs`),
# linter (`linter.rs`), and LSP (`lsp.rs`) that previously lived in removed Python harnesses.

# Snapshot / conformance baselines — every .rad under the directory tree that has a sibling .snap
# is checked. CI runs: cargo run -p rad-vm --bin rad -- snapshot tests/
rad snapshot tests/
# After building: target/debug/rad snapshot tests/   (Windows: target\debug\rad.exe)
rad snapshot tests/ --create   # write missing .snap files
rad snapshot tests/ --update   # refresh all .snap files next to .rad sources

# Criterion benchmarks (core/vm/benches/rad_benchmarks.rs — includes pipeline & ECS examples)
cargo bench -p rad-vm --bench rad_benchmarks
# Alternative: cargo bench --manifest-path core/vm/Cargo.toml --bench rad_benchmarks
# If Windows reports "access denied" replacing `rad.exe`, close running `rad`/IDE locks on `target/release`, then retry.
py benches/compare.py

# World forking benchmarks (fork/simulate/commit/peek at scale)
rad benches/bench_fork.rad
rad benches/bench_fork_only.rad
```
