# Adversarial Round 2 Findings (2026-03-28)

## Scope

New conformance stress tests focused on:
- deep recursion and recursion boundaries
- heavy/shared module import graphs
- weird map/list typing edges and nested value semantics
- higher-order pipeline depth with pure function composition

## New tests added

- `tests/conformance/adversarial_mutual_recursion_overflow.rad`
- `tests/conformance/adversarial_map_homogeneous_reject.rad` (name is historical — this test now validates that heterogeneous maps work correctly)
- `tests/conformance/adversarial_nested_value_semantics.rad`
- `tests/conformance/adversarial_high_order_pipeline_depth.rad`
- `tests/conformance/adversarial_module_graph_shared_deps.rad`
- module graph fixtures under `tests/conformance/modules/adversarial_graph/`

## Results

- Full conformance passes: `98` tests total.
- No fresh runtime crashes or backend parity mismatches found in this round.

## Notable constraints observed (non-crash)

1. **Forward references now work for top-level functions**
   - Calling a function before its declaration is accepted.
   - `ping()`/`pong()` mutual recursion now reaches runtime recursion checks.

2. **Heterogeneous maps are now supported**
   - Mixed-value map literals (`{"scores": [1,2,3], "owner_count": 3}`) are valid and infer `map<str, any>`.
   - The checker widens the value type to `any` when entries don't unify, rather than rejecting the literal.

3. **Pipeline purity remains strict**
   - Helper functions used inside pipelines must be declared `pure fn`.

## Verdict

No new correctness bug confirmed in this pass. The adversarial additions currently reinforce stability rather than exposing regressions.
