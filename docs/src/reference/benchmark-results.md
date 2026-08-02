# World Forking Benchmark Results

## Test Environment
- **VM**: Rad bytecode VM (release build)
- **Workload**: 3 components/entity (Pos, Vel, Stats), Physics system mutates Pos only
- **System**: Windows 10

## Before vs After: Copy-on-Write Snapshot Architecture

### Main Benchmark (release build)

| Operation       | Entities | Before CoW (ms) | After CoW (ms) | Speedup |
|----------------|----------|-----------------|----------------|---------|
| Fork           | 100      | 0               | 0              | —       |
| Fork           | 1,000    | 0               | 0              | —       |
| Fork           | 10,000   | 4               | 0              | >4x     |
| Simulate (1t)  | 100      | 0               | 0              | —       |
| Simulate (1t)  | 1,000    | 5               | 3              | 1.7x    |
| Simulate (1t)  | 10,000   | 104             | 71             | 1.5x    |
| Simulate (10t) | 100      | 5               | 2              | 2.5x    |
| Simulate (10t) | 1,000    | 39              | 28             | 1.4x    |
| Simulate (10t) | 10,000   | 1,565           | 1,164          | 1.3x    |
| Peek (×100)    | 10,000   | 2               | 0              | >2x     |
| Commit         | 10,000   | 4               | 0              | >4x     |

### Fork-Only Benchmark (isolates snapshot cost)

| Entities | Before CoW: 50 forks (ms) | After CoW: 1000 forks (ms) | Per-fork before | Per-fork after | Speedup |
|----------|--------------------------|---------------------------|-----------------|----------------|---------|
| 1,000    | 7                        | 4                         | 140µs           | 4µs            | **35x** |
| 5,000    | 58                       | 4                         | 1,160µs         | 4µs            | **290x**|
| 10,000   | 174                      | 7                         | 3,480µs         | 7µs            | **497x**|

## Analysis

**Fork cost is now nearly independent of world size.** Before CoW, fork cost scaled linearly
with entity count (deep-copying all component data). After CoW, fork only performs O(A) `Arc`
refcount bumps on column handles and world maps — the actual data is shared and only cloned
on first mutation via `Arc::make_mut`, which triggers an O(E) retain scan on the `ValueColumn`
to manage persistent `Arc<Object>` refcounts for heap-backed values.

**Simulate sees a ~1.3-1.5x improvement** because only the mutated column (Pos) is cloned,
while untouched columns (Vel, Stats) remain shared. The remaining simulate time is dominated
by bytecode interpretation overhead, not data copying. String fields use `Arc<str>`, so
cloning a column with strings pays only atomic refcount bumps per string, not byte copies.

**Peek and Commit dropped to sub-millisecond** at 10k entities. `peek` deep-copies component
fields across the air gap (O(F) per component, with strings O(1) via `Arc<str>`). `commit`
is an O(1) pointer swap of `Arc`-wrapped maps.

## Architecture

- `SoAColumn.fields`: per-field `Arc<ValueColumn>` wrapping (custom `Clone`/`Drop` for persistent `Arc<Object>` lifecycle)
- `Archetype.entities`: `Arc<Vec<u32>>` — entity list shared across forks
- `Archetype.entity_row`: `Arc<HashMap<u32, usize>>` — row index shared
- `World.entity_archetype`: `Arc<HashMap<u32, ArchetypeId>>` — shared
- `World.name_to_id / id_to_name`: `Arc<HashMap<...>>` — shared
- `World.type_registry`: `Arc<HashMap<String, TypeId>>` — shared
- `World.archetype_map`: `Arc<HashMap<Vec<TypeId>, ArchetypeId>>` — shared

All mutations go through `Arc::make_mut()` which only clones when the refcount > 1.

---

## C Backend Stress Test Results

Historical note: `core/c-backend/` is frozen legacy code. These numbers are
kept for archaeology and are not current project health criteria.

### Test Environment
- **Harness**: `core/c-backend/test_c_backend.py` — emits C via `emit_c.rad`, compiles with GCC, diffs against Rust VM reference output
- **Compiler**: GCC (MinGW-w64 on Windows), 32 MB stack size (`-Wl,--stack,33554432`)

### Results (14/15 pass)

| Test | Status | Notes |
|------|--------|-------|
| `stress_subset` | PASS | Subset of compiler features (types, closures, ECS) |
| `test_ecs` | PASS | ECS operations (spawn, get, set, despawn, queries) |
| `test_closures` | PASS | Closure capture, higher-order functions |
| `test_match_literals` | PASS | Primitive matching (string, int, float, bool) |
| `test_lexer_standalone` | PASS | Self-hosted lexer compiling and running |
| `test_multiline_string` | PASS | Multi-line and triple-quoted string handling |
| `test_parser_standalone` | PASS | Self-hosted parser compiling and running |
| `test_emit_c_standalone` | PASS | Self-hosted C emitter compiling and running |
| `test_platinum` | PASS | Compiling `stress_subset.rad` to C and executing |
| `test_value_types` | PASS | Value semantics, deep copy, list/map operations |
| `test_separate` | PASS | Separate-compilation emission, per-module objects, runtime object, and link step |
| `neg_type_mismatch` | PASS | Negative test: type mismatch error detection |
| `neg_wrong_arity` | PASS | Negative test: arity mismatch error detection |
| `neg_immutable_assign` | PASS | Negative test: immutable assignment error detection |
| `test_diamond` | **KNOWN LIMITATION** | Self-compilation (compiler compiling itself); times out due to O(n) `rad_value_deep_copy` overhead at scale |

### Notes on `test_diamond`

The `test_diamond` benchmark attempts to compile `emit_c.rad` (4000+ lines) through the C backend, producing 40,000+ lines of generated C with ~7,960 `rad_value_deep_copy` calls. The generated binary runs correctly but extremely slowly due to the quadratic deep-copy overhead inherent in the current `RadValue` boxing model. This is a performance limitation of the C runtime's value representation, not a correctness bug. Resolving it would require a fundamental change to the runtime memory model (e.g., reference counting or move semantics), which is out of scope for the current milestone.
