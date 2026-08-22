# Developer Tools

Rad ships with a complete developer experience out of the box: a language server, formatter, linter with presets, and snapshot testing. No third-party plugins required.

## Language Server (LSP)

The Rad LSP is built directly into the Rust CLI (`rad lsp`) and provides real-time editor integration. It communicates over the standard Language Server Protocol, so any editor that supports LSP can use it.

### Starting the LSP

```bash
rad lsp
```

### Features

**Diagnostics & Error Recovery** — parse errors and type errors from the Rust VM checker appear inline as you type. Rad's parser uses **error recovery**, meaning it won't stop at the first syntax error. It synchronizes and continues, allowing the type checker to run on the salvaged AST. This means you see *all* syntax and type errors across your entire project simultaneously.

**Hover info** — hover over any symbol to see its type signature:

- **Components** show all fields with types: `component Health { hp: int, max: int }`
- **Resources** show singleton fields: `resource GameTime { tick: int }`
- **Functions** show parameters, return type, and purity: `pure fn double(x: int) -> int`
- **Systems** show queried components and scheduling: `system Move(p: mut Position) after Physics`
- **Events** show payload fields: `event Hit { target_id: str, amount: int }`
- **State machines** show all states: `state Door { Locked, Closed, Open }`
- **Sum types** show variants with fields
- **Builtins** show signature and documentation: `push(list, value) -> list`
- **Keywords** show usage examples

**Go-to-definition** — jump to where any component, resource, function, system, event, state, type, entity, or variable is defined.

**Autocompletion** — context-aware suggestions:

- After `system::` — declared systems (for `simulate` or `schedule`)
- After `::` — state machine states or sum type variants
- After `.` — component fields
- At line start — declaration keywords (`component`, `resource`, `system`, `fn`, etc.)
- Anywhere — user-defined symbols, builtins (with signatures), keywords

**Document formatting** — the LSP exposes `textDocument/formatting`, so editor format-on-save works with the Rad formatter.

### Optional: Phase 3 WASM diagnostics and `rad build`

When `rad` is built with the `native-wasm-phase3` Cargo feature (default for the in-tree `rad-vm` crate), you can route **publishDiagnostics** through the self-hosted compiler loaded as WebAssembly instead of the Rust checker, and emit a reactor **`compiler.wasm`** from the CLI.

**Emit a WASM module (after type-checking the entry program):**

```bash
rad build --target wasm path/to/entry.rad path/to/out.wasm
```

- Positional arguments are **`entry.rad`** then **`out.wasm`**. Put `--target wasm` before those paths.
- Output bytes: if **`RAD_COMPILER_WASM`** is set, `rad` copies that file to `out.wasm`; otherwise it writes the in-tree **stub** reactor (until you build a real guest from `emit_wasm.rad`).

**LSP environment variables:**

| Variable | Role |
|----------|------|
| `RAD_WASM_PHASE3=1` | Use the WASM reactor for diagnostics. |
| `RAD_COMPILER_WASM` | Path to a `compiler.wasm` guest; also enables the WASM diagnostic path. |
| `RAD_VFS_ROOT` | Optional workspace root for `vfs_read` when resolving imports not present in open-editor overlays. |

The host VFS fills the overlay from **open documents**, then falls back to the filesystem (`RAD_VFS_ROOT` or the active file’s directory). The stub reactor returns no diagnostics until the guest implements checking. Hover, completion, go-to-definition, and formatting still use the Rust implementation unless extended separately.

Full ABI, VFS order, and recovery notes: [Phase 3 WASM](../reference/wasm-phase3.md).

## Copy Profiling (`--profile-copies`)

The `--profile-copies` CLI flag enables runtime diagnostics for hidden `Arc` deep clones in list operations.

### Usage

```bash
rad main.rad --profile-copies
```

### What it reports

When a list mutation (`push`, element assignment, or `extend`) triggers `Arc::make_mut` on a shared backing buffer (reference count > 1), the VM emits a diagnostic to stderr:

```
[copy-profile] line 42: deep clone of 10000-element list (Arc refcount was 2)
```

### When to use it

- **Debugging performance regressions** — find hot loops where an unintended alias causes full vector clones
- **Validating `unique` bindings** — confirm that `let unique` eliminates all clones as expected
- **Profiling pipeline chains** — identify intermediate copies in fused pipelines

The flag has negligible overhead when no copies occur. Combine with `let unique` (see [Value Semantics](./value-semantics.md)) to guarantee zero-copy mutations.

### Embedding the VM from Rust

If you drive [`VM`](../../../core/vm/src/vm/mod.rs) from a native renderer or
game host, exchange owned
[`FrozenValue`](../../../core/vm/src/host_value.rs) trees through
`import_value`, `call_global`, `export_global`, `component_value`,
`resource_value`, and `enqueue_frozen_event`.
`ValueHandle<'vm>` provides a read-only borrowed view that cannot outlive its
VM. The NaN-boxed runtime `Value`, its raw object pointers, and `GcHeap` are
crate-private; native hosts never allocate into or dereference that heap.
Deterministic or isolated hosts can construct the VM with
`VM::new_with_seed(seed)` so initialization never consults host time.

ECS world columns still use **persistent** storage (`Arc<Object>` managed by
`ValueColumn` retain/release) for component field values written by RAD code;
strings are `Arc<str>` for O(1) internal air-gap copies. That internal path is
separate from the host value boundary and closure backup collector. See
[Architecture → Semantics notes](../reference/architecture.md).

### Frozen C backend

`experiments/c-backend/` is frozen experimental code. It is not part of normal developer
tooling, not a release health gate, and not a source of truth for language
behavior. See [C Backend Freeze](../project/c-backend-freeze.md).

Any detailed C-backend status below is historical and not current maintenance
guidance.

Historical note: the self-hosted compiler (`experiments/c-backend/src/emit_c.rad`)
was typechecked by the default Rust VM checker. At its freeze point, the C
backend passed 14/15 benchmark stress tests; `test_diamond` (the compiler
compiling itself) remained a performance limitation. `lexer.rad` was strict
clean, while `parser.rad`, `checker.rad`, and `emit_c.rad` had not completed
that migration. No strictness rollout continues in the frozen experiment.

The old entry points are opt-in only and require `RAD_RUN_FROZEN_C_BACKEND=1`:

| Command | Use |
|---|---|
| `RAD_RUN_FROZEN_C_BACKEND=1 py experiments\c-backend\test_conformance_c.py` | Historical C conformance harness. |
| `RAD_RUN_FROZEN_C_BACKEND=1 py experiments\c-backend\test_c_backend.py` | Historical stress harness. |

Generated C, temporary emitter scripts, and binaries belong under
`experiments/c-backend/target/`. Source-level notes about the C backend belong in
[Architecture](../reference/architecture.md), [Phase 3 WASM](../reference/wasm-phase3.md),
or this page, not in ad hoc files under `experiments/c-backend/`.

## Formatter

The Rad formatter (`rad fmt`) produces consistently styled code. It is idempotent — running it twice always produces the same output.

### What it does

- **Indentation**: 2-space indent, tracking brace depth through all block constructs
- **Operator spacing**: normalizes `=`, `==`, `!=`, `|>`, `=>`, `->`, `<`, `>`, `<=`, `>=`
- **Blank lines**: ensures exactly one blank line between top-level declarations; collapses multiple blanks
- **Trailing newline**: every file ends with exactly one newline
- **Comment preservation**: comments are indented but never modified

### Usage

```bash
rad fmt                    # format all .rad files recursively
rad fmt src/               # format a specific directory
rad fmt main.rad           # format a single file
rad fmt --check            # check mode — exit 1 if any file needs formatting
```

The `--check` flag is useful in CI pipelines:

```yaml
- run: rad fmt --check
```

### Shared engine

The formatter is built natively in Rust and uses the compiler's lexer to safely format code without mangling comments or strings.

## Authority Graph

The checker builds one fine-grained effect graph for functions, systems,
handlers, closures, callbacks, and imported helpers. Query it directly:

```bash
rad effects RemoveEntity --json --file app.rad
rad writers LiveMembership --file app.rad
rad readers WireIdentity --file app.rad
rad path mission_frame "->" full_scan --file app.rad
```

Inside a project with `rad.toml`, `--file` is optional and the configured entry
file is used. `effects` prints direct, synchronous, and complete transitive
effects; `writers` and `readers` use cached reverse indexes; `path` returns the
shortest callable chain or reports that none exists. All commands reject
ambiguous short names and accept `--json` for machine-readable output.

The compiler uses this graph both to enforce system authority and to build
parallel conflict batches; lint does not maintain a separate model. System
roots are specialized for statically resolved callback arguments before
enforcement and batching. A generic helper's standalone report can
conservatively union callback targets from multiple call sites.

System permissions cover the complete synchronous effect record:

```rad
system RemoveEntity(
    live: mut LiveMembership,
    writes WireIdentity,
    emits EntityRetired,
    io false,
    async false,
) { ... }
```

`io` and `async` default to `false`; write `true` only to grant the effect.
`emits *`, `reads *`, and `writes *` are explicit wildcard grants. Runtime
entity-name indexes use quoted keys: `"$entity_names"` and
`"$entity_identity"`.

## Lint Presets

`rad lint` combines custom source-level rules with the Rust VM's type checker. Three presets target different project stages:

### Enterprise

Maximum safety for production codebases.

```bash
rad lint --preset enterprise
```

- Requires type annotations on all bindings, parameters, and return types (`--strict-types`, though `pub` exports are always strictly typed and checked for private type leaks)
- Treats all warnings as errors (`--deny-warnings`)
- Enforces PascalCase for type, component, resource, state, and event names
- Includes compile-time transitive system-authority enforcement
- Limits functions to 50 lines
- Limits files to 500 lines

### Strict

Strict type checking with warnings as errors. Good for mature projects that haven't fully annotated everything yet.

```bash
rad lint --preset strict
```

- Requires type annotations (`--strict-types`, though `pub` exports are always strictly typed and checked for private type leaks)
- Treats warnings as errors (`--deny-warnings`)
- Includes compile-time transitive system-authority enforcement
- Limits functions to 80 lines
- Limits files to 1000 lines

### Teaching

Beginner-friendly mode that suggests improvements without blocking.

```bash
rad lint --preset teaching
```

- **Suggests** (does not require) type annotations on `let` bindings
- **Suggests** marking pipeline-safe functions as `pure fn`
- Warns when a pipeline has more than 5 stages
- Warns on trailing whitespace
- Limits functions to 100 lines

### Lint rules

| Code | What it catches |
|---|---|
| `RAD-L001` | File exceeds the preset's line limit |
| `RAD-L002` | Function exceeds the preset's line limit |
| `RAD-L003` | Type name doesn't follow PascalCase (enterprise only) |
| `RAD-L004` | `let` binding without type annotation (teaching: suggest) |
| `RAD-L005` | Function could be marked `pure fn` (teaching: suggest) |
| `RAD-L006` | Pipeline has more than 5 stages (teaching: warn) |
| `RAD-L007` | Trailing whitespace |
| `RAD-L008` | Entity declarations found without any systems (enterprise/strict) |
| `RAD-L009` | Systems are declared but never run (enterprise/strict) |
System authority is not a preset-specific lint rule. Every checker invocation
enforces it through ordinary/imported calls, closures, callbacks, resources,
transitions, and synchronous handler chains. `rad lint` runs that checker in
addition to the custom rules above, so all presets report authority errors.

## Snapshot Testing

Snapshot tests capture the output of `.rad` scripts and verify that future runs produce the same result. They are the simplest way to add regression tests to a Rad project.

*Note: Snapshot testing is built directly into the native `rad` CLI.*

### Creating snapshots

```bash
# Create snapshots for all .rad files in a directory
rad snapshot --create tests/snapshots/
```

This runs each `.rad` file through the Rust VM and writes a `.snap` file alongside it:

```
tests/snapshots/hello.rad       # your script
tests/snapshots/hello.snap      # captured output
```

### Verifying snapshots

```bash
rad snapshot tests/snapshots/
```

Each script is re-run and compared against its `.snap` file. If the output differs, a unified diff is printed:

```
  PASS  tests/snapshots/hello.rad
  FAIL  tests/snapshots/math.rad
        stdout diff:
        --- expected (snapshot)
        +++ actual
        @@ -1,2 +1,2 @@
         2 + 2 = 4
        -3 * 3 = 9
        +3 * 3 = 10
```

### Updating snapshots

After intentional changes to script behavior:

```bash
rad snapshot --update tests/snapshots/
```

This overwrites all `.snap` files with the current output.

### Snapshot format

`.snap` files are human-readable text:

```
---
source: tests/snapshots/hello.rad
exit_code: 0
---
Hello, Rad!
```

If the script produces stderr output (warnings or errors), it appears after a `---stderr---` separator.

### CI integration

```yaml
- run: rad snapshot tests/snapshots/
```

The command exits with code 1 if any snapshot fails, making it suitable for CI gates.

### Scaffold

`rad new` creates a `tests/snapshots/` directory with a starter script. Run `rad snapshot --create tests/snapshots/` to generate the initial baselines.
