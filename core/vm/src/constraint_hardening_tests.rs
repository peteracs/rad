//! Focused regressions for RFC-0002 resource, output, and replay hardening.

use crate::causal_laws_tests::compile_vm;
use crate::host_value::FrozenValue;
use crate::value::{Builtin, Value};
use crate::vm::VM;
use crate::CausalValueLimits;

fn profile(
    fuel: u64,
    heap: usize,
    per_invocation: usize,
    settlement: usize,
) -> crate::constraint_types::ConstraintLimitProfile {
    crate::constraint_types::ConstraintLimitProfile::try_new(
        CausalValueLimits::default(),
        fuel,
        heap,
        per_invocation,
        settlement,
        1024 * 1024,
    )
    .unwrap()
}

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
        "constraint.invocation_violation_limit"
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
    let crate::constraint_types::CandidateCausalExplanation::Visible {
        resolver,
        intent,
        proposal_origins,
        ..
    } = explanation
    else {
        panic!("trusted explanation should remain visible")
    };
    assert_eq!(resolver, "ResolveMove");
    assert_eq!(intent, "Move");
    assert_eq!(proposal_origins.len(), 1);
    assert!(matches!(
        &proposal_origins[0],
        crate::constraint_types::RejectionProposalOrigin::Visible { law, .. }
            if law == "Push"
    ));
}

#[test]
fn straight_line_constraint_work_is_charged_per_opcode() {
    let mut body = String::from("let n0 = proposed.x\n");
    for index in 1..128 {
        body.push_str(&format!("let n{index} = n{} + 1\n", index - 1));
    }
    body.push_str("require false else \"work.done\"\n");
    let source = format!(
        r#"
component Position {{ x: int = 0 }}
intent Move {{ key target: entity }}
law Push(target: entity) {{ propose Move {{ target: target }} }}
resolver ResolveMove for Move(target, proposals) {{ next(target, Position {{ x: 1 }}) }}
constraint Work for Position(subject, proposed) {{
{body}
}}
entity hero {{ Position {{}} }}
fn attempt() {{ settle {{ Push(hero) }} }}
fn ping() {{ }}
"#
    );
    let mut vm = compile_vm(&source);
    vm.run(0).expect("initialize straight-line fuel program");
    vm.set_constraint_limit_profile(profile(20, 64 * 1024, 8, 16));

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("constraint resource failure should be a typed rejection")
    };
    assert_eq!(
        rejection.evaluation_failures[0].code,
        "constraint.fuel_exhausted"
    );
    assert_eq!(before, vm.observable_state_signature());
    vm.call_global("ping", &[]).expect("VM remains reusable");
}

#[test]
fn straight_line_aggregate_allocation_is_preflighted_and_reclaimed() {
    let entries = std::iter::repeat_n("proposed.x", 512)
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
component Position {{ x: int = 0 }}
intent Move {{ key target: entity }}
law Push(target: entity) {{ propose Move {{ target: target }} }}
resolver ResolveMove for Move(target, proposals) {{ next(target, Position {{ x: 1 }}) }}
constraint Allocate for Position(subject, proposed) {{
    let values = [{entries}]
    require len(values) > 0 else "allocation.complete"
}}
entity hero {{ Position {{}} }}
fn attempt() {{ settle {{ Push(hero) }} }}
fn ping() {{ }}
"#
    );
    let mut vm = compile_vm(&source);
    vm.run(0).expect("initialize straight-line heap program");
    vm.set_constraint_limit_profile(profile(10_000, 1024, 8, 16));

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("constraint resource failure should be a typed rejection")
    };
    assert_eq!(
        rejection.evaluation_failures[0].code,
        "constraint.memory_exhausted"
    );
    assert_eq!(before, vm.observable_state_signature());
    vm.call_global("ping", &[]).expect("VM remains reusable");
}

#[test]
fn allocation_heavy_pure_builtin_is_preflighted_before_work() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 1 }) }
constraint Allocate for Position(subject, proposed) {
    let values = range(0, 10000000)
    require len(values) > 0 else "allocation.complete"
}
entity hero { Position {} }
fn attempt() { settle { Push(hero) } }
fn ping() { }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize builtin heap program");
    vm.set_constraint_limit_profile(profile(10_000, 1024, 8, 16));

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("constraint resource failure should be a typed rejection")
    };
    assert_eq!(
        rejection.evaluation_failures[0].code,
        "constraint.memory_exhausted"
    );
    assert_eq!(before, vm.observable_state_signature());
    vm.call_global("ping", &[]).expect("VM remains reusable");
}

#[test]
fn unaudited_native_builtin_fails_closed_inside_constraint_meter() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 1 }) }
constraint Render for Position(subject, proposed) {
    let rendered = str(proposed)
    require len(rendered) > 0 else "render.empty"
}
entity hero { Position {} }
fn attempt() { settle { Push(hero) } }
fn ping() { }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize builtin fail-closed program");

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("unsupported native helper must reject the settlement")
    };
    assert_eq!(
        rejection.evaluation_failures[0].code,
        "constraint.evaluation_failed"
    );
    assert!(rejection.evaluation_failures[0]
        .message
        .contains("no mechanically verified native resource upper bound"));
    assert_eq!(before, vm.observable_state_signature());
    vm.call_global("ping", &[]).expect("VM remains reusable");
}

#[test]
fn settlement_outcome_limit_is_enforced_during_collection() {
    let mut constraints = String::new();
    for index in 0..12 {
        constraints.push_str(&format!(
            "constraint C{index} for Position(subject, proposed) {{\n\
             require false else \"c{index}.a\"\n\
             require false else \"c{index}.b\"\n\
             }}\n"
        ));
    }
    let source = format!(
        r#"
component Position {{ x: int = 0 }}
intent Move {{ key target: entity }}
law Push(target: entity) {{ propose Move {{ target: target }} }}
resolver ResolveMove for Move(target, proposals) {{ next(target, Position {{ x: 1 }}) }}
{constraints}
entity hero {{ Position {{}} }}
fn attempt() {{ settle {{ Push(hero) }} }}
fn ping() {{ }}
"#
    );
    let mut vm = compile_vm(&source);
    vm.run(0).expect("initialize outcome-meter program");
    vm.set_constraint_limit_profile(profile(10_000, 64 * 1024, 4, 5));

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("bounded rejection expected")
    };
    assert!(rejection.violations.is_empty());
    assert_eq!(rejection.evaluation_failures.len(), 1);
    assert_eq!(
        rejection.evaluation_failures[0].code,
        "constraint.settlement_outcome_limit"
    );
    assert_eq!(before, vm.observable_state_signature());
    vm.call_global("ping", &[]).expect("VM remains reusable");
}

#[test]
fn recorded_attempt_replay_ignores_later_parent_program_state() {
    let source = r#"
component Position { x: int = 0 }
component Unused { value: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 9 }) }
constraint Reject for Position(subject, proposed) {
    require false else "position.rejected"
}
constraint Allow for Unused(subject, proposed) {
    require true else "unused"
}
entity hero { Position {} }
fn attempt() { settle { Push(hero) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize replay isolation program");
    let crate::constraint_types::SettlementAttemptOutcome::Rejected(attempt) = vm
        .call_global_attempt("attempt", &[])
        .expect("record rejection")
    else {
        panic!("initial attempt must reject")
    };

    let reject_slot = vm
        .constraint_registry
        .iter()
        .find(|constraint| constraint.name == "Reject")
        .unwrap()
        .global_slot as usize;
    let allow_slot = vm
        .constraint_registry
        .iter()
        .find(|constraint| constraint.name == "Allow")
        .unwrap()
        .global_slot as usize;
    vm.globals[reject_slot] = vm.globals[allow_slot];

    let before = vm.observable_state_signature();
    vm.replay_failed_attempt(&attempt)
        .expect("recorded replay must use the pre-attempt constraint closure");
    assert_eq!(before, vm.observable_state_signature());

    let failure = vm
        .replay_portable_failed_attempt(&attempt.portable_recipe())
        .expect_err("portable replay must reject the later parent checkpoint");
    assert!(matches!(
        failure,
        crate::constraint_types::VmFailure::Host(crate::constraint_types::HostFault {
            ref code,
            ..
        }) if code == "attempt.checkpoint_mismatch"
    ));
    assert_eq!(before, vm.observable_state_signature());
}

#[test]
fn portable_attempt_replay_binds_global_names_to_exact_slots() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 9 }) }
constraint Reject for Position(subject, proposed) {
    require false else "position.rejected"
}
entity hero { Position {} }
fn attempt() { settle { Push(hero) } }
fn alternate() { return 42 }
"#;
    let mut recorder = compile_vm(source);
    recorder.run(0).expect("initialize recorded program");
    let crate::constraint_types::SettlementAttemptOutcome::Rejected(recorded) = recorder
        .call_global_attempt("attempt", &[])
        .expect("record portable rejection")
    else {
        panic!("attempt must reject")
    };

    let mut supplied = compile_vm(source);
    supplied.run(0).expect("initialize supplied checkpoint");
    let attempt_slot = supplied
        .global_names
        .iter()
        .position(|name| name == "attempt")
        .unwrap();
    let alternate_slot = supplied
        .global_names
        .iter()
        .position(|name| name == "alternate")
        .unwrap();
    std::sync::Arc::make_mut(&mut supplied.global_names).swap(attempt_slot, alternate_slot);

    let before = supplied.observable_state_signature();
    let error = supplied
        .replay_portable_failed_attempt(&recorded.portable_recipe())
        .expect_err("changed global symbol mapping must fail before replay");
    assert!(matches!(
        error,
        crate::constraint_types::VmFailure::Host(crate::constraint_types::HostFault {
            ref code,
            ..
        }) if code == "attempt.program_mismatch"
    ));
    assert_eq!(before, supplied.observable_state_signature());
}

#[test]
fn portable_attempt_replay_rejects_hidden_allocator_drift_before_execution() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 9 }) }
constraint Reject for Position(subject, proposed) {
    require false else "position.rejected"
}
let mut attempt_runs = 0
fn attempt() {
    attempt_runs = attempt_runs + 1
    let spawned = spawn(Position {})
    settle { Push(spawned) }
}
"#;
    let mut recorder = compile_vm(source);
    recorder.run(0).expect("initialize recorded program");
    let crate::constraint_types::SettlementAttemptOutcome::Rejected(recorded) = recorder
        .call_global_attempt("attempt", &[])
        .expect("record portable rejection")
    else {
        panic!("attempt must reject")
    };

    let mut supplied = compile_vm(source);
    supplied.run(0).expect("initialize supplied checkpoint");
    let visible_before = supplied.world.snapshot_json_like();
    let mut altered = supplied.world.snapshot();
    altered.free_ids = vec![42];
    supplied.world.restore(altered);
    assert_eq!(visible_before, supplied.world.snapshot_json_like());

    let runs_slot = supplied
        .global_names
        .iter()
        .position(|name| name == "attempt_runs")
        .unwrap();
    assert_eq!(supplied.globals[runs_slot].as_int(), Some(0));
    let error = supplied
        .replay_portable_failed_attempt(&recorded.portable_recipe())
        .expect_err("allocator drift must fail before replay execution");
    assert!(matches!(
        error,
        crate::constraint_types::VmFailure::Host(crate::constraint_types::HostFault {
            ref code,
            ..
        }) if code == "attempt.checkpoint_mismatch"
    ));
    assert_eq!(supplied.globals[runs_slot].as_int(), Some(0));
}

#[test]
fn portable_attempt_replay_rejects_nested_world_fork_topology_drift_before_execution() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 9 }) }
constraint Reject for Position(subject, proposed) {
    require false else "position.rejected"
}
entity hero { Position {} }
let mut topology = nil
let mut attempt_runs = 0
fn attempt() {
    attempt_runs = attempt_runs + 1
    settle { Push(hero) }
}
"#;
    let mut recorder = compile_vm(source);
    recorder.run(0).expect("initialize recorded program");
    let topology_slot = recorder
        .global_names
        .iter()
        .position(|name| name == "topology")
        .unwrap();
    let shared_snapshot = std::sync::Arc::new(recorder.world.snapshot());
    let shared_left = Value::world_fork(&mut recorder.gc, shared_snapshot.clone());
    let shared_right = Value::world_fork(&mut recorder.gc, shared_snapshot);
    recorder.globals[topology_slot] =
        Value::list(&mut recorder.gc, vec![shared_left, shared_right]);
    let crate::constraint_types::SettlementAttemptOutcome::Rejected(recorded) = recorder
        .call_global_attempt("attempt", &[])
        .expect("record portable rejection")
    else {
        panic!("attempt must reject")
    };

    let mut supplied = compile_vm(source);
    supplied.run(0).expect("initialize supplied checkpoint");
    let topology_slot = supplied
        .global_names
        .iter()
        .position(|name| name == "topology")
        .unwrap();
    let snapshot = supplied.world.snapshot();
    let distinct_left = Value::world_fork(&mut supplied.gc, std::sync::Arc::new(snapshot.clone()));
    let distinct_right = Value::world_fork(&mut supplied.gc, std::sync::Arc::new(snapshot));
    supplied.globals[topology_slot] =
        Value::list(&mut supplied.gc, vec![distinct_left, distinct_right]);
    let runs_slot = supplied
        .global_names
        .iter()
        .position(|name| name == "attempt_runs")
        .unwrap();

    let error = supplied
        .replay_portable_failed_attempt(&recorded.portable_recipe())
        .expect_err("nested WorldFork topology drift must fail before replay");
    assert!(matches!(
        error,
        crate::constraint_types::VmFailure::Host(crate::constraint_types::HostFault {
            ref code,
            ..
        }) if code == "attempt.checkpoint_mismatch"
    ));
    assert_eq!(supplied.globals[runs_slot].as_int(), Some(0));
}

#[test]
fn matching_operational_checkpoint_replays_same_spawned_candidate() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 9 }) }
constraint Reject for Position(subject, proposed) {
    require false else "position.rejected"
}
fn attempt() {
    let spawned = spawn(Position {})
    settle { Push(spawned) }
}
"#;
    let mut recorder = compile_vm(source);
    recorder.run(0).expect("initialize recorded program");
    let crate::constraint_types::SettlementAttemptOutcome::Rejected(recorded) = recorder
        .call_global_attempt("attempt", &[])
        .expect("record portable rejection")
    else {
        panic!("attempt must reject")
    };

    let mut supplied = compile_vm(source);
    supplied.run(0).expect("initialize matching checkpoint");
    let actual = supplied
        .replay_portable_failed_attempt(&recorded.portable_recipe())
        .expect("matching operational checkpoint must replay");
    let expected_bytes = recorded
        .rejection
        .canonical_bytes(&supplied.constraint_limit_profile)
        .expect("encode expected rejection");
    let actual_bytes = actual
        .canonical_bytes(&supplied.constraint_limit_profile)
        .expect("encode replay rejection");
    assert_eq!(expected_bytes, actual_bytes);
}

#[test]
fn failed_attempt_replay_does_not_mutate_parent_capture_cells() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity, amount: int }
law Push(target: entity, amount: int) { propose Move { target: target, amount: amount } }
resolver ResolveMove for Move(target, proposals) {
    next(target, Position { x: proposals[0].amount })
}
constraint Reject for Position(subject, proposed) {
    require false else "position.rejected"
}
fn make_counter() {
    let mut count = 0
    return fn() {
        count = count + 1
        return count
    }
}
let counter = make_counter()
entity hero { Position {} }
fn attempt() {
    let amount = counter()
    settle { Push(hero, amount) }
}
fn read_counter() { return counter() }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize capture replay program");
    let crate::constraint_types::SettlementAttemptOutcome::Rejected(attempt) = vm
        .call_global_attempt("attempt", &[])
        .expect("record rejection after first counter increment")
    else {
        panic!("initial attempt must reject")
    };

    let expected = &attempt.rejection.candidate_details[&attempt.rejection.violations[0].candidate];
    let crate::constraint_types::RejectionValue::Visible(FrozenValue::Component {
        fields: expected_fields,
        ..
    }) = expected
    else {
        panic!("visible candidate detail expected")
    };
    assert_eq!(expected_fields["x"], FrozenValue::Int(1));

    let replayed = vm
        .replay_failed_attempt(&attempt)
        .expect("replay must reproduce rejection in a detached graph");
    let replayed_detail = &replayed.candidate_details[&replayed.violations[0].candidate];
    assert_eq!(
        replayed_detail, expected,
        "replay must start from count = 0"
    );
    assert_eq!(
        vm.call_global("read_counter", &[])
            .expect("read parent counter"),
        FrozenValue::Int(2),
        "replay must increment only the child capture cell"
    );

    let replayed_again = vm
        .replay_failed_attempt(&attempt)
        .expect("later parent changes cannot move the replay checkpoint");
    assert_eq!(
        replayed_again.candidate_details[&replayed_again.violations[0].candidate],
        expected.clone()
    );
}

#[test]
fn replay_graph_preserves_child_aliases_cycles_and_shared_captures() {
    let source = r#"
fn make_pair() {
    let mut count = 0
    let reader = fn() { return count }
    let writer = fn() {
        count = count + 1
        return count
    }
    return [reader, writer]
}
let pair = make_pair()
let reader = pair[0]
let writer = pair[1]
let shared = [42]
let aliases = [shared, shared]
let large = filled(16384, 7)
fn make_cycle() {
    let mut holder: any = nil
    let cycle = fn() { return holder }
    holder = cycle
    return cycle
}
let cycle = make_cycle()
fn read_counter() { return reader() }
fn write_counter() { return writer() }
"#;
    let mut parent = compile_vm(source);
    parent.run(0).expect("initialize replay graph program");
    let mut child = parent
        .detached_attempt_replay_vm()
        .expect("clone cyclic replay graph");

    assert_eq!(
        child.call_global("write_counter", &[]).unwrap(),
        FrozenValue::Int(1)
    );
    assert_eq!(
        child.call_global("read_counter", &[]).unwrap(),
        FrozenValue::Int(1),
        "child reader and writer must share the rewritten capture cell"
    );
    assert_eq!(
        parent.call_global("read_counter", &[]).unwrap(),
        FrozenValue::Int(0),
        "child capture mutation must not reach the parent"
    );

    let slot = |vm: &VM, name: &str| {
        vm.global_names
            .iter()
            .position(|candidate| candidate == name)
            .unwrap()
    };
    let parent_aliases = parent.globals[slot(&parent, "aliases")];
    let child_aliases = child.globals[slot(&child, "aliases")];
    let parent_items = parent_aliases.as_list().unwrap();
    let child_items = child_aliases.as_list().unwrap();
    assert_eq!(
        child_items.get(0).unwrap().object_identity(),
        child_items.get(1).unwrap().object_identity(),
        "shared child DAG edges must remain shared"
    );
    assert_ne!(
        parent_items.get(0).unwrap().object_identity(),
        child_items.get(0).unwrap().object_identity(),
        "child DAG nodes must not point into the parent heap"
    );
    let child_large = child.globals[slot(&child, "large")];
    let large_retained = child_large.as_object().unwrap().accounted_heap_bytes();
    assert!(
        child.gc.bytes_allocated()
            >= std::mem::size_of::<crate::value::Object>().saturating_add(large_retained),
        "populated replay objects must replace placeholder accounting"
    );

    let parent_cycle = parent.globals[slot(&parent, "cycle")];
    let child_cycle = child.globals[slot(&child, "cycle")];
    let parent_capture = parent_cycle.as_closure().unwrap().captures[0];
    let child_capture = child_cycle.as_closure().unwrap().captures[0];
    assert_ne!(parent_capture, child_capture);
    assert_eq!(
        unsafe { (*child_capture).get() }.object_identity(),
        child_cycle.object_identity(),
        "the self-cycle must close over the child closure"
    );
}

#[test]
fn observational_attempt_replay_fails_closed_on_host_effects() {
    let parent = VM::new_with_seed(7);
    let mut child = parent
        .detached_attempt_replay_vm()
        .expect("construct observational replay VM");

    let error = child
        .call_builtin(Builtin::SleepMs, vec![Value::int(0)])
        .expect_err("observational replay must not execute host effects");
    assert!(error.contains("irreversible host effect"), "{error}");
}

#[test]
fn failed_attempt_replay_preserves_main_timeline_emit_after_semantics() {
    let source = r#"
event Ping {}
component Position { x: int = 0 }
intent Move { key target: entity, amount: int }
law Push(target: entity) { propose Move { target: target, amount: 1 } }
resolver ResolveMove for Move(target, proposals) {
    next(target, Position { x: proposals[0].amount })
}
constraint Reject for Position(subject, proposed) {
    require false else "position.rejected"
}
entity hero { Position {} }
fn attempt() {
    emit Ping {} after 1
    settle { Push(hero) }
}
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize replay-role program");
    let crate::constraint_types::SettlementAttemptOutcome::Rejected(attempt) = vm
        .call_global_attempt("attempt", &[])
        .expect("main-timeline emit-after must reach the rejecting settlement")
    else {
        panic!("attempt must reject")
    };
    assert_eq!(vm.delayed_events.len(), 1);
    let parent_before_replay = vm.observable_state_signature();

    let replayed = vm
        .replay_failed_attempt(&attempt)
        .expect("replay must retain main-timeline event semantics");
    assert_eq!(
        replayed
            .canonical_bytes(vm.constraint_limit_profile())
            .unwrap(),
        attempt
            .rejection
            .canonical_bytes(vm.constraint_limit_profile())
            .unwrap()
    );
    assert_eq!(parent_before_replay, vm.observable_state_signature());
}

#[test]
fn public_attempt_recording_rejects_worker_execution_role() {
    let main = VM::new_with_seed(7);
    let mut worker = VM::from_shared_state(main.shared_state());
    let error = worker
        .call_global_attempt("missing", &[])
        .expect_err("worker attempts are not authoritative replay roots");
    assert!(error.to_string().contains("main-timeline"), "{error}");
}

#[test]
fn checkpoint_digest_tracks_replay_semantic_execution_fields() {
    let source = r#"
event Ping {}
on Ping once(e) {}
"#;
    let mut vm = compile_vm(source);
    vm.run(0)
        .expect("initialize checkpoint sensitivity program");
    let baseline = vm.attempt_checkpoint_digest();

    vm.is_worker = true;
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
    vm.is_worker = false;

    vm.serial_schedule = true;
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
    vm.serial_schedule = false;

    vm.trace_timeline = true;
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
    vm.trace_timeline = false;

    vm.current_trace_id = Some(41);
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
    vm.current_trace_id = None;

    vm.in_simulation_fork = 1;
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
    vm.in_simulation_fork = 0;

    vm.emit_ids_current.push(7);
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
    vm.emit_ids_current.clear();
    vm.emit_ids_next.push(8);
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
    vm.emit_ids_next.clear();

    let handlers = std::sync::Arc::make_mut(&mut vm.event_handlers);
    handlers.get_mut("Ping").unwrap()[0].fired = true;
    assert_ne!(baseline, vm.attempt_checkpoint_digest());
}
