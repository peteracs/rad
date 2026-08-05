#[test]
fn derived_fact_rejects_movement_inside_the_atomic_candidate() {
    use crate::relation_frontend::{compile, FrontendOptions};
    use crate::relation_runtime::{
        EntityOperand, OperationMetadata, PendingFactKey, PendingRelationOperation,
        PendingRelationValue, RelationTransaction,
    };

    let source = r#"
component Position { x: int = 0 }
intent MoveIntent { key target: entity, amount: int }
law Move(target: entity, amount: int) {
    propose MoveIntent { target: target, amount: amount }
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

    let hero = vm.world.get_entity_by_name("hero").unwrap();
    let item = vm.world.spawn_entity(Some("anvil")).unwrap();
    let hero_ref = vm.world.entity_ref(hero).unwrap();
    let item_ref = vm.world.entity_ref(item).unwrap();
    let insert = |relation: &str, tuple| PendingRelationOperation::Insert {
        fact: PendingFactKey::new(relation, tuple),
        metadata: OperationMetadata::cause("dogfood.seed"),
    };
    vm.world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![
                insert(
                    "game::derived::Owns",
                    vec![
                        PendingRelationValue::Entity(EntityOperand::Existing(hero_ref)),
                        PendingRelationValue::Entity(EntityOperand::Existing(item_ref)),
                    ],
                ),
                insert(
                    "game::derived::ItemWeight",
                    vec![
                        PendingRelationValue::Entity(EntityOperand::Existing(item_ref)),
                        PendingRelationValue::Int(40),
                    ],
                ),
                insert(
                    "game::derived::CarryCapacity",
                    vec![
                        PendingRelationValue::Entity(EntityOperand::Existing(hero_ref)),
                        PendingRelationValue::Int(25),
                    ],
                ),
            ],
            ..RelationTransaction::default()
        })
        .expect("seed authoritative facts");
    let encumbered = crate::relation_runtime::FactKey::new(
        "game::derived::Encumbered",
        vec![crate::relation_runtime::FactValue::Entity(hero_ref)],
    );
    assert!(vm.world.derived_relation_state().facts().contains_key(&encumbered));

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
