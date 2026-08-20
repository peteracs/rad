# Folder Tree

This is the exhaustive visual map of directories tracked by Git. Every entry
states the responsibility owned by that folder; ignored build output and local
scratch work are intentionally absent. The [Repository Map](repo-map.md)
defines dependency rules, and `python tooling/check_architecture.py` keeps
this inventory aligned with the source tree.

```text
rad/
  - `.github/` - GitHub contribution templates and automation configuration.
    - `.github/ISSUE_TEMPLATE/` - Structured bug, feature, onboarding, and language-design issue forms.
    - `.github/workflows/` - CI, soundness, benchmarks, line limits, and deployment workflows.
  - `adapters/` - Host-facing composition that depends on the authoritative VM library.
    - `adapters/cli/` - Rust package for the rad executable and process I/O.
      - `adapters/cli/src/` - CLI composition root.
        - `adapters/cli/src/cli/` - Command arguments, execution, and diagnostic presentation.
    - `adapters/lsp/` - Rust package for the Language Server Protocol boundary.
      - `adapters/lsp/src/` - LSP library composition root.
        - `adapters/lsp/src/lsp/` - Document analysis, server requests, symbols, and diagnostics.
    - `adapters/webgpu/` - Reusable WebGPU presentation host and runnable RAD dogfood.
      - `adapters/webgpu/browser-test/` - Real-browser pixel, resize, restart, and recovery smoke.
      - `adapters/webgpu/demo/` - Clockwork Eye application, world source, and browser entry point.
        - `adapters/webgpu/demo/session/` - Fixed-step RAD session and frame-loop ownership.
        - `adapters/webgpu/demo/testing/` - Browser harness and deterministic frame analysis.
        - `adapters/webgpu/demo/ui/` - Demo-only status presentation.
      - `adapters/webgpu/src/` - Public package composition root.
        - `adapters/webgpu/src/facade/` - High-level application facade over independent subsystems.
        - `adapters/webgpu/src/gpu/` - Device lifecycle and bounded GPU-buffer ownership.
        - `adapters/webgpu/src/presentation/` - Packet contract, lineage, renderer, shader, targets, and readback.
        - `adapters/webgpu/src/runtime/` - Ephemeral WASM presentation-packet source.
      - `adapters/webgpu/test/` - Package unit and contract tests with GPU/runtime doubles.
  - `benches/` - Repository-level workload inputs and benchmark orchestration.
    - `benches/baselines/` - Recorded and executable comparisons with external implementations.
      - `benches/baselines/collab/` - RAD, Automerge, and Yjs collaborative-edit comparisons.
      - `benches/baselines/micro/` - Matched RAD, Lua, and JavaScript microbenchmarks.
  - `core/` - Authoritative RAD language and runtime code only.
    - `core/vm/` - Primary Rust implementation of the RAD language and runtime.
      - `core/vm/benches/` - Criterion benchmarks for VM and world-state operations.
      - `core/vm/scripts/` - VM-specific documentation and benchmark helpers.
      - `core/vm/src/` - Language semantics, compilation, world state, execution, replay, relations, and WASM boundary.
        - `core/vm/src/builtins/` - Builtin signatures grouped by value, world, host, and simulation roles.
        - `core/vm/src/causality/` - Settlement provenance model and explanation rendering.
        - `core/vm/src/checker/` - Static semantics, effects, scopes, diagnostics, and type checking.
          - `core/vm/src/checker/declarations/` - Declaration registration and effect-analysis phases.
          - `core/vm/src/checker/tests/` - Behavior-grouped checker regression suites.
          - `core/vm/src/checker/typeck/` - Expression, statement, query, pattern, and builtin type checking.
        - `core/vm/src/compiler/` - Typed AST to bytecode lowering and optimization.
          - `core/vm/src/compiler/expr/` - Expression lowering, folding, query, and vector compilation.
          - `core/vm/src/compiler/stmt/` - Statement, control-flow, effect, and loop lowering.
          - `core/vm/src/compiler/tests/` - Compiler execution and regression suites.
        - `core/vm/src/constraint_types/` - Candidate-constraint profiles, values, and pure reference contracts.
        - `core/vm/src/host_value/` - Frozen host-value representation and boundary tests.
        - `core/vm/src/internal_tests/` - Crate-private cross-module integration suites.
          - `core/vm/src/internal_tests/causal_laws/` - Settlement, resource-limit, provenance, and wire tests requiring private VM state.
          - `core/vm/src/internal_tests/composition/` - World, causality, allocator, and fork-delta composition tests.
          - `core/vm/src/internal_tests/migration/` - Allocator transport, persistence identity, and migration semantics tests.
        - `core/vm/src/lexer/` - Declaration, expression, and statement tokenization.
        - `core/vm/src/linter/` - Lint engine and lint-specific tests.
        - `core/vm/src/merge/` - Three-way world merge engine and conflict tests.
        - `core/vm/src/module_loader/` - Module resolution, aliases, lockfiles, and loader tests.
        - `core/vm/src/parser/` - Syntax parser, recovery, types, and causal extensions.
          - `core/vm/src/parser/decl/` - Callable, data, and dispatch declaration parsing.
          - `core/vm/src/parser/expr/` - Operator, postfix, and primary expression parsing.
          - `core/vm/src/parser/tests/` - Core syntax, strings, and type parser regressions.
        - `core/vm/src/relation/` - First-class relation bounded context.
          - `core/vm/src/relation/derivation/` - Full and indexed derived-fact evaluation, proofs, limits, and explanations.
          - `core/vm/src/relation/frontend/` - Bounded RFC-0003 lexer, parser, checker, manifests, and tooling.
          - `core/vm/src/relation/runtime/` - Authoritative relation store, candidates, manifests, encoding, and profiles.
        - `core/vm/src/replay/` - Replay codecs, identity, execution, and verification tests.
        - `core/vm/src/sandbox/` - Capability policy, isolation, and speculative-execution tests.
        - `core/vm/src/value/` - NaN-boxed values, persistent collections, objects, and value builtins.
        - `core/vm/src/vm/` - Bytecode execution, scheduling, builtins, manifests, and settlement integration.
          - `core/vm/src/vm/builtins_impl/` - Runtime builtin implementations grouped by stable domain responsibility.
          - `core/vm/src/vm/constraint_runtime/` - Constraint evaluator metering and tests.
          - `core/vm/src/vm/exec/` - Opcode families, frames, calls, scheduling, and execution helpers.
          - `core/vm/src/vm/settlement/` - Atomic candidates, commits, fact access, and World-Law gameplay tests.
        - `core/vm/src/wasm/` - Browser runtime API, execution bridge, and presentation packet encoder.
        - `core/vm/src/world/` - ECS storage, entity allocator, operations, snapshots, encoding, and tests.
      - `core/vm/tests/` - Public API, host, session, and executable RFC integration tests.
        - `core/vm/tests/rfc0003_reference/` - Independent accepted RFC-0003 semantic oracle.
          - `core/vm/tests/rfc0003_reference/tests/` - Behavior-grouped oracle and runtime-differential fixtures.
  - `docs/` - Canonical mdBook documentation and accepted RFC records.
    - `docs/rfcs/` - Canonical RFC bodies and sole editable RFC authority.
    - `docs/src/` - Rendered mdBook source and navigation.
      - `docs/src/examples/` - Narrative guides for examples and dogfood applications.
      - `docs/src/getting-started/` - Installation, first program, CLI, and playground onboarding.
      - `docs/src/guide/` - User-facing language and runtime guide.
      - `docs/src/project/` - Governance, roadmap, changelog, audits, and repository maps.
      - `docs/src/reference/` - Normative language, runtime, performance, and architecture references.
        - `docs/src/reference/builtins_parts/` - Builtin reference sections included by the composition page.
        - `docs/src/reference/spec_parts/` - Language-specification sections included by the composition page.
      - `docs/src/rfcs/` - Generated-by-inclusion mdBook wrappers for canonical RFC bodies.
    - `docs/theme/` - mdBook CSS and theme behavior overrides.
  - `examples/` - Small copy-and-run RAD programs and embedding examples.
  - `experiments/` - Non-authoritative implementation research.
    - `experiments/c-backend/` - Frozen self-hosted C/AOT experiment retained for research.
      - `experiments/c-backend/repro/` - Minimal historical backend failure reproductions.
      - `experiments/c-backend/src/` - Self-hosted frontend, C emitter/runtime, and WASM encoder sources.
  - `projects/` - Larger applications, integrations, tutorials, and product dogfood.
    - `projects/dogfood/` - Focused applications that stress language guarantees with real workloads.
      - `projects/dogfood/authority-effects/` - Transitive authority enforcement and graph-query dogfood.
      - `projects/dogfood/bastion/` - Sandbox capability and resource-exhaustion laboratory.
        - `projects/dogfood/bastion/bugs/` - Reproductions found by the bastion audit.
        - `projects/dogfood/bastion/caps/` - Malformed and adversarial capability fixtures.
      - `projects/dogfood/budget/` - Transactional budgeting application and input data.
      - `projects/dogfood/calcite/` - Parser/evaluator written in RAD to stress the language frontend.
        - `projects/dogfood/calcite/bugs/` - Calcite-discovered language regressions.
        - `projects/dogfood/calcite/checker/` - Static-checker diagnostic gallery and runner.
      - `projects/dogfood/causal-constraints/` - RFC-0002 candidate acceptance and rejection examples.
      - `projects/dogfood/causality/` - Provenance and `why()` explanation example.
      - `projects/dogfood/causal-laws/` - RFC-0001 proposal, resolver, and settlement example.
      - `projects/dogfood/collatz-lab/` - Exact structural Collatz search, certificates, and independent verifiers.
      - `projects/dogfood/deathsight/` - Terminal/browser dungeon simulation with speculative oracle play.
      - `projects/dogfood/foundry/` - ECS scheduling and mutation-boundary stress application.
        - `projects/dogfood/foundry/bugs/` - Minimal scheduler and mutation regressions.
      - `projects/dogfood/frankl-search/` - Exact and heuristic union-closed-family research workload.
        - `projects/dogfood/frankl-search/certificates/` - Checked mathematical result manifests and certificates.
      - `projects/dogfood/native-math-kernels/` - Project-owned native acceleration outside the generic VM.
        - `projects/dogfood/native-math-kernels/src/` - Extension implementation and registration root.
          - `projects/dogfood/native-math-kernels/src/affine_parity/` - Affine residue and parity search kernels.
          - `projects/dogfood/native-math-kernels/src/bin/` - Standalone mathematical analysis binaries.
          - `projects/dogfood/native-math-kernels/src/boolean_lattice/` - Boolean-lattice closure and audit kernels.
          - `projects/dogfood/native-math-kernels/src/lib/` - RAD extension ABI bindings and registration sections.
      - `projects/dogfood/opsdesk/` - Operations-desk workflow application.
      - `projects/dogfood/oracle/` - Speculative planner and beam-search application.
        - `projects/dogfood/oracle/bugs/` - Oracle-discovered runtime regressions.
        - `projects/dogfood/oracle/probes/` - Narrow performance and semantic probes.
          - `projects/dogfood/oracle/probes/purity/` - Purity and effect-boundary probes.
      - `projects/dogfood/orianna_gui/` - Orianna ability and browser-arena RAD sources.
      - `projects/dogfood/radgui/` - Generic declarative GUI and RADSCOPE dogfood.
        - `projects/dogfood/radgui/targets/` - Runnable applications rendered through RADGUI.
      - `projects/dogfood/radrad/` - RAD programs that inspect or exercise RAD itself.
        - `projects/dogfood/radrad/_probes/` - Focused self-hosting and language probes.
      - `projects/dogfood/radsheet/` - Spreadsheet application with persistence and collaboration.
        - `projects/dogfood/radsheet/demo/` - Scripted spreadsheet demonstrations and fixtures.
        - `projects/dogfood/radsheet/incident/` - Replayable spreadsheet incident reproduction.
      - `projects/dogfood/radtrack/` - Offline-first tracker and synchronization application.
        - `projects/dogfood/radtrack/demo/` - Tracker sync, conflict, replay, and migration demonstrations.
      - `projects/dogfood/schema/` - Versioned schema and saved-world migration fixtures.
      - `projects/dogfood/speculation/` - Fork, simulate, sandbox, and blast-radius examples.
      - `projects/dogfood/strongbox/` - Persistence, migration, replay, and hostile-save laboratory.
        - `projects/dogfood/strongbox/bugs/` - Persistence and replay regressions.
          - `projects/dogfood/strongbox/bugs/05_retro_write_hole/` - Retroactive-write replay reproduction assets.
          - `projects/dogfood/strongbox/bugs/08_dir/` - Directory-discovery test reproduction.
        - `projects/dogfood/strongbox/saves/` - Valid and forged world-save fixtures.
      - `projects/dogfood/sudoku/` - Sudoku solvers and profiling helpers.
        - `projects/dogfood/sudoku/data/` - Fixed benchmark puzzle corpus.
      - `projects/dogfood/syncdesk/` - Networked synchronization and soak-test application.
      - `projects/dogfood/tactics/` - Speculative tactics game, arena, replay, and tournament.
        - `projects/dogfood/tactics/bots/` - Sandboxed player-bot modules.
      - `projects/dogfood/timetravel/` - Record/replay, trace editing, and time-travel examples.
      - `projects/dogfood/todo/` - Minimal persistent task application.
      - `projects/dogfood/world-law-rpg/` - Ownership, weight, encumbrance, derivation, constraints, and `why()` vertical slice.
      - `projects/dogfood/worldmerge/` - Three-way world merge and conflict example.
    - `projects/moba/` - Single ownership namespace for every MOBA-specific artifact.
      - `projects/moba/kit/` - Reusable RAD gameplay corpus, modular kits, and golden fixtures.
        - `projects/moba/kit/gen/` - Generated or generation-oriented MOBA fixtures.
        - `projects/moba/kit/kit/` - Reusable MOBA gameplay modules.
        - `projects/moba/kit/tools/` - MOBA fixture generation and validation tools.
      - `projects/moba/simcore/` - Project-owned Rust/native/WASM acceleration for the MOBA damage core.
        - `projects/moba/simcore/src/` - Simcore implementation.
        - `projects/moba/simcore/tests/` - Golden parity tests against the authoritative simulation contract.
      - `projects/moba/vertical-slice/` - Networked MOBA product dogfood.
        - `projects/moba/vertical-slice/client/` - TypeScript browser client package and build configuration.
          - `projects/moba/vertical-slice/client/src/` - Client composition root, RAD host, scene, styles, and types.
            - `projects/moba/vertical-slice/client/src/app/` - Client orchestration, prediction, reconciliation, input, and telemetry.
            - `projects/moba/vertical-slice/client/src/characters/` - Character-specific presentation assets and adapters.
              - `projects/moba/vertical-slice/client/src/characters/clockworkMage/` - Clockwork Mage client presentation.
                - `projects/moba/vertical-slice/client/src/characters/clockworkMage/models/` - Clockwork Mage model construction.
            - `projects/moba/vertical-slice/client/src/generated/` - Generated TypeScript wire identities and offsets; never hand-edited.
            - `projects/moba/vertical-slice/client/src/netcode/` - Fixed-tick prediction, acknowledgement, and reconciliation primitives.
            - `projects/moba/vertical-slice/client/src/rad/` - RAD source embedded by the browser client.
            - `projects/moba/vertical-slice/client/src/render/` - Three.js rendering, interpolation, pooling, and overlays.
            - `projects/moba/vertical-slice/client/src/transport/` - Match protocol, buffering, wire parsing, and WebTransport.
            - `projects/moba/vertical-slice/client/src/ui/` - Browser HUD components.
          - `projects/moba/vertical-slice/client/test/` - Node tests for netcode, transport, rendering contracts, and app orchestration.
        - `projects/moba/vertical-slice/docs/` - Standalone MOBA architecture, protocol, replay, and operations book.
        - `projects/moba/vertical-slice/edge-proxy/` - Rust WebTransport/UDP edge that forwards opaque authority traffic.
          - `projects/moba/vertical-slice/edge-proxy/src/` - Proxy entry point and deterministic chaos injection.
        - `projects/moba/vertical-slice/protocol/` - Shared hand-edited wire contract and deterministic binding generator.
        - `projects/moba/vertical-slice/server/` - RAD authority package and server tooling.
          - `projects/moba/vertical-slice/server/scripts/` - Node launch helpers for the RAD authority.
          - `projects/moba/vertical-slice/server/src/` - Modular authoritative game source.
            - `projects/moba/vertical-slice/server/src/generated/` - Generated RAD wire identities and offsets; never hand-edited.
            - `projects/moba/vertical-slice/server/src/server/` - Server clocks, queues, state, and replay logging.
            - `projects/moba/vertical-slice/server/src/sim/` - Gameplay components, movement, projectiles, and lag compensation.
            - `projects/moba/vertical-slice/server/src/test/` - Headless authority smoke and abuse suites.
            - `projects/moba/vertical-slice/server/src/transport/` - Endpoint-local packet codec and UDP transport over the shared protocol contract.
            - `projects/moba/vertical-slice/server/src/world/` - Avatar and scene construction.
    - `projects/playground/` - Browser playground shells, workers, hosts, and shared demo scripts.
      - `projects/playground/demos/` - Standalone visual browser demonstrations.
        - `projects/playground/demos/moonlit-road/` - Moonlit Road tiled-scene demo.
          - `projects/playground/demos/moonlit-road/assets/` - Demo-owned visual assets.
            - `projects/playground/demos/moonlit-road/assets/moonlit-road/` - Moonlit Road tile and decoration images.
      - `projects/playground/relay/` - Small collaboration relay service and lockfile.
      - `projects/playground/test/` - Browser-session and playground behavior tests.
    - `projects/tutorial/` - Multi-step teaching projects.
      - `projects/tutorial/task-board/` - Task-board progression through state, provenance, replay, merge, and sync.
        - `projects/tutorial/task-board/03_replay/` - Buggy/fixed replay lesson and recorded session.
  - `tests/` - Repository-level language contracts and adversarial fixtures.
    - `tests/conformance/` - Snapshot-backed canonical language behavior.
      - `tests/conformance/modules/` - Module loading and graph conformance fixtures.
        - `tests/conformance/modules/adversarial_graph/` - Shared-dependency and adversarial import graph.
    - `tests/features/` - Focused attacks and feature regressions.
      - `tests/features/destructure/` - Destructuring syntax, control flow, and binding attacks.
    - `tests/fixtures/` - Versioned experimental-language fixture families.
      - `tests/fixtures/causal-constraints/` - RFC-0002 source and snapshot contracts.
      - `tests/fixtures/causal-laws/` - RFC-0001 source and snapshot contracts.
    - `tests/manual/` - Opt-in local probes outside automated conformance.
  - `tooling/` - Repository policy and developer tooling that does not own language semantics.
    - `tooling/editors/` - Editor integrations.
      - `tooling/editors/vscode/` - VS Code extension package and language configuration.
        - `tooling/editors/vscode/src/` - Extension implementation.
        - `tooling/editors/vscode/syntaxes/` - TextMate grammar.
    - `tooling/scripts/` - Documentation, matrix, restore, and VM-location helpers.
    - `tooling/templates/` - Project templates consumed by `rad new`.
```

## Placement rule

```text
language/runtime authority  -> core/vm/
host integration            -> adapters/
application/domain code     -> projects/
non-authoritative research  -> experiments/
language contracts          -> tests/
developer automation        -> tooling/
user-facing explanation     -> docs/src/
```

If a new directory does not have one nameable owner and responsibility, do not
add it. A line-limit split is not, by itself, a responsibility.
