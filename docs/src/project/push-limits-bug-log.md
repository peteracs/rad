# Push Limits Bug Log (2026-03-28)

This log captures issues observed while running `tests/conformance/push_limits_enterprise_weird.rad` and the full conformance suite.

## Scope

- Stress suite: `tests/conformance/push_limits_enterprise_weird.rad` (27 checks)
- Full conformance: `cargo run -p rad-vm --bin rad -- snapshot tests/` (from repo root; compares `tests/**/*.rad` against sibling `.snap` files), plus `cargo test -p rad-vm`

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

### BL-002: v0.5 DX rest-binding flag diagnostic mismatch

- **Status**: Resolved
- **File**: `tests/conformance/v05_dx_match_rest_requires_flag.rad`
- **Observed**:
  - The expected `E2503` flag-gate diagnostic is not emitted.
  - Parser/checker reports follow-on errors (`Unknown component type 'Some'`, etc.).
- **Expected**:
  - A direct diagnostic that `..` in match patterns requires `--compat-v0.5-dx`.
- **Resolution**:
  - The default parser and checker options were updated to correctly disable `compat_v0_5_dx` by default, enforcing the opt-in requirement.

### BL-003: v0.5 zero-field shorthand flag-gate mismatch

- **Status**: Resolved
- **File**: `tests/conformance/v05_dx_zero_field_shorthand_requires_flag.rad`
- **Observed**:
  - Test expects non-zero exit without flag, but execution succeeds.
- **Expected**:
  - Consistent flag-gated behavior for zero-field shorthand.
- **Resolution**:
  - The checker's resolution for `Expr::StateRef` was updated to strictly gate zero-field sum variant shorthand behind the `--compat-v0.5-dx` flag, emitting `E2502` when the flag is missing.

## What passed

- New stress suite `push_limits_enterprise_weird.rad` passes in the Rust VM and C backend.
- Cross-backend output parity is preserved for all 27 checks.
