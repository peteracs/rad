# Language reference and ultimate dogfood certification

Status: **local acceptance complete; exact-commit publication pending**
Owner artifact: `projects/dogfood/sovereign-grid/`
Verified: 2026-08-22

This is the completed receipt for the original “document and execute every
feature and syntax” TODO. It covers the one canonical current RAD grammar.
Historical spellings and removed feature switches are rejected; internal AST
recovery nodes are documented as implementation details rather than fabricated
as source syntax. Experimental causal and relation forms remain explicitly
feature-gated and are tested under those gates.

The documentation structure follows the [Rust Reference](https://doc.rust-lang.org/reference/),
[rustdoc](https://doc.rust-lang.org/rustdoc/what-is-rustdoc.html), the
[Python Language Reference](https://docs.python.org/3/reference/), and
[mdBook file inclusion](https://rust-lang.github.io/mdBook/format/mdbook.html#including-files):
normative grammar, generated API reference, host references, tutorials, and
production evidence have separate owners.

## Acceptance summary

- [x] The parser-owned inventory contains 203 executable syntax forms and six
  explicitly internal forms.
- [x] `Builtin::ALL` contains 256 public builtins and every one has executable
  API and Sovereign Grid coverage.
- [x] Seventeen source-owned normative examples execute under the documentation
  gate; copied project commands execute through the acceptance runners.
- [x] Sovereign Grid passes its workflow, exact negative diagnostics, model,
  causal, relation, FFI, replay, inspection, and benchmark matrix.
- [x] The equivalent indexed hot path measures over 200× median wall-time and
  46,667.87× semantic-instruction speedup over the correct baseline.
- [x] The cumulative 11-project portfolio passes 189/189 local acceptance
  operations; every project score is at least 18/20.
- [x] RiskBridge completes each one-million-adjudication mode below its 512 MiB
  private-memory ratchet: record 328,876,032 bytes, replay 243,073,024 bytes,
  and reference 186,974,208 bytes in the final dirty-tree diagnosis run.
- [ ] Publish mdBook and rustdoc from the final clean commit, verify CI and
  Pages, then replace this pending line with the immutable commit and URLs.

## Phase 1 — authoritative inventory and gates

- [x] `tooling/language_surface.py` derives token, declaration, statement,
  expression, pattern, type, callable-contract, and builtin catalogs from live
  owners rather than a hand-maintained denominator.
- [x] `docs/language-surface.json` records a format version and source digest;
  `docs/host-api-surface.json` does the same for CLI and embedding surfaces.
- [x] `projects/dogfood/sovereign-grid/coverage.toml` maps every public syntax
  and builtin row to executable evidence.
- [x] `tooling/check_language_surface.py` rejects missing, duplicate, stale,
  orphaned, unexecutable, or wrong-diagnostic rows.
- [x] CI regenerates and compares the catalogs, so parser deletion or addition
  must update documentation and dogfood in the same change.
- [x] The canonical-only gate rejects removed parser modes and historical
  spellings instead of preserving compatibility branches.
- [x] Every normative rule links to conformance and Sovereign Grid sources.

## Phase 2 — complete language reference

### Lexical structure

- [x] Encoding, source locations, whitespace, line directives, identifiers,
  hard/soft/reserved keywords, and keyword field names are normative.
- [x] Integer, fixed-width integer, float, bool, nil, ordinary/escaped string,
  interpolated string, f-string, and triple f-string forms are covered.
- [x] Comments, delimiters, punctuation, operators, precedence, associativity,
  pipe, logical, comparison, bitwise, shifts, arithmetic, unary, postfix `?`,
  field/call/index, and spread are covered with stable `lex.*` rule IDs.

### Types and patterns

- [x] Primitive/runtime, fixed-width, entity, any, task, fork, host-handle,
  bitset, buffer, byte-buffer, list, map, tuple, union, function,
  `Option<T>`, and `Result<T,E>` types are covered.
- [x] Generic aliases/functions substitute through nested tuples, lists, maps,
  unions, and function types; strict `Pair<int>` substitution has a regression.
- [x] Opaque nominal types, enums, bitflags, `repr(C)`, packed layout, size,
  alignment, offsets, endian conversion, overflow, and narrowing are covered.
- [x] Binding, wildcard, primitive, tuple/list nested, variant named/positional,
  rest, shorthand, guarded, let/closure/loop destructuring, scope, and
  shadowing patterns are covered.

### Declarations

- [x] Components cover defaults, required/indexed/ordered fields, whole and
  field ownership, co-owners, transfer, and visibility.
- [x] Persistent/transient resources, structs, native-layout structs, opaque
  types, enums, bitflags, entities, and entity literals are covered.
- [x] Sum types, parameterized aliases, state machines, and transitions are
  covered.
- [x] Ordinary/generic/pure/readonly/effect-annotated/async functions and
  transactions with `requires`, transitive `changes_only`, `ensures`, and
  `post_commit` are covered.
- [x] Systems cover bound/type-only/mutable/accumulator parameters, authority
  reads/writes/emits/IO/async grants, and ordering.
- [x] Events and sync/next/phase/delayed handlers, phases and serial phases,
  intents, laws, resolvers, candidate constraints, watched components,
  materialized views, ordered keys, and migrations are covered.
- [x] Bare/aliased/normalized modules, visibility, immutable `pub let`, isolated
  and explicitly shared tests, and stateful models are covered.

### Statements and expressions

- [x] Every source-reachable binding and assignment form is covered, including
  immutable/mutable/unique/recursive/annotated/destructured/let-else bindings,
  nested writeback, and bucket fill.
- [x] `if`, `else if`, `else`, `while`, filtered `for`, `break`, `continue`,
  return, expression statements, guarded match, schedules, and updates are
  covered.
- [x] Ordinary/next/phase/delayed emission, sync signals, transaction statements,
  `settle`, `propose`, resolver `next`, and constraint `require ... else` are
  covered.
- [x] Every literal, access, construction, entity literal, spread, operator,
  pipeline form, closure form, state/variant, `is`, match/if/query expression,
  async call, `await`, postfix `?`, and spread argument is covered.

### Callable contracts and execution

- [x] All 13 callable contracts are covered: `@frame`, `@tick`, `@render`,
  `@no_full_scan`, `@allow_full_scan`, the three allocation categories,
  `@budget`, `@no_nested_flush`, `@non_reentrant`, `@exactly_once`, and
  `@must_complete_before`.
- [x] Parallel batches, serial phases, order edges, accumulator merge,
  worker-count determinism, every event-delivery mode, and reentrancy are
  executable.
- [x] Fork/simulate/commit/merge, snapshot/migration, streaming record/replay,
  schema/world digests, provenance retention and truncation, transactions,
  experimental settlements, model generation/shrink/replay, and relations are
  executable.

## Phase 3 — professional API and host references

- [x] The generated [builtin API](../reference/api.md) has one canonical entry
  for each of the 256 names in `Builtin::ALL`.
- [x] Entries record signature, constraints, return type, failures, effects,
  region/sandbox availability, determinism, platform availability, complexity,
  and allocation category.
- [x] The API is grouped by values, collections, text, buffers, ECS,
  queries/indexes/views, events/phases, provenance, transactions, speculation,
  persistence/wire, model testing, host IO/networking, relations, and FFI.
- [x] Pure/state examples and deterministic local file, HTTP, TCP, UDP, async,
  and extension harnesses execute in Sovereign Grid acceptance.
- [x] The [CLI reference](../reference/cli.md) is generated against 35 command
  forms and 37 options and documents exit behavior, streams, JSON, effects, and
  examples.
- [x] Warning-free rustdoc covers the Rust embedding API; separate embedding,
  WASM, and FFI references document 43 public runtime methods, ownership,
  errors, ABI descriptors, containment, replay, and generation pinning.
- [ ] Publish mdBook and rustdoc from the same final commit and verify both URLs.

## Phase 4 — Sovereign Grid ultimate dogfood

- [x] The production-shaped multi-module workflow uses only canonical strict
  syntax and executes every manifest row.
- [x] Accepted and rejected cases cover every callable contract plus ownership,
  transactions, views, ordered indexes, phases, provenance, state machines,
  speculation, snapshots, migration, and replay interactions.
- [x] Experimental settlements and relations execute under explicit flags
  without weakening the stable main boundary.
- [x] Native/reference/recorded FFI paths are byte-layout verified and
  generation/effect bound.
- [x] Isolated tests, intentional shared-world tests, and a stateful temporal
  model deterministically shrink and replay failures.
- [x] Thirty-three intended negative fixtures are matched against their exact
  diagnostic contract; unrelated parser failures cannot count as acceptance.
- [x] Record/replay outcomes are checked under 1, 2, 4, and logical-CPU worker
  counts for world, event, view/index, and provenance equality.
- [x] `projects/dogfood/sovereign-grid/accept.ps1` emits source-, binary-,
  toolchain-, profile-, option-, and artifact-bound JSON evidence.

## Phase 5 — measured 100× hot-path gate

- [x] A deliberately correct full-scan baseline and equivalent indexed/view
  implementation assert equal business result and digest first.
- [x] One release binary and process runs warmup plus 30 samples per mode.
- [x] Deterministic work improves 46,667.87× and measured median wall time
  improves by more than 200×; the report also records p95, CPU, wall time, RSS, all four
  allocation categories, cardinality, digest, and query plans.
- [x] `@no_full_scan` and precise allocation contracts are enforced on the hot
  root with transitive cost-path evidence.
- [x] Runtime optimization followed profiler/allocator evidence. The hot path
  performs 15 VM instructions, two runtime allocations totaling 32 bytes, no
  guest/host/managed allocations, and retains no DHAT blocks after 1,000 runs.

## Phase 6 — release gates

- [x] Sovereign Grid main, workflow, negatives, model, relations, FFI, scale,
  replay, and operational inspection pass.
- [x] All 11 dogfoods pass 189/189 cumulative local operations.
- [x] Formatting, workspace/all-target checking, 1,449 VM tests plus integration
  and doc tests, 31 CLI tests, five LSP tests, strict Clippy, architecture/SRP,
  280 conformance, 10 feature, 29 causal snapshots, and warning-free WASM pass.
- [x] All 17 normative RAD examples and the complete generated surface execute.
- [x] mdBook, 685 internal links, generated catalogs, and warning-free rustdoc
  pass locally.
- [x] Ignored acceptance/profiler output is kept outside the source tree and the
  release hygiene scan rejects accidental captures, binaries, and scratch.
- [ ] Commit the clean source generation, rerun the release-eligible portfolio,
  push, verify CI, and verify published mdBook/rustdoc.

## Closed defects found by the dogfood

- [x] Synthetic `$transition` is permitted inside a transaction's staged patch
  while ordinary pre-commit emission and authoritative post-commit mutation
  remain rejected. Checker and compiler regressions cover it.
- [x] Generic aliases substitute parameters recursively; `Pair<int>` and nested
  union substitution have strict checker regressions.

## Evidence owners

- Syntax/API manifests: `docs/language-surface.json`,
  `docs/host-api-surface.json`
- Coverage gate: `tooling/check_language_surface.py`
- Documentation example gate: `tooling/check_rad_doc_examples.py`
- Ultimate acceptance: `tooling/accept_sovereign_grid.py`
- Cumulative portfolio: `devtools/accept-portfolio.ps1`
- Canonical reference: `docs/src/reference/spec.md`,
  `docs/src/reference/api.md`, `docs/src/reference/cli.md`,
  `docs/src/reference/embedding.md`, `docs/src/reference/ffi.md`
