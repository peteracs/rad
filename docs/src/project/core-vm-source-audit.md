# Core VM Source Audit

This page is the working ledger for the 250-line source audit of
`core/vm/src`. Keep it current as files are reviewed, moved, deleted, or
documented. Do not create separate local audit notes.

Rules for this audit:

- Review source in chunks of about 250 lines.
- Delete or move code only after checking module references, tests, Cargo
  targets, docs, and runtime behavior.
- If a source feature is real and user-facing, update the relevant wiki page
  under `docs/src/` with examples.
- If a file is an artifact, deprecated, outdated, or superseded, record the
  reason here and remove it in the same change.

Status values:

- `Pending`: not reviewed yet.
- `Reviewed`: source read and no action needed.
- `Changed`: source was refactored, moved, or documented.
- `Deleted`: file was removed after verification.
- `Blocked`: needs a decision or failing verification.

## Inventory Summary

Generated from the current source tree on 2026-06-21 and updated after
twenty-third-shot WASM runtime review. Line counts use actual `Get-Content` entry
counts so files without trailing newlines are not undercounted. Deleted files
remain recorded in the file ledger with their reviewed deletion counts.

| Scope | Files | Lines | File-local chunks |
|---|---:|---:|---:|
| `core/vm/src` total | 79 | 76,895 | 350 |
| root files | 43 | 30,503 | 145 |
| `core/vm/src/bin` | 0 | 0 | 0 |
| `core/vm/src/checker` | 9 | 16,352 | 70 |
| `core/vm/src/compiler` | 11 | 9,933 | 45 |
| `core/vm/src/lexer` | 3 | 1,516 | 8 |
| `core/vm/src/parser` | 6 | 4,631 | 22 |
| `core/vm/src/vm` | 7 | 13,960 | 60 |

Planning estimate: 76,895 current raw lines is 308 continuous 250-line chunks.
The current source tree lists 350 file-local chunks because each file rounds up.
Use six raw chunks per review shot as the default pace, or about 1,500 lines per shot.
Dense runtime/checker files may need four chunks per shot; straightforward
tests may allow eight. From the beginning, that works out to roughly 51-58
source-review shots, plus 5-8 integration, deletion, reorganization, docs, and
full-test shots. After shot 23, every file under `core/vm/src` has been
reviewed at least once in this audit. The remaining work is the final
integration, deletion/reorganization sanity pass, and documentation sweep.

End-of-folder VM verification on 2026-06-21: `cargo test -p rad-vm` passed
with 908 library tests, 17 CLI tests, and 1 WASM host test; 3 benchmark tests
were ignored. The run emitted two pre-existing warnings in
`sheet_property_tests.rs`.

## Shot Log

| Shot | Lines reviewed | Files | Result |
|---|---:|---|---|
| 1 | 2,169 | `lib.rs`, small root support files, and `merkle.rs` | Shot over: small files were comfortable. Deleted unused `merkle.rs`, removed its export, documented `rad.toml [network]` limits, and verified with `cargo check -p rad-vm` plus `mdbook build docs`. |
| 2 | 3,574 | `ast.rs`, `lexer.rs`, `lexer/decl.rs`, `lexer/expr.rs`, `lexer/stmt.rs` | Shot over for syntax-layer files. No source deletion; fixed stale RADGUI docs for reserved field names and f-string string-literal indexing. Verified lexer tests and docs build. |
| 3 | 4,952 | `parser.rs`, `parser/decl.rs`, `parser/expr.rs`, `parser/recovery.rs`, `parser/stmt.rs`, `parser/tests.rs`, `parser/types.rs` | Shot over for the parser layer. No deletions; fixed `event fn` parsing so explicit Event effects reach the checker, added parser tests, and aligned the spec grammar/prose for parser-supported syntax. Verified parser tests and docs build. |
| 4 | 1,548 | `compiler/decl.rs`, `compiler/egraph.rs`, `compiler/emit.rs` | Normal compiler-density shot. No deletions; documented Rust VM declaration lowering, e-graph optimization, and label-safe bytecode peepholes in the architecture reference. Used cheap source/doc checks only under the faster cadence. |
| 5 | 1,723 | `compiler/escape.rs`, `compiler/expr.rs` | Normal compiler-density shot. No deletions; documented Rust VM expression lowering and static escape-analysis roles in the architecture reference. Existing docs already covered user-facing pipeline vectorization, query structural exclusion, `debug_trace` release stripping, and bitset value semantics. Used cheap source/doc checks only under the faster cadence. |
| 6 | 1,490 | `compiler/layout_analysis.rs`, `compiler/materialization.rs`, `compiler/mod.rs`, `compiler/pipeline.rs` | Normal compiler-density shot. No deletions; documented compiler state/result assembly, SoA/AoS layout analysis, materialization planning, and scalar pipeline fallback in the architecture reference. Used cheap source/doc checks only under the faster cadence. |
| 7 | 1,405 | `compiler/stmt.rs` | Normal compiler-density shot. No deletions; documented statement bytecode lowering and mutable-query writeback behavior in the architecture reference. Existing user docs already covered `let rec`, update blocks, delayed emits, mutable queries, schedules, and match patterns. Used cheap source/doc checks only under the faster cadence. |
| 8 | 3,767 | `compiler/tests.rs` | Shot over for test inventory. No deletions; reviewed 234 compiler/VM execution tests covering closure/upvalue behavior, `let rec`, mutability diagnostics, loops, pipelines, ECS/query writebacks, tuples/destructuring, entity literals, f-string formatting, numeric/runtime bug regressions, math, and JSON behavior. Existing guide/reference docs already cover the user-facing features. Used cheap source/doc checks only under the faster cadence. |
| 9 | 1,953 | `checker/declarations.rs`, `checker/diagnostics.rs`, `checker/match_test.rs` | Normal checker-density shot. No deletions; documented checker declaration/effect passes and diagnostics classification in the architecture reference, and fixed stale wiki pages that still said `simulate()` suppresses or forbids all event emissions. Used cheap source/doc checks only under the faster cadence. |
| 10 | 2,190 | `checker/mod.rs`, `checker/reachability.rs`, `checker/resolve.rs`, `checker/scope.rs` | Shot over for checker core support. Removed a duplicate cross-file helper superseded by the shared checker helper; documented checker orchestration, reachability, type resolution, and scope support in the architecture reference; tightened dead-code roots in the spec and added a module-boundary/private-type example. Used cheap source/doc checks only under the faster cadence. |
| 11 | 5,434 | `checker/tests.rs` | Shot over for test inventory. No deletions; reviewed 172 checker tests covering mutability, reachability, match exhaustiveness/destructuring, state machines, effects/pipelines, `simulate` system refs and handler safety, strict/public typing, aliases, resources, query select, `let rec`, destructured closures, collection typing, unique bindings, bitwise ops, `self`, filtered loops, delayed emits, and diagnostic hints. Existing guide/reference docs already cover the user-facing features. Used cheap source/doc checks only under the faster cadence. |
| 12 | 6,782 | `checker/typeck.rs` | Shot over for dense checker semantics. No deletions; reviewed the main statement/expression type checker covering module contracts, declarations, systems/resources, migrations, handlers, `let`/`let-else`, assignments, narrowing, loops, emits/schedules/updates, match exhaustiveness/destructuring, pipelines/effects, async/await, spreads, aliases, unique bindings, typed builtins, and diagnostic hints. Architecture/module docs now categorize the typechecker and contract surface. Used cheap source/doc checks only under the faster cadence. |
| 13 | 2,974 | `bench_tests.rs`, `bin/test_wasm.rs`, `builtins.rs`, `causality.rs` | Shot over for benchmark/runtime support. Deleted obsolete auto-discovered `test_wasm` manual smoke binary; `RadRuntime` is covered by the WASM runtime tests and docs. Reviewed ignored release benchmark tests, the builtin signature/effect/name registry, and the causality ledger/wire-provenance implementation. Builtins docs now cover previously missing buffer, byte-string, host/runtime builtins. Used cheap source/doc checks only under the faster cadence. |
| 14 | 3,711 | `composition_tests.rs`, `formatter.rs`, `fuzz_tests.rs`, `index_tests.rs` | Shot over for validation/tooling coverage. No deletions; reviewed cross-feature composition tests, the token-aware native formatter, decode-boundary fuzz/soundness gates, and indexed-query survival tests. Existing guide/reference docs already cover formatter behavior, indexed fields/lookup semantics, delta/merge/why surfaces, and fuzz/validation context. Used cheap source/doc checks only under the faster cadence. |
| 15 | 2,129 | `leak_lab.rs`, `linter.rs`, `lsp.rs` | Normal tooling-density shot. Added the missing LSP document-formatting handler so the wiki claim that LSP format-on-save uses the native formatter is true. Reviewed the leak-lab memory-slope harness, lint presets/AST lints, and LSP diagnostics, hover, definition, completion, and open-document overlay behavior. Used cheap source/doc checks only under the faster cadence. |
| 16 | 3,806 | `main.rs`, `merge.rs`, `migration_tests.rs` | Shot over for CLI/runtime-edge files. No source deletion; reviewed CLI command parsing/dispatch, diagnostics, lockfile writing, trace record/replay, sandbox/replay servers, three-way world merge, conflict resolution, schema migration, and convergence receipt tests. Flagged the lockfile docs/source surface for the module-loader follow-up in shot 17. Used cheap source/doc checks only under the faster cadence. |
| 17 | 3,681 | `module_loader.rs`, `opcode.rs`, `radpack.rs`, `replay.rs`; `main.rs` lockfile follow-up | Shot over for module/replay files. Removed the superseded legacy `forge.lock` writer in `main.rs` and routed `--write-lock` through `LockFile::generate`/`write_lockfile`, preserving SHA-256 pins verified by the loader. Reviewed module graph loading, lockfile parsing/verification, remote fetch limits, opcode compatibility reservations, RADPACK envelopes, and record/replay/retroactive replay. Lockfile docs now consistently document byte counts, FNV-1a checksums, and SHA-256 pins. Used cheap source/doc checks only under the faster cadence. |
| 18 | 3,446 | `replay_serve.rs`, `sandbox.rs`, `sandbox_serve.rs`, `sheet_property_tests.rs`, `types.rs`, `visitor.rs` | Shot over for sandbox/type support. No source deletion; reviewed replay JSON-RPC time travel, sandbox capabilities/escape tests, sandbox JSON-RPC serving, Radsheet property fuzz, type/effect/substitution structures, and AST visitor traversal. Removed a stale internal QA label from a replay-server test comment and corrected enterprise map-key docs to match current deterministic data-key support. Used cheap source/doc checks only under the faster cadence. |
| 19 | 4,816 | `value.rs`, `wire.rs`, `world.rs` | Shot over for value/world runtime files. Removed duplicate `trace_id`/`flush_events` entries from `Builtin::ALL` and added a local invariant test so builtin global slots stay one-per-name. Reviewed NaN-boxed values, persistent storage, deterministic map keys, canonical fork/delta wire codecs, SoA world storage, resources, indices, snapshots, semantic diffs, and copy-on-write world restore. Existing wiki pages already cover the user-facing memory model, ECS, event flush, wire, digest, and merge surfaces. Used cheap source/doc checks only under the faster cadence. |
| 20 | 8,505 | `vm/builtins_impl.rs` | Shot over for the full builtin runtime implementation. No deletions; reviewed sandbox/replay interposition, GC-paused builtin dispatch, structured output, event flushing, async/file/HTTP/TCP host builtins, higher-order collections, ECS/resource/entity helpers, fork/simulate/commit, merge conflict values, sandbox guests, fork/wire/delta/save/load codecs, schema migration, causality/diff helpers, bitsets, and formatting. Fixed async `list_dir()` so awaiting it returns the documented `list<str>` shape, and updated the wiki for `trace_id()` nilability plus string-keyed `log`/`metric` fields. Used cheap source/doc checks only under the faster cadence. |
| 21 | 2,243 | `vm/builtins_tests.rs`, `vm/helpers.rs`, `vm/io_pool.rs`, `vm/mod.rs`, `vm/parallel.rs` | Shot over for VM support-state files. No deletions; reviewed builtin runtime tests, arithmetic/comparison helpers, native async I/O worker pool, system conflict partitioning, and VM shared state/load/run/snapshot/replay/task/RNG plumbing. Fixed pooled worker VM shared-state refresh so transient-resource metadata follows the program tables it protects. Existing wiki pages already cover tuple arithmetic, bitwise ops, deterministic ordering, async host I/O, transient resources, and system parallelism. Used cheap source/doc checks only under the faster cadence. |
| 22 | 3,213 | `vm/exec.rs` | Shot over for the VM execution loop. No source deletion; reviewed worker VM pooling, fuel and GC charge points, opcode dispatch, calls/closures/native calls, async await payload conversion, collection/index forms, ECS/resource/query operations, state transitions, system scheduling, event dispatch, causal writeback, vectorized query helpers, and in-place bitset/buffer/list fast paths. Corrected the wiki for native parallel schedule batches and the delayed-emit limitation inside parallel batches. Used cheap source/doc checks only under the faster cadence. |
| 23 | 1,457 | `wasm.rs` | Shot over for the WASM/browser runtime bridge. No source deletion; reviewed `RadRuntime` compile/check/run APIs, streaming sessions, deltas, state load/save, render deltas, undo/redo, inspect, preview forks, timeline tracing, bytecode chunk construction, opcode-name bridge, and WASM runtime tests. Expanded the embedding API reference so the documented host contract matches the exported methods. Used cheap source/doc checks only under the faster cadence. |

## File Ledger

| File | Lines | Chunks | Status | Notes |
|---|---:|---:|---|---|
| `core/vm/src/arena.rs` | 92 | 1 | Reviewed | Live per-system `BumpArena`; covered by memory-model/value-semantics docs. |
| `core/vm/src/ast.rs` | 791 | 4 | Reviewed | Live AST/source-map layer for declarations, entity/resource literals, `let-else`, f-string parts, type expressions, aliases, and component entries; covered by spec, types, ECS, and DX docs. |
| `core/vm/src/bench_tests.rs` | 452 | 2 | Reviewed | Live ignored release benchmark harness for fork/commit, wire encode/decode, delta sync, merge/diff, save/load, events, causality ledger memory, and 1M-entity scale receipts; performance docs already point to the benchmark outputs. |
| `core/vm/src/bin/test_wasm.rs` | 16 | 1 | Deleted | Removed obsolete manual `RadRuntime` smoke binary. Cargo auto-discovered it as an extra bin, while the maintained WASM/runtime coverage lives in `wasm.rs`, playground/embed docs, and examples. |
| `core/vm/src/builtins.rs` | 1700 | 7 | Changed | Live builtin metadata registry for checker/runtime names, type schemes, effect classification, signature help, and `Builtin` return types; filled reference-doc gaps for buffer, byte-string, host/runtime builtins. |
| `core/vm/src/causality.rs` | 806 | 4 | Reviewed | Live provenance ledger behind `why()`/`why_resource`, event/write cause chains, fork wire provenance closures, foreign emit-id remapping, retention eviction, commit-seam disclosure, and causality tests; builtins/performance/example docs already cover the user-facing surface. |
| `core/vm/src/checker/declarations.rs` | 1635 | 7 | Reviewed | Live declaration/effect pass for collecting declarations, public API annotation enforcement, component/resource conflicts, defaultable and indexed fields, aliases/sum types, system dependencies, purity/readonly/effect inference, simulation handler-chain safety, and order-independent effect refinement; architecture and simulation docs now reflect it. |
| `core/vm/src/checker/diagnostics.rs` | 290 | 2 | Reviewed | Live checker diagnostics/effect helper layer for return merging, builtin purity/readonly/effect classification, immutable-transform detection, "did you mean" suggestions, and type-conversion hints; architecture docs now categorize it. |
| `core/vm/src/checker/match_test.rs` | 28 | 1 | Reviewed | Live focused checker regression for unreachable match arms. |
| `core/vm/src/checker/mod.rs` | 960 | 4 | Reviewed | Live checker orchestrator for built-in `Option`/`Result`/`Conflict` sum types, module alias registration/body checks, checker output, event handler indexing for simulation safety, effect refinement ordering, unused-system warnings, and static `simulate` system-reference invocation collection; architecture docs now categorize it. |
| `core/vm/src/checker/reachability.rs` | 727 | 3 | Reviewed | Live public API leak checker and dead-code reachability pass over functions, components, events, structs, aliases, emits, public events, systems, tests, top-level statements, type annotations, and literals; spec docs now state the actual roots. |
| `core/vm/src/checker/resolve.rs` | 293 | 2 | Changed | Live type-expression and literal-type resolver for builtins, generic aliases, `world_fork`, `system`, private cross-file type checks, component/struct/sum literals, and system refs; removed its duplicate cross-file helper in favor of the shared checker helper. |
| `core/vm/src/checker/scope.rs` | 203 | 1 | Reviewed | Live lexical scope support for unused binding warnings, shadowing diagnostics, builtin-name shadowing warnings, read tracking, unique bindings, and pipeline-local binding checks; architecture docs now categorize it. |
| `core/vm/src/checker/tests.rs` | 5434 | 22 | Reviewed | Live checker regression inventory with 172 tests for mutability, reachability, match/state/sum-type behavior, effect and pipeline safety, `simulate` system refs and handler-chain checks, strict/public typing, alias resolution, resources, query projection, recursive lets, destructured closures, collection typing, uniqueness, bitwise and shift ops, system `self`, filtered loops, delayed emits, and actionable diagnostic hints; no stale test island found. |
| `core/vm/src/checker/typeck.rs` | 6782 | 28 | Reviewed | Live statement/expression type checker for module contracts, declaration bodies, systems/resources, migrations, handlers, `let`/`let-else`, assignments, guard narrowing, loops, emits/schedules/updates, match exhaustiveness/destructuring, pipelines and effects, async/await, spreads, aliases, unique bindings, typed builtins, and diagnostic hints; architecture/module docs now categorize the surface. |
| `core/vm/src/compiler/decl.rs` | 459 | 2 | Reviewed | Live declaration lowering into globals, chunks, state transitions, systems, migrations, event handlers, functions, and tests; architecture docs now categorize the compiler layer. |
| `core/vm/src/compiler/egraph.rs` | 741 | 3 | Reviewed | Live e-graph optimizer for integer/boolean expressions and logical field/index loads/stores; architecture docs now cover its rewrite scope and bitwise fallback. |
| `core/vm/src/compiler/emit.rs` | 348 | 2 | Reviewed | Live bytecode emission helpers for label-safe `GetLocal2` and compare-branch peepholes, `PopN`, locals, scopes, and upvalues; architecture docs now cover the optimization surface. |
| `core/vm/src/compiler/escape.rs` | 293 | 2 | Reviewed | Live static escape analyzer for non-escaping bitset/buffer locals; architecture docs now tie it to in-place update opcode selection while preserving value semantics. |
| `core/vm/src/compiler/expr.rs` | 1430 | 6 | Reviewed | Live expression bytecode emitter for constants, calls, spreads, aliases, component/variant/state/entity/query/match/function expressions, `debug_trace` release stripping, query negation hoisting, and vectorized/scalar pipelines; architecture docs now categorize this layer. |
| `core/vm/src/compiler/layout_analysis.rs` | 478 | 2 | Reviewed | Live SoA/AoS vote analysis over checker output, effects, systems, and containment; architecture docs now describe the layout-analysis role. |
| `core/vm/src/compiler/materialization.rs` | 71 | 1 | Reviewed | Live `CompileResult` materialization plan for SoA types crossing AoS boundaries; retained despite limited current call sites because it is assembled from active layout analysis. |
| `core/vm/src/compiler/mod.rs` | 793 | 4 | Reviewed | Live compiler state/result assembly for globals, locals/upvalues, aliases, file-private mangling, checker output, layouts, transient resources, variants, metadata, warnings, and compile-time heap. |
| `core/vm/src/compiler/pipeline.rs` | 148 | 1 | Reviewed | Live scalar lowered pipeline fallback for stack-safe non-vectorized map/filter chains; architecture docs now document its loop/result-list lowering. |
| `core/vm/src/compiler/stmt.rs` | 1405 | 6 | Reviewed | Live statement bytecode lowering for declarations-in-body, destructuring, recursive lets, assignments/writebacks, loops, counted ranges, mutable query loops, returns/breaks/continues, emits, schedules/phases, update sugar, and match statements; architecture docs now categorize the layer. |
| `core/vm/src/compiler/tests.rs` | 3767 | 16 | Reviewed | Live compiler/VM execution regression inventory with 234 tests for closures/upvalues, recursive lets, mutability, loops, pipelines, ECS/query writebacks, tuples/destructuring, entity literals, f-string formatting, runtime bug fixes, math, and JSON behavior; no stale test island found. |
| `core/vm/src/compiler_abi.rs` | 60 | 1 | Reviewed | Live Phase 3 WASM ABI constants/diagnostic layout; covered by `wasm-phase3.md`. |
| `core/vm/src/composition_tests.rs` | 1808 | 8 | Reviewed | Live cross-feature regression suite for runtime diagnostics, auto-GC around nested builtins, fork/commit/merge event queues, simulation events, causality commit seams, replay/retro-edit/migration, sandbox event inheritance, fork wire codec, cross-VM merge, schema drift, default-filled literals, causality retention, merge conflicts/resolution, cross-machine `why()`, delta sync, digest receipts, field-level patches, and delta/merge equivalence. |
| `core/vm/src/determinism.rs` | 148 | 1 | Reviewed | Test-only determinism tripwire for replay/world digest behavior. |
| `core/vm/src/ffi.rs` | 231 | 1 | Reviewed | Live native plugin bridge used by `load_plugin`; changelog mentions heap merge behavior. |
| `core/vm/src/formatter.rs` | 518 | 3 | Reviewed | Live token-aware `rad fmt` engine shared with LSP formatting: bracket-stack indentation, BOM/CRLF preservation, triple-f-string/comment safety, operator spacing, unary-minus handling, `>>` adjacency, and idempotence tests; developer-tool docs already cover the formatter surface. |
| `core/vm/src/fuzz_tests.rs` | 848 | 4 | Reviewed | Live D2 soundness gate for malformed external inputs: structure-aware mutation/resealing of radpack, fork, delta, save/load, drifted schemas, merge survivors, raw tape envelopes, adversarial semantic payloads, and GC-pressure codec/merge paths; no committed corpus artifact is stored here. |
| `core/vm/src/gc.rs` | 177 | 1 | Reviewed | Live backup heap/capture-cell store; covered by architecture and memory-model docs. |
| `core/vm/src/index_tests.rs` | 537 | 3 | Reviewed | Live indexed-query battle tests for `indexed`, `lookup`, `lookup_all`, duplicate-key ordering, string/bool/float/entity key edges, loud unindexed-field errors, index maintenance across set/update/remove/despawn/id reuse, fork/commit, wire, save/load, delta, merge, migration, GC pressure, and replay without checker output. |
| `core/vm/src/leak_lab.rs` | 307 | 2 | Reviewed | Live test-only memory-slope harness with a counting allocator and shared lab lock, covering syncdesk/DPUSH fork, commit, wire, delta, merge, and note-cycle paths; retained the ignored report test and the active flat-memory regression. |
| `core/vm/src/lexer.rs` | 1267 | 6 | Reviewed | Live token definitions, recovery-friendly scanner, and lexer test suite; docs already cover syntax behavior, with stale RADGUI f-string note fixed. |
| `core/vm/src/lexer/decl.rs` | 285 | 2 | Reviewed | Live declaration keyword scanner, reserved-keyword rename hints, and f-string start helper. |
| `core/vm/src/lexer/expr.rs` | 263 | 2 | Reviewed | Live cursor, string/number, and comment-preservation helpers shared by the lexer and formatter. |
| `core/vm/src/lexer/stmt.rs` | 968 | 4 | Reviewed | Live main tokenizer, f-string/triple-f-string scanner, multiline string handling, operators, and escaped quote interpolation behavior. |
| `core/vm/src/lib.rs` | 56 | 1 | Changed | Removed stale `merkle` export after deleting unused module. |
| `core/vm/src/linter.rs` | 668 | 3 | Reviewed | Live `rad lint` preset engine and AST linter for standard/enterprise/strict/teaching policy, line limits, naming, teaching hints, effect suggestions, imperative collection loops, bare prints, aliased imports, and boundary checks; developer-tool docs already cover the surface. |
| `core/vm/src/lsp.rs` | 1154 | 5 | Changed | Live LSP backend for native/WASM diagnostics, open-document overrides, hover, definition, completion, and formatting; added document-formatting support so format-on-save docs match source behavior. |
| `core/vm/src/main.rs` | 1949 | 8 | Changed | Live CLI entrypoint for run, fmt, lint, lsp, test, new, snapshot, play, build, sandbox, replay, version, checker/compiler/VM dispatch, trace record/replay, lockfile writing, diagnostic formatting, and CLI parser tests; removed the stale TOML-ish lockfile helper and now writes the loader's `rad-lock` format with SHA-256 pins. |
| `core/vm/src/manifest.rs` | 125 | 1 | Changed | Live `rad.toml` network limits parser used by module loading; documented `[network]` limits in `guide/modules.md`. |
| `core/vm/src/merge.rs` | 1340 | 6 | Reviewed | Live three-way world merge engine for field/resource conflicts, entity-id collision remaps, name-claim and rename resolutions, event/delayed timer merge honesty, deterministic apply, and regression tests; builtins/performance/example docs already cover the user-facing surface. |
| `core/vm/src/merkle.rs` | 33 | 1 | Deleted | Unused generic Blake3/Merkle helper; superseded by direct `blake3` digest paths in `world`, `radpack`, `replay`, and builtins. `cargo check -p rad-vm` passed after removal. |
| `core/vm/src/migration_tests.rs` | 501 | 3 | Reviewed | Live save/load and schema-migration regression suite covering field order, added/renamed fields, resource migration, loud shape drift, provenance after load, in-language roundtrips, schema digests, fork world digests, and cross-version convergence certification. |
| `core/vm/src/module_loader.rs` | 1652 | 7 | Changed | Live module graph loader for `use`, source maps, aliases, duplicate/public symbol checks, `rad.toml` network policy, `forge.lock` parse/verify/write, SHA-256-pinned remote fetch/cache, and loader regression tests; added lockfile roundtrip coverage for SHA-256 pins. |
| `core/vm/src/opcode.rs` | 344 | 2 | Reviewed | Live bytecode opcode enum and chunk constant pool; reserved/deprecated opcodes are intentionally retained for bytecode compatibility and not emitted by current compiler paths. |
| `core/vm/src/parser.rs` | 321 | 2 | Reviewed | Live parser state, source spans, recovery hooks, keyword-as-field handling, and braced-literal disambiguation helpers. |
| `core/vm/src/parser/decl.rs` | 934 | 4 | Changed | Live declaration parser; fixed `event fn` effect disambiguation while preserving `event Name { ... }`; spec grammar updated. |
| `core/vm/src/parser/expr.rs` | 1099 | 5 | Changed | Live expression parser for precedence, `system::`, entity literals, f-strings, if-expressions, accessor closures, query expressions, spreads, and component updates; spec grammar/prose updated. |
| `core/vm/src/parser/recovery.rs` | 52 | 1 | Reviewed | Live top-level error recovery with capped diagnostics; covered by developer tools/spec recovery docs and parser tests. |
| `core/vm/src/parser/stmt.rs` | 878 | 4 | Changed | Live statement parser for `update`, `let-else`, destructuring, filtered loops, delayed emits, schedules, match patterns, and append sugar; spec grammar/prose updated. |
| `core/vm/src/parser/tests.rs` | 1547 | 7 | Changed | Live parser behavior inventory; added coverage for `event fn` vs event declarations and combined effects. |
| `core/vm/src/parser/types.rs` | 121 | 1 | Reviewed | Live type-expression parser for unions, generics, tuples, and function types; covered by spec/type docs and parser tests. |
| `core/vm/src/play.rs` | 181 | 1 | Reviewed | Live native `rad play` playground server called by CLI. |
| `core/vm/src/radpack.rs` | 397 | 2 | Reviewed | Live deterministic RADPACK text/file envelope codec with legacy pass-through, digest verification, inflate ceilings, zstd/deflate file variants, and property-style codec tests. |
| `core/vm/src/replay.rs` | 1288 | 6 | Reviewed | Live record/replay engine for deterministic boundary capture, tagged value trace codec, strict replay divergence checks, frame seeking, retroactive replay oracle mode, end-digest reports, and replay regression tests. |
| `core/vm/src/replay_serve.rs` | 588 | 3 | Changed | Live `rad replay --serve` JSON-RPC time-travel server for info, frame seeking, peeking, frame diffs, `why`, crash timelines, and protocol tests; cleaned one stale internal QA label from a test comment. |
| `core/vm/src/sandbox.rs` | 852 | 4 | Reviewed | Live sandbox capability model, denied-builtin mask, JSON grants, deterministic fork seed derivation, escape tests, `simulate_par` determinism/isolation tests, and blast-radius assertion coverage. |
| `core/vm/src/sandbox_serve.rs` | 481 | 2 | Reviewed | Live `rad sandbox serve` JSON-RPC host protocol for propose, peek, commit, drop, shutdown, per-request caps, structured guest output, fork storage, and protocol tests. |
| `core/vm/src/scaffold.rs` | 128 | 1 | Reviewed | Live `rad new` scaffold command called by CLI; docs mention templates at a high level. |
| `core/vm/src/sheet_property_tests.rs` | 462 | 2 | Reviewed | Live Radsheet dogfood property suite using the shipped `lib_sheet.rad`: random formula/edit/merge scenarios, derive invariants, range-vs-chain SUM checks, algebraic identities, reflow idempotence, merge re-derivation, record/replay determinism, and planted-bug proof. |
| `core/vm/src/simulate_syntax.rs` | 249 | 1 | Reviewed | Live static schedule classifier shared by checker/compiler/LSP; documented in DX updates/changelog/spec references. |
| `core/vm/src/snapshot.rs` | 288 | 2 | Reviewed | Live `rad snapshot` runner called by CLI; documented in developer tools and architecture docs. |
| `core/vm/src/types.rs` | 681 | 3 | Changed | Live type/effect/checker-output definitions for effects, assignability, unions, valid map keys, generic apps, substitutions, component/resource/system/sum metadata, and simulation breach metadata; corrected enterprise docs that still said maps were string-keyed only. |
| `core/vm/src/value.rs` | 2145 | 9 | Changed | Live NaN-boxed `Value` model, persistent object store, copy-on-write lists, HAMT maps with deterministic scalar/tuple keys, component/sum/closure values, and builtin enum/name registry; removed duplicate `trace_id`/`flush_events` entries from `Builtin::ALL` and added a uniqueness invariant test. Memory-model, spec, architecture, ECS, event, and builtins docs already cover the user-facing behavior. |
| `core/vm/src/visitor.rs` | 382 | 2 | Reviewed | Live unified AST visitor/walker used by linter and analysis passes, covering declarations, statements, expressions, type expressions, patterns, calls, schedules, and function declarations. |
| `core/vm/src/vm/builtins_impl.rs` | 8504 | 35 | Changed | Live builtin runtime dispatch and implementation surface for sandbox/replay interposition, event flushing, host I/O, collections, ECS/resources, speculative execution, sandboxing, wire/delta/save/load, schema migration, causality, bitsets, and formatting; fixed async `list_dir()` to preserve the documented `list<str>` await result and updated observability docs. |
| `core/vm/src/vm/builtins_tests.rs` | 568 | 3 | Reviewed | Live builtin runtime unit coverage for stack/list helpers, file/string/regex helpers, module require helpers, HTTP error shaping, deterministic random helpers, conversion helpers, and collection transforms; no stale test island found. |
| `core/vm/src/vm/exec.rs` | 3213 | 13 | Reviewed | Live VM execution loop for worker pooling, budget checks, bytecode dispatch, function/closure/native calls, async tasks, indexing, ECS/resource/query ops, state transitions, schedules, event dispatch, causal writes, vectorized operations, and in-place fast paths; schedule and delayed-event docs now match runtime behavior. |
| `core/vm/src/vm/helpers.rs` | 467 | 2 | Reviewed | Live VM helper layer for constants, component/index checks, arithmetic, tuple elementwise operations, bitwise/shift operations, total ordering, comparisons, and value equality. |
| `core/vm/src/vm/io_pool.rs` | 75 | 1 | Reviewed | Live native async I/O worker pool with clean shutdown and a wasm no-worker stub. |
| `core/vm/src/vm/mod.rs` | 1072 | 5 | Changed | Live VM state/shared-state definition and load/run plumbing for globals, chunks, worlds, snapshots, replay, async tasks, RNG, resources, tracing, sandboxing, and workers; fixed worker shared-state refresh so transient resources stay aligned with program metadata. |
| `core/vm/src/vm/parallel.rs` | 61 | 1 | Reviewed | Live deterministic system partitioning by component/resource read-write conflicts for parallel runtime batches. |
| `core/vm/src/wasm.rs` | 1457 | 6 | Reviewed | Live browser/native `RadRuntime` bridge for compile/check/run, streaming sessions, host-pushed events, fork deltas, state handshakes, render deltas, undo/redo, inspect/why, speculative preview, timeline tracing, `WasmChunk` bytecode construction, opcode-name parsing, and runtime tests; embedding API docs now cover the exported host contract. |
| `core/vm/src/wasm_binary_emit.rs` | 107 | 1 | Reviewed | Live Phase 3 compiler reactor stub emitter used by `rad build --target wasm` fallback. |
| `core/vm/src/wasm_compiler_host.rs` | 294 | 2 | Reviewed | Live optional native wasmtime host for compiler reactor diagnostics/LSP. |
| `core/vm/src/wire.rs` | 621 | 3 | Reviewed | Live canonical fork/delta wire codec for world snapshots, events, delayed timers, provenance, tuple map keys, deterministic field ordering, digest verification, and migration-aware decode/apply paths; builtins/tutorial docs already cover the user-facing wire and delta sync surfaces. |
| `core/vm/src/world.rs` | 2050 | 9 | Reviewed | Live SoA ECS world with copy-on-write archetypes, `ValueColumn` persistent refcounts, resources, indexed fields, entity/name maps, snapshots, JSON/digest output, semantic diffs, touched-entity detection, and world restore; architecture, memory-model, ECS, events, and builtins docs already cover the public behavior. |

## Review Order

Start with the small/root shape files, then move through parser/compiler/checker
and finally the heavy VM runtime files:

1. Root overview and small files: `lib.rs`, `arena.rs`, `compiler_abi.rs`,
   `determinism.rs`, `ffi.rs`, `gc.rs`, `manifest.rs`, `play.rs`,
   `scaffold.rs`, `simulate_syntax.rs`, `snapshot.rs`,
   `wasm_binary_emit.rs`, `wasm_compiler_host.rs`.
2. Syntax and AST: `ast.rs`, `lexer.rs`, `lexer/`, `parser.rs`, `parser/`.
3. Compiler: `compiler/` and compiler-facing tests.
4. Checker: `checker/` and checker-facing tests.
5. Runtime/data model: `builtins.rs`, `value.rs`, `world.rs`, `opcode.rs`,
   `vm/`, `module_loader.rs`, `merge.rs`, `causality.rs`, `wire.rs`.
6. Tooling/embedding/tests: `main.rs`, `formatter.rs`, `linter.rs`, `lsp.rs`,
   `wasm.rs`, `replay*`, `sandbox*`, and remaining test files.

## Documentation Targets

For each reviewed feature, check these wiki targets:

- Syntax and semantics: `docs/src/reference/spec.md` and the relevant guide.
- Builtins and embedding APIs: `docs/src/reference/builtins.md`.
- Runtime architecture: `docs/src/reference/architecture.md`.
- Memory and value layout: `docs/src/reference/memory-model.md`.
- WASM compiler/runtime path: `docs/src/reference/wasm-phase3.md`.
- Project/repo structure: `docs/src/project/repo-map.md` and
  `docs/src/project/repo-audit.md`.
