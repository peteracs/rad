#[test]
fn derived_fact_rejects_movement_inside_the_atomic_candidate() {
    use crate::relation_frontend::{compile, FrontendOptions};

    let source = r#"
component Position { x: int = 0 }
intent EquipIntent {
    key owner: entity,
    item: entity,
    weight: int,
    capacity: int,
}
intent MoveIntent { key target: entity, amount: int }
law Equip(owner: entity, item: entity, weight: int, capacity: int) {
    propose EquipIntent {
        owner: owner,
        item: item,
        weight: weight,
        capacity: capacity,
    }
}
law Move(target: entity, amount: int) {
    propose MoveIntent { target: target, amount: amount }
}
resolver ResolveEquip for EquipIntent(owner, proposals) {
    let equipment = proposals[0]
    insert_fact("game::derived::Owns", [owner, equipment.item])
    insert_fact("game::derived::ItemWeight", [equipment.item, equipment.weight])
    insert_fact("game::derived::CarryCapacity", [owner, equipment.capacity])
}
resolver ResolveMove for MoveIntent(target, proposals) {
    let position = require(target, Position)
    let amount = proposals |> map(fn(proposal) { proposal.amount }) |> sum()
    next(target, Position { x: position.x + amount })
}
constraint MovementPermission for Position(subject, proposed) {
    require !candidate_fact("game::derived::Encumbered", [subject])
        else "movement.encumbered"
}
entity hero { Position {} }
entity anvil {}
fn equip() { settle { Equip(hero, anvil, 40, 25) } }
fn attempt_move() { settle { Move(hero, 3) } }
"#;
    let mut vm = crate::causal_laws_tests::compile_vm(source);
    vm.run(0).expect("initialize movement program");

    let artifacts = compile(
        r#"
relation Owns(owner: entity, item: entity)
relation ItemWeight(item: entity, weight: int)
relation CarryCapacity(person: entity, capacity: int)

derive TotalWeight(person, sum(weight))
    when Owns(person, item)
    and ItemWeight(item, weight)

derive Encumbered(person)
    when TotalWeight(person, total)
    and CarryCapacity(person, capacity)
    and total > capacity
"#,
        &FrontendOptions {
            enabled: true,
            module_id: "game::derived".into(),
            ..FrontendOptions::default()
        },
    )
    .expect("compile relation program");
    vm.install_relation_frontend(&artifacts)
        .expect("install relation program");

    vm.call_global_detailed("equip", &[])
        .expect("resolver-authored fact patch commits");

    let hero = vm.world.get_entity_by_name("hero").unwrap();
    let hero_ref = vm.world.entity_ref(hero).unwrap();
    let encumbered = crate::relation_runtime::FactKey::new(
        "game::derived::Encumbered",
        vec![crate::relation_runtime::FactValue::Entity(hero_ref)],
    );
    assert!(vm.world.derived_relation_state().facts().contains_key(&encumbered));
    let ownership = vm
        .world
        .relation_state()
        .assertions()
        .values()
        .find(|assertion| assertion.fact_key.relation == "game::derived::Owns")
        .expect("resolver inserted ownership assertion");
    assert!(ownership.causes.iter().any(|cause| {
        cause.starts_with(
            "resolution:ResolveEquip:EquipIntent:0:proposals=",
        )
    }));

    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt_move", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed settlement rejection expected")
    };
    assert_eq!(rejection.violations[0].code, "movement.encumbered");
    assert!(rejection.evaluation_failures.is_empty());
    assert_eq!(before, vm.observable_state_signature());
}

#[test]
fn resolver_fact_replace_and_remove_preserve_assertion_lifetimes() {
    use crate::relation_frontend::{compile, FrontendOptions};
    use crate::relation_runtime::{FactKey, FactValue};

    let source = r#"
intent AssignOwnership { key item: entity, owner: entity }
intent ReleaseOwnership { key item: entity, owner: entity }
law Assign(item: entity, owner: entity) {
    propose AssignOwnership { item: item, owner: owner }
}
law Release(item: entity, owner: entity) {
    propose ReleaseOwnership { item: item, owner: owner }
}
resolver ResolveAssignment for AssignOwnership(item, proposals) {
    let assignment = proposals[0]
    replace_fact_by(
        "game::inventory::Owns",
        "item",
        [item],
        [assignment.owner, item],
    )
}
resolver ResolveRelease for ReleaseOwnership(item, proposals) {
    remove_fact("game::inventory::Owns", [proposals[0].owner, item])
}
entity alice {}
entity bob {}
entity sword {}
fn assign_alice() { settle { Assign(sword, alice) } }
fn assign_bob() { settle { Assign(sword, bob) } }
fn release_bob() { settle { Release(sword, bob) } }
"#;
    let mut vm = crate::causal_laws_tests::compile_vm(source);
    vm.run(0).expect("initialize ownership program");
    let artifacts = compile(
        "relation Owns(owner: entity, item: entity)\n    unique item\n",
        &FrontendOptions {
            enabled: true,
            module_id: "game::inventory".into(),
            ..FrontendOptions::default()
        },
    )
    .expect("compile ownership relation");
    vm.install_relation_frontend(&artifacts)
        .expect("install ownership relation");

    let alice = vm.world.get_entity_by_name("alice").unwrap();
    let bob = vm.world.get_entity_by_name("bob").unwrap();
    let sword = vm.world.get_entity_by_name("sword").unwrap();
    let sword_ref = vm.world.entity_ref(sword).unwrap();
    vm.call_global_detailed("assign_alice", &[])
        .expect("first assignment commits");
    let first = vm
        .world
        .relation_state()
        .assertions()
        .values()
        .next()
        .unwrap()
        .assertion_id;

    vm.call_global_detailed("assign_bob", &[])
        .expect("replacement commits");
    let bob_fact = FactKey::new(
        "game::inventory::Owns",
        vec![
            FactValue::Entity(vm.world.entity_ref(bob).unwrap()),
            FactValue::Entity(sword_ref),
        ],
    );
    let replacement = vm
        .world
        .relation_state()
        .assertions()
        .get(&bob_fact)
        .expect("replacement owns sword");
    assert!(replacement.assertion_id > first);
    assert!(!vm.world.relation_state().assertions().contains_key(&FactKey::new(
        "game::inventory::Owns",
        vec![
            FactValue::Entity(vm.world.entity_ref(alice).unwrap()),
            FactValue::Entity(sword_ref),
        ],
    )));

    vm.call_global_detailed("release_bob", &[])
        .expect("removal commits");
    assert!(vm.world.relation_state().assertions().is_empty());
}

#[test]
fn fact_reads_are_sandbox_closed() {
    let mut vm = VM::new();
    vm.sandbox_caps = Some(std::sync::Arc::new(crate::sandbox::SandboxCaps::new(
        std::collections::HashSet::new(),
        1_000,
        1 << 20,
    )));
    let relation = Value::from_string(&mut vm.gc, "game::facts::Marker".into());
    let tuple = Value::list(&mut vm.gc, Vec::new());
    let error = vm
        .bi_constraint_fact(vec![relation, tuple], true)
        .expect_err("sandbox relation reads must fail closed");
    assert!(error.contains("capability-aware relation grant"), "{error}");
}

#[test]
fn fact_lookup_resource_quote_dominates_native_temporary_allocation() {
    use crate::relation_frontend::{compile, FrontendOptions};
    use crate::value::Builtin;
    use crate::vm::constraint_runtime::builtin_resource_charge;

    let artifacts = compile(
        "relation Marker(value: text)\n",
        &FrontendOptions {
            enabled: true,
            module_id: "game::facts".into(),
            ..FrontendOptions::default()
        },
    )
    .unwrap();
    let mut vm = VM::new();
    vm.install_relation_frontend(&artifacts).unwrap();
    let snapshot = vm.world.snapshot();
    vm.settlement = Some(SettlementContext {
        settlement_id: 1,
        owner_frame_id: 0,
        owner_chunk_id: 0,
        begin_ip: 0,
        base: snapshot.clone(),
        origin: crate::causality::Cause::Main,
        proposals: Vec::new(),
        patches: Vec::new(),
        candidate: Some(snapshot),
        relation_changes: Vec::new(),
        active: None,
        active_constraint: Some(ActiveConstraint {
            identity: crate::constraint_types::ConstraintIdentity {
                qualified_name: "game::facts::MarkerConstraint".into(),
                attached_component: "MarkerComponent".into(),
            },
            subject: 0,
            violations: Vec::new(),
            occurrences: BTreeMap::new(),
            retained_bytes: 0,
            max_retained_bytes: 1024,
            overflowed: false,
        }),
        next_proposal_id: 1,
    });
    let relation = Value::from_string(&mut vm.gc, "game::facts::Marker".into());
    let text = Value::from_string(&mut vm.gc, "x".repeat(16 * 1024));
    let tuple = Value::list(&mut vm.gc, vec![text]);
    let args = vec![relation, tuple];
    let quote = builtin_resource_charge(Builtin::CandidateFact, &args).unwrap();
    let (result, measured_peak) = crate::leak_lab::measure_peak_bytes(|| {
        vm.bi_constraint_fact(args, true)
    });
    assert_eq!(result.unwrap().as_bool(), Some(false));
    assert!(
        measured_peak <= quote.heap,
        "fact lookup: measured {measured_peak} > quoted {}",
        quote.heap
    );
}

#[test]
fn invalid_resolver_fact_write_rolls_back_the_complete_settlement() {
    use crate::relation_frontend::{compile, FrontendOptions};

    let source = r#"
component Counter { value: int = 0 }
intent BreakCandidate { key target: entity }
law Break(target: entity) { propose BreakCandidate { target: target } }
resolver ResolveBroken for BreakCandidate(target, proposals) {
    next(target, Counter { value: 1 })
    insert_fact("game::facts::Missing", [target])
}
entity hero { Counter {} }
fn attempt() { settle { Break(hero) } }
"#;
    let mut vm = crate::causal_laws_tests::compile_vm(source);
    vm.run(0).expect("initialize rollback program");
    let artifacts = compile(
        "relation Known(subject: entity)\n",
        &FrontendOptions {
            enabled: true,
            module_id: "game::facts".into(),
            ..FrontendOptions::default()
        },
    )
    .unwrap();
    vm.install_relation_frontend(&artifacts).unwrap();

    let before = vm.observable_state_signature();
    let error = vm.call_global_detailed("attempt", &[]).unwrap_err();
    assert!(error.to_string().contains("unknown relation"), "{error}");
    assert_eq!(before, vm.observable_state_signature());
    assert!(vm.world.relation_state().assertions().is_empty());
    assert_eq!(vm.world.relation_state().next_assertion_id(), 0);
}

#[test]
fn resolver_fact_writes_obey_the_sandbox_write_grant() {
    use crate::relation_frontend::{compile, FrontendOptions};

    let artifacts = compile(
        "relation Marker(subject: entity)\n",
        &FrontendOptions {
            enabled: true,
            module_id: "game::facts".into(),
            ..FrontendOptions::default()
        },
    )
    .unwrap();
    let mut vm = VM::new();
    vm.install_relation_frontend(&artifacts).unwrap();
    let entity = vm.world.spawn_entity(Some("subject")).unwrap();
    let snapshot = vm.world.snapshot();
    vm.settlement = Some(SettlementContext {
        settlement_id: 1,
        owner_frame_id: 0,
        owner_chunk_id: 0,
        begin_ip: 0,
        base: snapshot,
        origin: crate::causality::Cause::Main,
        proposals: Vec::new(),
        patches: Vec::new(),
        candidate: None,
        relation_changes: Vec::new(),
        active: Some(ActiveResolution {
            resolver: "ResolveMarker".into(),
            intent: "Mark".into(),
            key: entity,
            proposal_ids: vec![1],
            writes: Vec::new(),
            relation_operations: Vec::new(),
        }),
        active_constraint: None,
        next_proposal_id: 2,
    });
    let args = |vm: &mut VM| {
        let relation = Value::from_string(&mut vm.gc, "game::facts::Marker".into());
        let entity = Value::from_entity_id(&mut vm.gc, entity);
        let tuple = Value::list(&mut vm.gc, vec![entity]);
        vec![relation, tuple]
    };

    vm.sandbox_caps = Some(std::sync::Arc::new(crate::sandbox::SandboxCaps::new(
        std::collections::HashSet::new(),
        1_000,
        1 << 20,
    )));
    let denied_args = args(&mut vm);
    let denied = vm
        .bi_resolver_fact_write(denied_args, Builtin::InsertFact)
        .unwrap_err();
    assert!(denied.contains("game::facts::Marker"), "{denied}");
    assert!(vm
        .settlement
        .as_ref()
        .unwrap()
        .active
        .as_ref()
        .unwrap()
        .relation_operations
        .is_empty());

    vm.sandbox_caps = Some(std::sync::Arc::new(crate::sandbox::SandboxCaps::new(
        std::collections::HashSet::from(["game::facts::Marker".into()]),
        1_000,
        1 << 20,
    )));
    let allowed_args = args(&mut vm);
    vm.bi_resolver_fact_write(allowed_args, Builtin::InsertFact)
        .expect("relation identity grant permits staged write");
    assert_eq!(
        vm.settlement
            .as_ref()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .relation_operations
            .len(),
        1
    );
}
