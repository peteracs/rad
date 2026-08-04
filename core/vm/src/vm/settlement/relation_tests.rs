#[test]
fn relation_patch_requires_a_resolver_and_preserves_proposal_fan_in() {
    use crate::relation_frontend::{compile, FrontendOptions};
    use crate::relation_runtime::{
        OperationMetadata, PendingFactKey, PendingRelationOperation, PendingRelationValue,
        RelationTransaction,
    };

    let artifacts = compile(
        "relation Marker(value: int)\n",
        &FrontendOptions {
            enabled: true,
            module_id: "game::settlement".into(),
            ..FrontendOptions::default()
        },
    )
    .unwrap();
    let mut vm = VM::new_with_seed(37);
    vm.install_relation_frontend(&artifacts).unwrap();
    let base = vm.world.snapshot();
    vm.settlement = Some(SettlementContext {
        settlement_id: 1,
        owner_frame_id: 0,
        owner_chunk_id: 0,
        begin_ip: 0,
        base,
        origin: crate::causality::Cause::Main,
        proposals: Vec::new(),
        patches: Vec::new(),
        candidate: None,
        relation_changes: Vec::new(),
        active: None,
        active_constraint: None,
        next_proposal_id: 3,
    });

    let operation = || PendingRelationOperation::Insert {
        fact: PendingFactKey::new(
            "game::settlement::Marker",
            vec![PendingRelationValue::Int(5)],
        ),
        metadata: OperationMetadata::cause("resolver.write"),
    };
    assert_eq!(
        vm.stage_relation_operation(operation()).unwrap_err(),
        "relation patches are only valid inside a resolver"
    );

    vm.settlement.as_mut().unwrap().active = Some(ActiveResolution {
        resolver: "game::settlement::ResolveMarker".into(),
        intent: "game::settlement::Mark".into(),
        key: 42,
        proposal_ids: vec![11, 12],
        writes: Vec::new(),
        relation_operations: Vec::new(),
    });
    vm.stage_relation_operation(operation()).unwrap();
    let active = vm.settlement.as_mut().unwrap().active.take().unwrap();
    let expected_cause = relation_resolution_cause(
        &active.resolver,
        &active.intent,
        active.key,
        &active.proposal_ids,
    );
    assert!(active.relation_operations[0]
        .metadata()
        .causes
        .contains(&expected_cause));

    let changes = vm
        .world
        .apply_relation_transaction(&RelationTransaction {
            operations: active.relation_operations.clone(),
            ..RelationTransaction::default()
        })
        .unwrap();
    let context = vm.settlement.as_mut().unwrap();
    context.proposals = vec![
        Proposal {
            id: 11,
            intent: active.intent.clone(),
            key: active.key,
            payload: Value::NIL,
            canonical: vec![0],
            law: "law.alpha".into(),
            source_line: 1,
        },
        Proposal {
            id: 12,
            intent: active.intent.clone(),
            key: active.key,
            payload: Value::NIL,
            canonical: vec![1],
            law: "law.beta".into(),
            source_line: 2,
        },
    ];
    context.patches.push(ResolutionPatch {
        resolver: active.resolver,
        intent: active.intent,
        key: active.key,
        proposal_ids: active.proposal_ids,
        writes: Vec::new(),
        relation_operations: active.relation_operations,
    });
    context.relation_changes = changes;
    let context = vm.settlement.take().unwrap();
    vm.record_settlement_provenance(&context);

    let resolution = vm.ledger.resolutions.back().unwrap();
    assert_eq!(resolution.proposal_ids, vec![1, 2]);
    let write = vm.ledger.writes.back().unwrap();
    assert_eq!(write.component, "relation::game::settlement::Marker");
    assert_eq!(write.resolution_id, Some(resolution.id));
}
