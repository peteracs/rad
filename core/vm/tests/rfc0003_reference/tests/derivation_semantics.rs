//! Positive nonrecursive derivation, proof, aggregate, and ancestry semantics.

use super::*;

#[test]
fn insertion_permutations_and_affected_projection_match_full_recomputation() {
    let (seed, person, item_a, item_b) = seed_ownership_model();
    let mut forward = WorldModel {
        entities: seed.entities.clone(),
        components: seed.components.clone(),
        relations: RelationStore {
            schemas: seed.relations.schemas.clone(),
            ..RelationStore::default()
        },
    };
    let operations = vec![
        insert(
            pending_key("Owns", vec![existing(person), existing(item_a)]),
            "a",
        ),
        insert(
            pending_key("Owns", vec![existing(person), existing(item_b)]),
            "b",
        ),
        insert(
            pending_key("ItemWeight", vec![existing(item_a), int(7)]),
            "c",
        ),
        insert(
            pending_key("ItemWeight", vec![existing(item_b), int(6)]),
            "d",
        ),
        insert(
            pending_key("CarryCapacity", vec![existing(person), int(10)]),
            "e",
        ),
    ];
    forward
        .apply_transaction(Transaction {
            operations: operations.clone(),
            ..Transaction::default()
        })
        .unwrap();
    let mut reverse = WorldModel {
        entities: seed.entities.clone(),
        components: seed.components.clone(),
        relations: RelationStore {
            schemas: seed.relations.schemas.clone(),
            ..RelationStore::default()
        },
    };
    reverse
        .apply_transaction(Transaction {
            operations: operations.into_iter().rev().collect(),
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(forward.relations.assertions, reverse.relations.assertions);
    let schemas = derived_schemas();
    let rules = ownership_rules();
    let limits = limits(128);
    assert_eq!(
        derive_all(&forward.relations, &schemas, &rules, limits).unwrap(),
        derive_all(&reverse.relations, &schemas, &rules, limits).unwrap()
    );

    let mut projection =
        AffectedRelationProjectionHarness::new(forward, schemas, rules, limits).unwrap();
    for transaction in [
        Transaction {
            operations: vec![remove(
                pending_key("ItemWeight", vec![existing(item_b), int(6)]),
                "delta.remove_weight",
            )],
            ..Transaction::default()
        },
        Transaction {
            operations: vec![insert(
                pending_key("ItemWeight", vec![existing(item_b), int(1)]),
                "delta.insert_weight",
            )],
            ..Transaction::default()
        },
        Transaction {
            operations: vec![remove(
                pending_key("Owns", vec![existing(person), existing(item_a)]),
                "delta.remove_owns",
            )],
            ..Transaction::default()
        },
    ] {
        projection.apply(transaction).unwrap();
        assert_eq!(
            projection.derived,
            derive_all(
                &projection.model.relations,
                &projection.derived_schemas,
                &projection.rules,
                limits,
            )
            .unwrap()
        );
    }
}

#[test]
fn alternative_proofs_are_unioned_and_final_support_removal_retracts_fact() {
    let mut model = WorldModel::default();
    for name in ["MarkerA", "MarkerB"] {
        model
            .relations
            .register(RelationSchema::new(name, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(pending_key("MarkerA", vec![int(7)]), "source.a"),
                insert(pending_key("MarkerB", vec![int(7)]), "source.b"),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let rules = ["MarkerA", "MarkerB"]
        .into_iter()
        .map(|source| RulePlan {
            id: format!("derive.Marked.{source}"),
            head_relation: "Marked".to_owned(),
            head: vec![Term::Constant(FactValue::Int(7))],
            atoms: vec![Atom::new(source, vec![Term::Constant(FactValue::Int(7))])],
            predicates: Vec::new(),
            aggregate: None,
        })
        .collect::<Vec<_>>();
    let schemas = derived_schemas();
    let limits = limits(32);
    let marked = FactKey::new("Marked", vec![FactValue::Int(7)]);
    let first = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    assert_eq!(first[&marked].len(), 2);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(pending_key("MarkerA", vec![int(7)]), "remove.a")],
            ..Transaction::default()
        })
        .unwrap();
    let second = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    assert_eq!(second[&marked].len(), 1);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(pending_key("MarkerB", vec![int(7)]), "remove.b")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(!derive_all(&model.relations, &schemas, &rules, limits)
        .unwrap()
        .contains_key(&marked));
}

#[test]
fn aggregate_contract_is_checked_exact_and_bounded() {
    assert_eq!(aggregate_values(AggregateKind::Sum, &[]), Ok(None));
    assert_eq!(
        aggregate_values(
            AggregateKind::Sum,
            &[FactValue::Int(i64::MAX), FactValue::Int(1)],
        ),
        Err("derivation.sum_overflow")
    );
    assert_eq!(
        aggregate_values(AggregateKind::Min, &[FactValue::Int(9), FactValue::Int(-2)],),
        Ok(Some(FactValue::Int(-2)))
    );
    assert_eq!(
        aggregate_values(AggregateKind::Max, &[FactValue::Int(9), FactValue::Int(-2)],),
        Ok(Some(FactValue::Int(9)))
    );
    assert_eq!(checked_count(u64::MAX, 1), Err("derivation.count_overflow"));

    let (model, _, _, _) = seed_ownership_model();
    assert_eq!(
        derive_all(
            &model.relations,
            &derived_schemas(),
            &ownership_rules(),
            limits(1),
        ),
        Err("derivation.binding_limit")
    );
}

#[test]
fn rule_plans_are_range_restricted_and_cycles_fail_closed() {
    let unsafe_rule = RulePlan {
        id: "derive.Unsafe".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::var("unbound")],
        atoms: Vec::new(),
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        unsafe_rule.validate(&RelationStore::default(), &derived_schemas()),
        Err("derivation.unbound_variable")
    );
    let cycle = RulePlan {
        id: "derive.Cycle".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        derive_all(
            &RelationStore::default(),
            &derived_schemas(),
            &[cycle],
            limits(8),
        ),
        Err("derivation.cycle")
    );
}

#[test]
fn assertion_lifetimes_preserve_noop_ancestry_and_reinsert_gets_new_ancestry() {
    let (mut model, owner, item, _) = seed_ownership_model();
    let key = FactKey::new(
        "Owns",
        vec![FactValue::Entity(owner), FactValue::Entity(item)],
    );
    let first = model.relations.assertions[&key].clone();
    let semantic_before = fact_key_bytes(&key);
    let operational_before = operational_checkpoint_bytes(&model);
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "ignored.noop",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(model.relations.assertions[&key], first);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "settlement.remove",
            )],
            ..Transaction::default()
        })
        .unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "settlement.reinsert",
            )],
            ..Transaction::default()
        })
        .unwrap();
    let second = &model.relations.assertions[&key];
    assert_ne!(second.id, first.id);
    assert_eq!(
        second.causes,
        BTreeSet::from(["settlement.reinsert".to_owned()])
    );
    assert_eq!(fact_key_bytes(&key), semantic_before);
    assert_ne!(operational_checkpoint_bytes(&model), operational_before);
}

#[test]
fn why_chain_reaches_exact_assertion_versions_and_settlement_causes() {
    let (model, person, item_a, item_b) = seed_ownership_model();
    let derived = derive_all(
        &model.relations,
        &derived_schemas(),
        &ownership_rules(),
        limits(128),
    )
    .unwrap();
    let encumbered = FactKey::new("Encumbered", vec![FactValue::Entity(person)]);
    let encumbered_proof = derived[&encumbered].iter().next().unwrap();
    let total_support = encumbered_proof
        .supports
        .iter()
        .find(|support| support.key().relation == "TotalWeight")
        .unwrap();
    let total_proof_id = match total_support {
        SupportRef::Derived { proof_id, .. } => proof_id,
        SupportRef::Authoritative { .. } => panic!("TotalWeight must be derived"),
    };
    let total = total_support.key();
    assert_eq!(
        total.tuple,
        vec![FactValue::Entity(person), FactValue::Int(13)]
    );
    assert_eq!(
        total_proof_id,
        &derived[total].iter().next().unwrap().identity()
    );
    let total_proof = derived[total].iter().next().unwrap();
    for expected in [
        FactKey::new(
            "Owns",
            vec![FactValue::Entity(person), FactValue::Entity(item_a)],
        ),
        FactKey::new(
            "Owns",
            vec![FactValue::Entity(person), FactValue::Entity(item_b)],
        ),
        FactKey::new(
            "ItemWeight",
            vec![FactValue::Entity(item_a), FactValue::Int(7)],
        ),
        FactKey::new(
            "ItemWeight",
            vec![FactValue::Entity(item_b), FactValue::Int(6)],
        ),
    ] {
        let support = total_proof
            .supports
            .iter()
            .find(|support| support.key() == &expected)
            .unwrap();
        let SupportRef::Authoritative { assertion_id, .. } = support else {
            panic!("base support must be authoritative");
        };
        let assertion = &model.relations.assertions[&expected];
        assert_eq!(*assertion_id, assertion.id);
        assert!(!assertion.causes.is_empty());
    }
}

#[test]
fn capability_filtering_hides_proof_multiplicity_and_order() {
    let mut model = WorldModel::default();
    for name in ["VisibleSource", "HiddenSource"] {
        model
            .relations
            .register(RelationSchema::new(name, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                PendingOperation::Insert(
                    pending_key("VisibleSource", vec![int(1)]),
                    OperationMetadata::cause("visible"),
                ),
                PendingOperation::Insert(
                    pending_key("HiddenSource", vec![int(1)]),
                    OperationMetadata::cause("hidden").with_capability("secret"),
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let rules = ["VisibleSource", "HiddenSource"]
        .into_iter()
        .map(|source| RulePlan {
            id: format!("derive.Marked.{source}"),
            head_relation: "Marked".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![Atom::new(source, vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: None,
        })
        .collect::<Vec<_>>();
    let derived = derive_all(&model.relations, &derived_schemas(), &rules, limits(16)).unwrap();
    let public = render_visible(&derived, &BTreeSet::new());
    let marked = FactKey::new("Marked", vec![FactValue::Int(1)]);
    let mut without_hidden = derived.clone();
    without_hidden
        .get_mut(&marked)
        .unwrap()
        .retain(|proof| proof.required_capabilities.is_empty());
    assert_eq!(public, render_visible(&without_hidden, &BTreeSet::new()));
    let privileged = render_visible(&derived, &BTreeSet::from(["secret".to_owned()]));
    assert_ne!(public, privileged);
}
