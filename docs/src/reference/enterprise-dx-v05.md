# RAD v0.5 Enterprise DX Specification (Draft)

Status: Draft proposal
Target release: v0.5.0
Authors: RAD contributors

## 1. Goals

This document defines a concrete v0.5 developer-experience upgrade focused on:

- Reducing syntax surprises in sum-type construction.
- Reducing boilerplate in `match` destructuring.
- Improving diagnostic quality and migration safety.
- Providing enterprise-oriented coding conventions.

The proposal is additive and migration-safe for v0.5.x.

## 2. Pain Points Addressed

### 2.1 Zero-field sum variants are unintuitive

Current behavior requires:

```rad
AccessSignal::MfaDisabled { }
```

Using `AccessSignal::MfaDisabled` can be interpreted as a state-machine reference and produce confusing errors.

### 2.2 Match patterns become noisy

Large sum variants force all-needed-names style bindings:

```rad
match sig {
  SuspiciousGeo { account, region, severity } => { ... }
}
```

In enterprise handlers, identity often comes from entity context, making many payload fields redundant.

### 2.3 Ambiguous diagnostics

When `Type::Variant` is misinterpreted, diagnostics mention state-machine errors that do not explain the sum-type fix path.

## 3. Language Changes (v0.5)

## 3.1 Zero-field constructor shorthand

### New syntax

Allow:

```rad
TypeName::VariantName
```

as shorthand for:

```rad
TypeName::VariantName { }
```

### Semantics

- If `TypeName` resolves to a sum type and `VariantName` exists with zero fields, both forms construct the same variant value.
- If the variant has required fields and shorthand is used, emit a compile-time error with a fix-it hint.
- Existing braced constructor remains valid and unchanged.

### Grammar delta (conceptual)

Current (simplified):

```ebnf
primary = IDENT "::" IDENT [ "{" field_init* "}" ] | ...
```

v0.5 interpretation:

- `IDENT "::" IDENT "{" ... "}"` -> explicit variant constructor.
- `IDENT "::" IDENT` -> either zero-field variant constructor or state reference (resolved by disambiguation rules in Section 4).

## 3.2 Match rest binding

### New syntax

Allow:

```rad
match sig {
  SuspiciousGeo { region, .. } => { ... }
  MfaDisabled => { ... }
}
```

Equivalent explicit zero-field arm remains valid:

```rad
MfaDisabled { } => { ... }
```

Note: A bare variant match (`VariantName => { ... }`) is now supported for all variants, regardless of whether they have fields, when you only care about the variant tag.

### Semantics

- `..` means "ignore remaining fields of this variant".
- `..` does not bind a variable.
- At most one `..` per case.
- `..` must appear inside `{ ... }` and may appear with or without named bindings:
  - `Variant { .. }` valid.
  - `Variant { a, b, .. }` valid.

### Validation

- Unknown named binding in a variant case remains an error.
- Duplicate named bindings remain an error.
- Exhaustiveness rules are unchanged.

## 4. Disambiguation Rules for `Type::Variant`

Resolution order for `A::B` in expression position:

1. If followed by `{ ... }`, parse as explicit variant constructor syntax candidate.
2. If `A` is both a known sum type and state machine:
   - If sum type contains variant `B` with zero fields and no braces are provided, treat as sum constructor and emit informational note in `-W pedantic` mode.
   - If both interpretations remain valid and incompatible with context, emit explicit ambiguity error with fix-it choices.
3. If only sum type interpretation is valid, construct sum variant.
4. If only state-machine interpretation is valid, construct state reference.
5. If neither is valid, keep existing unknown-type/state diagnostic behavior but append contextual hint.

Required diagnostic hint in ambiguous cases:

- "Did you mean sum variant construction `A::B { }` or state reference `A::B`?"

## 5. Parser / AST / Checker Contract

This section defines responsibilities, not implementation details.

## 5.1 Parser responsibilities

- Preserve syntax-level ambiguity for `A::B` without `{ ... }`.
- Parse `match` cases with optional rest marker `..`.
- Enforce syntax invariants:
  - Only one rest marker per case.
  - Rest marker allowed only inside variant binding braces.

## 5.2 AST responsibilities

- Represent match-case bindings in a way that distinguishes:
  - named bindings
  - presence/absence of rest marker
- Represent qualified constructor/reference nodes such that checker can resolve ambiguity with symbol tables.

## 5.3 Checker responsibilities

- Resolve ambiguous `A::B` using declared sum/state symbols and contextual type expectations.
- Validate match bindings against actual variant fields.
- Produce actionable diagnostics with concrete rewrite suggestions.

## 6. Diagnostics Specification

## 6.1 Stable diagnostic codes

Add codes for new/changed diagnostics:

- `E2501`: reserved — ambiguous qualified reference (`A::B`) between state and sum variant. Currently unreachable at the source level because RAD's flat namespace prevents `type X` and `state X` from coexisting. Implemented as defense-in-depth for synthetic ASTs and future namespace evolution.
- `E2502`: zero-field shorthand used on non-zero-field variant.
- `E2503`: invalid rest marker placement in match pattern.
- `E2504`: unknown named binding in variant match pattern.
- `W2501`: compatibility warning for behavior that will become stricter in v0.6.

Code format should be stable and testable.

## 6.2 Required error style

Every error must include:

1. Problem summary.
2. One concrete fix-it.
3. Location with caret.

Example:

```text
Error[E2502]: Variant 'AccessSignal::LoginBurst' requires fields but shorthand was used
help: use 'AccessSignal::LoginBurst { extra_sessions: ..., region: ... }'
```

## 6.3 Warning policy flags

v0.5 CLI policy:

- `--deny-warnings` : warnings produce non-zero exit.
- `--warn-compat` : enable migration warnings (default: on for CLI, off for playground).

## 7. Migration and Compatibility Policy

## 7.1 v0.5.0 (additive)

- Accept both `Type::Variant` and `Type::Variant { }` for zero-field variants.
- Accept both `Variant` and `Variant { }` as zero-field match arms.
- Accept `Variant { fields, .. }`.

## 7.2 v0.5.x (warning hardening)

- Emit `W2501` when ambiguous syntax resolves through legacy fallback behavior.
- Encourage explicit form in diagnostics.

## 7.3 v0.6.0 (candidate)

- Turn unresolved `A::B` ambiguity into `E2501`.
- Keep compatibility flag (`--compat=0.5`) for one minor cycle if maintenance cost remains acceptable.

## 8. Test Matrix

## 8.1 Parser unit tests

Add tests for:

- Zero-field shorthand parse acceptance.
- Non-zero-field shorthand rejection.
- Match rest patterns (`{ .. }`, `{ a, .. }`, invalid duplicates).
- Ambiguity parse/resolve handoff coverage.

Target file:

- `core/vm/src/parser/tests.rs`

## 8.2 Checker tests

Add tests for:

- Ambiguity resolution and diagnostics (`E2501`, `E2502`).
- Match rest binding validation (`E2503`, `E2504`).

Suggested target:

- new checker tests module under `core/vm/src/checker/`.

## 8.3 Conformance tests

Add `.rad` conformance fixtures to verify:

- Expected output from `rad` invocations.
- Expected warnings/errors contain stable diagnostic codes.

Targets:

- `tests/conformance/` — `.rad` fixtures with `// expect:` / `// expect-runtime-error:` (and related) directives; verified via `rad snapshot tests/` against `.snap` baselines.
- Optional: extend the snapshot harness or fixture conventions (e.g. an explicit compile-error alias) in Rust (`core/vm/src/snapshot.rs` and friends), not a Python runner.

## 9. Enterprise Best Practices (v0.5 guidance)

## 9.1 Signal-envelope pattern

Use entity context as identity and keep signal payload as delta-only data.

Recommended envelope fields for audited systems:

- `trace_id`
- `source`
- `timestamp`
- `payload`

## 9.2 Handler convention

- Derive `account/service` identity from ECS context (`get(target, Account)`).
- Keep sum variant payload minimal (`region`, `severity`, deltas).
- Emit structured audit events with stable names.

## 9.3 Migration example

Before:

```rad
type AccessSignal {
  MfaDisabled { account: "" }
}
apply_signal(ci_bot, AccessSignal::MfaDisabled { account: "svc-ci-prod" })
```

After (v0.5 style):

```rad
type AccessSignal {
  MfaDisabled { }
}
apply_signal(ci_bot, AccessSignal::MfaDisabled)
```

## 10. Rollout Checklist

- [ ] Language spec updated with v0.5 syntax and examples.
- [ ] Parser/checker implementations complete with code-tagged diagnostics.
- [ ] Rust unit tests and conformance fixtures added.
- [ ] Changelog migration notes published.
- [ ] README includes link to this spec.

