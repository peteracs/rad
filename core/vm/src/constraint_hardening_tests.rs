//! Focused regressions for RFC-0002 resource, output, and replay hardening.

use crate::causal_laws_tests::compile_vm;
use crate::host_value::FrozenValue;
use crate::vm::VM;
use crate::CausalValueLimits;

#[test]
fn transaction_value_limits_cannot_diverge_from_constraint_value_limits() {
    let mut vm = VM::new_with_seed(7);
    let narrow = CausalValueLimits::default()
        .with_max_encoded_bytes(4_096)
        .unwrap();
    vm.set_constraint_limit_profile(
        crate::constraint_types::ConstraintLimitProfile::default()
            .with_value_limits(narrow)
            .unwrap(),
    );
    assert_eq!(vm.causal_value_limits(), narrow);
    assert_eq!(vm.constraint_limit_profile().value_limits(), narrow);

    let wider = CausalValueLimits::default()
        .with_max_encoded_bytes(16_384)
        .unwrap();
    vm.set_causal_value_limits(wider);
    assert_eq!(vm.causal_value_limits(), wider);
    assert_eq!(vm.constraint_limit_profile().value_limits(), wider);
}

#[test]
fn builtin_require_remains_an_ordinary_call_outside_constraint_context() {
    let source = r#"
component Health { hp: int = 100 }
entity hero { Health {} }
fn read_health() -> int { return require(hero, Health).hp }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize ordinary require program");
    assert_eq!(
        vm.call_global("read_health", &[]).expect("ordinary call"),
        FrozenValue::Int(100)
    );
}

#[test]
fn aggregate_constraint_budget_fails_before_any_invocation_and_vm_is_reusable() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 5 }) }
constraint First for Position(subject, proposed) { require true else "first" }
constraint Second for Position(subject, proposed) { require true else "second" }
entity hero { Position {} }
fn attempt() { settle { Push(hero) } }
fn ping() { }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize aggregate-budget program");
    let profile = crate::constraint_types::ConstraintLimitProfile::try_new(
        CausalValueLimits::default(),
        100,
        1024,
        16,
        32,
        32 * 1024,
    )
    .unwrap()
    .with_aggregate_limits(100, 2 * 1024)
    .unwrap();
    vm.set_constraint_limit_profile(profile);

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    assert!(matches!(
        failure,
        crate::constraint_types::VmFailure::Host(crate::constraint_types::HostFault {
            ref code,
            ..
        }) if code == "constraint.aggregate_budget_unavailable"
    ));
    assert_eq!(before, vm.observable_state_signature());
    assert!(vm.settlement.is_none());
    vm.call_global("ping", &[])
        .expect("VM remains reusable after aggregate-budget rejection");
}

#[test]
fn large_repeated_candidate_rejection_uses_one_bounded_detail() {
    let source = r#"
component Data { text: str = "before" }
intent Replace { key target: entity }
law Push(target: entity) { propose Replace { target: target } }
resolver ResolveReplace for Replace(target, proposals) {
    let large = "y" * 8000000
    next(target, Data { text: large })
}
constraint RejectMany for Data(subject, proposed) {
    for index in range(0, 256) {
        require false else "data.rejected"
    }
}
entity item { Data {} }
fn attempt() { settle { Push(item) } }
fn ping() { }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize bounded-rejection program");
    vm.set_constraint_limit_profile(
        crate::constraint_types::ConstraintLimitProfile::try_new(
            CausalValueLimits::default(),
            100_000,
            1024 * 1024,
            256,
            512,
            1024,
        )
        .unwrap(),
    );

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed rejection expected")
    };
    assert!(rejection.violations.is_empty());
    assert!(rejection.candidate_details.is_empty());
    assert_eq!(
        rejection.evaluation_failures[0].code,
        "constraint.outcome_byte_limit"
    );
    assert!(
        rejection
            .canonical_bytes(vm.constraint_limit_profile())
            .expect("bounded fallback encoding")
            .len()
            <= 1024
    );
    assert_eq!(before, vm.observable_state_signature());
    vm.call_global("ping", &[])
        .expect("VM remains reusable after bounded rejection");
}

#[test]
fn rejection_explanation_follows_only_the_owning_candidate_patch() {
    let source = r#"
component Position { x: int = 0 }
component Health { hp: int = 100 }
intent Move { key target: entity }
intent Damage { key target: entity }
law Push(target: entity) { propose Move { target: target } }
law Hit(target: entity) { propose Damage { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 9 }) }
resolver ResolveDamage for Damage(target, proposals) { next(target, Health { hp: 90 }) }
constraint WorldBounds for Position(subject, proposed) {
    require proposed.x <= 3 else "position.above_max"
}
entity hero { Position {}, Health {} }
fn attempt() {
    settle {
        Hit(hero)
        Push(hero)
    }
}
"#;
    let mut vm = compile_vm(source);
    vm.run(0)
        .expect("initialize candidate-specific why program");
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed rejection expected")
    };
    assert_eq!(rejection.explanation.candidates.len(), 1);
    let candidate = &rejection.violations[0].candidate;
    let explanation = &rejection.explanation.candidates[candidate];
    assert_eq!(explanation.resolver, "ResolveMove");
    assert_eq!(explanation.intent, "Move");
    assert_eq!(explanation.proposal_origins.len(), 1);
    assert!(matches!(
        &explanation.proposal_origins[0],
        crate::constraint_types::RejectionProposalOrigin::Visible { law, .. }
            if law == "Push"
    ));
}
