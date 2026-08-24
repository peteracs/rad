# CLI reference

This chapter defines the `rad` process contract. The generated
[host API surface](./generated/host-api-surface.md) is the command/option
denominator; repository checks fail when the production parser gains an entry
that is absent from this reference.

## Process contract

`rad` writes requested program output and JSON results to stdout. Diagnostics,
progress, and human-readable failures go to stderr. Success exits `0`; invalid
arguments, source/check/compiler/runtime errors, failed tests, replay
divergence, and verifier failure exit `1`.

Paths resolve from the current directory unless a project manifest or command
says otherwise. A `--` separator ends RAD option parsing; every later value is
visible through `sys_args()`.

## Run a program

```text
rad <file.rad> [options] [-- <program arguments>]
rad run [options] [-- <program arguments>]
```

`rad run` discovers `rad.toml`; the direct form uses the named entry file.

| Option | Contract |
|---|---|
| `--no-check` | Select the unchecked compiler path. It cannot be combined with `--record`, because replay requires checker-derived lowering metadata. Production acceptance never uses it. |
| `--strict-types` | Require explicit, resolved public and callable boundaries. |
| `--deny-warnings` | Make checker/linter warnings fatal. |
| `--feature <name>` | Enable one named language feature. Repeatable. |
| `--experimental-laws` | Enable causal laws/settlements. |
| `--experimental-relations` | Enable relation runtime operations. |
| `--relation-schema <file>` | Load a relation schema/derivation module. |
| `--relation-module <id>` | Bind its runtime module identity. |
| `--write-lock` | Write the authenticated module graph to `forge.lock`. |
| `--profile-copies` | Report copy-on-write deep copies. |
| `--serial-schedule` | Execute schedule batches serially for differential checking. |
| `--record <trace.radr>` | Atomically record a digest-sealed streaming zstd trace; memory is independent of I/O-record count. |
| `--version` | Print the exact CLI version when no input is supplied. |
| `--help`, `-h` | Print the production usage surface. |

## Source and project tools

| Command | Result and side effects |
|---|---|
| `rad new <name> --template <name>` | Create a project. `--list-templates` lists names without writes. |
| `rad fmt [--check] [files...]` | Format in place, or report drift without writes. |
| `rad lint [--preset strict] [--boundary N] [files...]` | Run semantic/style/architecture lint. |
| `rad snapshot [--update] [--create] [--experimental-laws] [dir]` | Compare `.rad` files with sibling `.snap` outputs. Update rewrites existing baselines; create adds absent ones. |
| `rad test [path]` | Execute isolated/shared tests and models with per-declaration and aggregate results. |
| `rad types <input.rad> <output.d.ts> [--feature <name>]` | Emit TypeScript declarations for a checked RAD program. |
| `rad build --target browser-package <input.rad> <output.radpkg.json>` | Emit a digest-bound canonical multi-module browser package. |
| `rad build --target compiler-wasm <input.rad> <output.wasm>` | Copy an explicitly configured `RAD_COMPILER_WASM` after checking the program; no placeholder is emitted. |
| `rad play [--port N]` | Serve the local playground. |
| `rad lsp [--experimental-relations]` | Run JSON-RPC language service over stdio. |

## Semantic inspection

These commands accept `--json`. JSON mode writes one complete JSON value to
stdout and diagnostics to stderr. Outside a project, pass `--file <entry.rad>`
or `--file=<entry.rad>`.

| Command | Question answered |
|---|---|
| `rad surface <file.rad>` | Which compiler-owned tokens/AST nodes/contracts/builtins occur? |
| `rad effects <callable>` | What synchronous/deferred effects are inferred? |
| `rad writers <type>` | Which roots may write this authority? |
| `rad readers <type>` | Which roots read it? |
| `rad path <from> -> <to>` | What transitive call/effect path connects the roots? |
| `rad query-plan <view-or-callable>` | Which scan/index/view plan and complexity are selected? |
| `rad cost-path <callable>` | Which transitive operation contributes cost? |
| `rad why <entity> <component>` | What causal chain produced the component? |
| `rad why-field <entity> <component> <field>` | What produced the field? |
| `rad why-removed <entity> <component>` | What tombstone/cause removed it? |
| `rad why-not-in-view <view> <entity>` | Which dependency/predicate excludes the entity? |

## Operations, replay, and performance

| Command | Contract |
|---|---|
| `rad bench <file.rad> [--json] [-- args...]` | Run setup once and measure `bench_run()` when present. JSON includes entry, elapsed/CPU time, semantic instructions, allocation categories, and digest. |
| `rad replay <trace.radr>` | Verify/replay. Faithful replay streams records; `--to-frame N` stops. `--with file.rad` and `--serve` intentionally materialize the trace for oracle/time-travel random access. `--force` explicitly accepts permitted identity drift. |
| `rad model-check <file.rad>` | Execute a model. `--model`, `--runs`, `--max-commands`, `--seed`, `--artifact-dir`, and `--json` control the campaign. Counts must be positive. |
| `rad shrink <failure.radr>` | Deterministically minimize a model-failure artifact. |
| `rad ffi verify <plugin>` | Verify image, descriptor, layouts, exports, effects, containment, and determinism. `--contract file.json` and `--json` control expected/machine output. |
| `rad relations check <file.rad> --experimental-relations [--module id]` | Parse, type, bound, and seal relation source without execution. |
| `rad sandbox serve [host.rad] [--caps file.json]` | Start the framed host with explicit capabilities/fuel/memory/effects. |

## JSON stability

Persisted/exchanged machine output carries a format/version discriminator.
Consumers reject unknown versions/required fields and honor the process exit
code. Replay, model, and benchmark reports include source or world digests so
evidence cannot be mixed across source generations.
