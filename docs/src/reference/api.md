# RAD API reference

This is the entry point for RAD's public programming interfaces. It is kept
separate from the language grammar and from tutorials:

- the [language specification](./spec.md) defines source syntax and core
  semantics;
- the [builtin category index](./generated/builtin-index.md) and
  [complete builtin catalog](./generated/builtin-api.md) define the 262
  functions installed by the VM;
- the [CLI guide](../getting-started/forge.md) defines command-line behavior;
- the [embedding guide](../guide/developer-tools.md#embedding-the-vm-from-rust)
  defines the Rust host boundary;
- [native extensions](../guide/cost-phases-provenance-models-and-ffi.md#11-effect-declared-native-extensions)
  define the versioned FFI boundary;
- [Phase 3 WASM](./wasm-phase3.md) defines the browser/JavaScript host
  boundary.

The separation is intentional. A language rule answers “what does this RAD
program mean?” An API entry answers “what callable interface is available?” A
tutorial answers “how do I build something with it?” Mature references make
the same distinction: the Rust Reference explicitly excludes standard-library
and tool documentation, while Python separates syntax/core semantics from its
standard library and C API.

## API-entry contract

Every public builtin, CLI operation, embedding method, and FFI export must have
one canonical entry containing:

1. exact spelling and signature;
2. accepted and returned types;
3. purity, state, IO, async, allocation, and replay effects;
4. platform availability (native, WASM, sandbox, transaction, settlement);
5. deterministic ordering and error behavior;
6. complexity and allocation characteristics where meaningful;
7. one executable positive example;
8. one intended failure example for each important rejected contract;
9. links to implementation, conformance coverage, and production dogfood.

An API name appearing in prose is not evidence of documentation. The API gate
must compare the source-owned catalogs against canonical API entries and fail
on missing, duplicate, or orphaned entries.

## Completeness contract

The source-owned catalog contains 262 builtins. `check_language_surface.py`
requires exactly one generated API entry per runtime name, validates every
field in the contract above, proves that the linked Sovereign Grid source calls
that builtin through the compiler AST, and executes the associated command.
Adding or removing a builtin therefore makes documentation and dogfood drift a
failing repository gate.

## Publication

The [mdBook](https://peteracs.github.io/rad/docs/) is the canonical
human-facing language/API output. API inventories are generated into Markdown
before `mdbook build`, so local HTML and GitHub Pages consume the same checked
source generation. Rust host APIs additionally use `cargo doc` and are
published from the same Pages workflow at the [Rust API
documentation](https://peteracs.github.io/rad/rustdoc/rad_vm/). Generated
rustdoc remains under `target/` locally and is never committed.
