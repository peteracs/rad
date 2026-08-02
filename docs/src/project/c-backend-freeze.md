# C Backend Freeze

`core/c-backend/` is frozen legacy code. It is not part of normal Rad language
development, and it is not a source of truth for syntax, checking, runtime
semantics, docs, playground behavior, or feature support.

## Ground Truth

The ground truth is `core/vm/`:

- Rust parser, checker, compiler, VM, formatter, LSP, and WASM bindings.
- `tests/conformance/` snapshots as executed by the Rust VM.
- Dogfood and playground behavior that runs through `rad-vm`.
- The documentation in `docs/src/`.

When `core/vm` and `core/c-backend` disagree, `core/vm` wins.

## What Frozen Means

- Do not touch `core/c-backend/` during normal feature work.
- Do not patch C-backend warnings or failures while developing `core/vm`.
- Do not require C-backend tests in CI or release health checks.
- Do not cite C-backend behavior as current language support.
- Keep generated C artifacts out of source control.

The old harnesses remain in place only for archaeology and possible future
revival. They require `RAD_RUN_FROZEN_C_BACKEND=1` so nobody runs them by
accident and interprets their result as project health.

## Revival Bar

A future C/AOT backend should be downstream of VM-owned semantics. The minimum
revival plan is:

1. Reuse `tests/conformance/` as the single parity source.
2. Maintain an explicit support manifest for passed, unsupported, and expected
   failure cases.
3. Fail CI only on unclassified parity drift.
4. Prefer a typed/lowered IR owned by `core/vm` before investing in long-term
   backend parity.

Until then, C-backend status is: preserved, frozen, non-authoritative.
