# Push Limits Bug Log (2026-03-28)

This log captures issues observed while running `tests/conformance/push_limits_enterprise_weird.rad` and the full conformance suite.

## Scope

- Stress suite: `tests/conformance/push_limits_enterprise_weird.rad` (27 checks)
- Full conformance: `cargo run -p rad-cli --bin rad -- snapshot tests/` (from repo root; compares `tests/**/*.rad` against sibling `.snap` files), plus `cargo test -p rad-vm`

## Findings

### BL-001: State transition separators are parser-sharp

- **Status**: Reproducible parser behavior (not a runtime crash)
- **Repro**:
  - `state X { A { on e1 -> B, on e2 -> C } }`
- **Observed**:
  - Parser error at comma: `Expected On, got Comma`
- **Expected (DX)**:
  - Either accept comma-separated transitions or emit a targeted hint:
    - "Use whitespace-separated transitions: `on e1 -> B on e2 -> C`"
- **Workaround**:
  - Use `on e1 -> B on e2 -> C` inside the state arm.

### BL-002: Rest-pattern diagnostics

- **Status**: Resolved
- **Files**: `tests/conformance/match_rest.rad` and
  `tests/conformance/match_rest_unknown_binding.rad`
- **Observed**:
  - Rest patterns and their semantic diagnostics were split across parser modes.
- **Expected**:
  - `..` is canonical syntax and an unknown field reaches `E2504`.
- **Resolution**:
  - Parser modes were removed. Positive execution and exact negative diagnostics
    are permanent conformance fixtures.

### BL-003: Zero-field shorthand semantics

- **Status**: Resolved
- **File**: `tests/conformance/sum_type_zero_field_shorthand.rad`
- **Observed**:
  - The same source could be accepted or rejected by checker configuration.
- **Expected**:
  - Zero-field shorthand is canonical in every compiler entry point.
- **Resolution**:
  - Configuration-dependent parsing and checking were removed. Braced syntax
    explicitly selects a sum variant when a state-machine name is ambiguous.

## What passed

- New stress suite `push_limits_enterprise_weird.rad` passes in the Rust VM and C backend.
- Cross-backend output parity is preserved for all 27 checks.
