# Language reference and ultimate dogfood certification

Status: **accepted portfolio, language surface, and local release gates green; clean-commit publication pending**
Owner artifact: `projects/dogfood/sovereign-grid/`
Verified: 2026-08-23

This is the cumulative receipt for the original “document and execute every
feature and syntax” TODO and the subsequent hard subsecond rule. It covers the
one canonical current RAD grammar. The language-surface work and local
current-tree gates are complete; release certification still needs the clean
commit, CI, and publication receipt. Phase 7 proves every invocation owned by the accepted
portfolio and language-surface matrices below one second without reducing those
workloads. It does not classify standalone long-form research receipts as
accepted subsecond runs.
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

## 2026-08-23 wrap-up checkpoint

Finished on the current working tree:

- [x] The cumulative production portfolio passes 11/11 projects and 189/189
  operations. All 187 timed child operations are below one second; the slowest
  is `riskbridge-workflow-tests` at 705.7 ms.
- [x] Sovereign Grid passes 127/127 acceptance outcomes, covering all 203 public
  source forms and 260 runtime builtins.
- [x] The exact six-lane affine frontier keeps its depth-1,024, support-16,
  beam-256 workload and completes run, record, independent verification, and
  replay below one second.
- [x] The optimized native mathematics suite passes 56 executed tests with one
  explicitly ignored production-scale receipt. Its differential lane test
  found and closed a real partition bug: production used support-weight six
  while the oracle used seven. Both now consume one stable lane contract while
  retaining independent search implementations.
- [x] Native extension calls have an explicit mandatory `timeout_ms` authority
  between 1 and 900 ms; the runtime has no hidden one-argument fallback.
- [x] `tooling/language_surface.py` and
  `tooling/generate_language_coverage.py` regenerated the source inventory and
  203/260 evidence manifest after the final syntax extraction.
- [x] The failed SMT experiment was rejected after exceeding both the native
  solver's time and memory. Its temporary source and all orphaned Python
  processes were removed; no solver dependency or generated proof entered the
  repository.

Still required before commit/release:

- [x] Correct the canonical builtin metadata for
  `load_extension(str, int) -> any`, regenerate the builtin API, and rerun the
  language-surface inventory. The runtime, checker, and generated API now expose
  only the required two-argument form.
- [x] Resolve the exact Support Pressure performance contract. The accepted
  matrices are subsecond, but the standalone support-ten exhaustive research
  receipt historically takes about 117.3 seconds. Do not lower support/depth,
  embed its result, or relabel it as subsecond. It is now explicitly documented
  outside the subsecond acceptance claim and remains a named optimization backlog.
- [x] Use `rg.exe` across every Markdown/text documentation owner to replace
  stale frontier timings, old one-argument extension examples, and any claim
  that confuses accepted operations with standalone research programs.
- [x] Rerun formatting, workspace/all-target check, full VM/CLI/LSP,
  conformance/snapshots, strict Clippy, architecture/folder/line gates, corpus,
  WASM, rustdoc, mdBook, and link checks on this exact tree. The source/API
  corpus gate reports 203 executable syntax rows and all 260 builtins.
- [x] Audit the large accumulated diff and untracked set, remove or archive only
  proven scratch/generated debris, run `git diff --check`, and commit only after
  the tree is organized and all current-tree gates are green. Every remaining
  untracked path is a source, test, fixture, or verifier owner; ignored build and
  acceptance artifacts remain outside the source tree.
- [ ] From that clean commit, rerun release-eligible portfolio/Sovereign
  receipts, push, verify CI and GitHub Pages, and record immutable URLs/hashes.

## Acceptance summary

- [x] The parser-owned inventory contains 203 executable syntax forms and six
  explicitly internal forms.
- [x] `Builtin::ALL` contains 260 public builtins and every one has executable
  API and Sovereign Grid coverage.
- [x] Seventeen source-owned normative examples execute under the documentation
  gate; copied project commands execute through the acceptance runners.
- [x] Sovereign Grid passes its workflow, exact negative diagnostics, model,
  causal, relation, FFI, replay, inspection, and benchmark matrix.
- [x] The equivalent indexed hot path measures 307.16× release-profile median
  wall-time and
  53,847.38× semantic-instruction speedup over the correct baseline.
- [x] The cumulative 11-project portfolio passes 189/189 local acceptance
  operations; every project score is at least 18/20.
- [x] RiskBridge completes each exact one-million-adjudication mode below one
  second and its 512 MiB private-memory ratchet. The 2026-08-23 release-profile
  receipt records 159.350/83.623/126.464 ms and
  97,464,320/13,291,520/4,513,792 private bytes for record/replay/reference.
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
  for each of the 260 names in `Builtin::ALL`.
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
- [x] Deterministic work improves 53,847.38× and measured release-profile
  median wall time improves by 307.16×; the report also records p95, CPU, wall
  time, RSS, all four
  allocation categories, cardinality, digest, and query plans.
- [x] `@no_full_scan` and precise allocation contracts are enforced on the hot
  root with transitive cost-path evidence.
- [x] Runtime optimization followed profiler/allocator evidence. The hot path
  performs 13 VM instructions, two runtime allocations totaling 32 bytes, no
  guest/host/managed allocations, and retains no DHAT blocks after 1,000 runs.

## Phase 6 — release gates

- [x] Sovereign Grid main, workflow, negatives, model, relations, FFI, scale,
  replay, and operational inspection pass.
- [x] All 11 dogfoods pass 189/189 cumulative local operations.
- [x] Formatting, workspace/all-target checking, the complete independently
  compiled syntax and VM suites plus integration and doc tests, CLI and LSP
  suites, strict Clippy, architecture/SRP,
  280 conformance, 10 feature, 29 causal snapshots, and warning-free WASM pass.
- [x] All 17 normative RAD examples and the complete generated surface execute.
- [x] mdBook, 688 internal links, generated catalogs, and warning-free rustdoc
  pass locally.
- [x] Ignored acceptance/profiler output is kept outside the source tree and the
  release hygiene scan rejects accidental captures, binaries, and scratch.
- [ ] Commit the clean source generation, rerun the release-eligible portfolio,
  push, verify CI, and verify published mdBook/rustdoc.

## Phase 7 — hard subsecond certification

The limit is an end-to-end process invariant, not an inner-loop estimate:
module loading, semantic checking, bytecode compilation, execution, digest or
inspection work, output, and teardown must total less than 1,000,000,000 ns.
Workloads, assertions, replay verification, provenance, and failure contracts
must remain intact. Rust compilation and external tool installation are
reported separately as build/toolchain operations; they are not RAD script
runs and cannot be used to hide RAD runtime work.

- [x] Make every 11-project main, workflow suite, negative fixture, benchmark,
  record, replay, effects/path/query-plan, why/why-not, model, relation, and FFI
  invocation finish below the hard limit.
- [x] Keep the one-million RiskBridge native, reference, record, and replay
  workloads exact; no sampling, reduced cardinality, deferred background work,
  or precomputed acceptance result may satisfy the gate.
- [x] Make the timeout/crash containment fixtures fail with their intended
  typed outcomes below one second; a multi-second timeout is itself a failure.
- [x] Extend `devtools/accept-portfolio.ps1`, Sovereign Grid acceptance, and
  machine-readable reports with `elapsedNs`, `limitNs`, and exact source/binary
  identities, and reject any RAD operation at or above the limit.
- [x] Retain profiler evidence for each redesigned hot-path family and prove
  deterministic digests, allocation categories, peak RSS, replay consumption,
  and scheduler-worker equality after optimization.
- [x] Rerun the complete syntax/API/doc-example inventory under the same hard
  process gate so generated coverage cannot point at a slow or skipped sample.
- [x] Update the performance, architecture, FFI, transaction, replay, dogfood,
  and API documentation with guarantees, non-guarantees, measured receipts,
  and the exact meaning of “subsecond.”
- [ ] Complete all repository/release gates from one clean source generation,
  commit, tag, push, verify CI, and verify published mdBook/rustdoc URLs.

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
