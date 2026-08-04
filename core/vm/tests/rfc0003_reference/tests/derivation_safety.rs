//! Capability safety plus aggregate and derivation resource boundaries.

use super::*;

#[test]
fn hidden_proof_branches_do_not_change_transitive_or_aggregate_public_bytes() {
    let mut model = WorldModel::default();
    for source in [
        "VisibleSourceA",
        "VisibleSourceB",
        "HiddenSourceA",
        "HiddenSourceB",
        "JoinSource",
    ] {
        model
            .relations
            .register(RelationSchema::new(source, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                PendingOperation::Insert(
                    pending_key("VisibleSourceA", vec![int(1)]),
                    OperationMetadata::cause("visible.a"),
                ),
                PendingOperation::Insert(
                    pending_key("VisibleSourceB", vec![int(1)]),
                    OperationMetadata::cause("visible.b"),
                ),
                PendingOperation::Insert(
                    pending_key("HiddenSourceA", vec![int(1)]),
                    OperationMetadata::cause("hidden.a").with_capability("secret.a"),
                ),
                PendingOperation::Insert(
                    pending_key("HiddenSourceB", vec![int(1)]),
                    OperationMetadata::cause("hidden.b").with_capability("secret.b"),
                ),
                insert(pending_key("JoinSource", vec![int(1)]), "join.visible"),
            ],
            ..Transaction::default()
        })
        .unwrap();

    let mut schemas = derived_schemas();
    for schema in [
        RelationSchema::new("Public", vec![int_column("value")]),
        RelationSchema::new("Joined", vec![int_column("value")]),
        RelationSchema::new("CountMarked", vec![count_column("count")]),
        RelationSchema::new("SumMarked", vec![int_column("sum")]),
        RelationSchema::new("MinMarked", vec![int_column("min")]),
        RelationSchema::new("MaxMarked", vec![int_column("max")]),
    ] {
        schemas.insert(schema.name.clone(), schema);
    }
    let mut rules = [
        "VisibleSourceA",
        "VisibleSourceB",
        "HiddenSourceA",
        "HiddenSourceB",
    ]
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
    rules.extend([
        RulePlan {
            id: "derive.Public".to_owned(),
            head_relation: "Public".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: None,
        },
        RulePlan {
            id: "derive.Joined".to_owned(),
            head_relation: "Joined".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![
                Atom::new("Marked", vec![Term::var("value")]),
                Atom::new("JoinSource", vec![Term::var("value")]),
            ],
            predicates: Vec::new(),
            aggregate: None,
        },
    ]);
    for (relation, output, kind) in [
        ("CountMarked", "count", AggregateKind::Count),
        ("SumMarked", "sum", AggregateKind::Sum),
        ("MinMarked", "min", AggregateKind::Min),
        ("MaxMarked", "max", AggregateKind::Max),
    ] {
        rules.push(RulePlan {
            id: format!("derive.{relation}"),
            head_relation: relation.to_owned(),
            head: vec![Term::var(output)],
            atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind,
                input: (!matches!(kind, AggregateKind::Count)).then(|| "value".to_owned()),
                output: output.to_owned(),
                group_by: Vec::new(),
            }),
        });
    }

    let with_hidden = derive_all(&model.relations, &schemas, &rules, limits(256)).unwrap();
    let mut without_hidden_model = model.clone();
    without_hidden_model
        .apply_transaction(Transaction {
            operations: vec![
                remove(pending_key("HiddenSourceA", vec![int(1)]), "remove.a"),
                remove(pending_key("HiddenSourceB", vec![int(1)]), "remove.b"),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let without_hidden = derive_all(
        &without_hidden_model.relations,
        &schemas,
        &rules,
        limits(256),
    )
    .unwrap();
    assert_eq!(
        render_visible(&with_hidden, &BTreeSet::new()),
        render_visible(&without_hidden, &BTreeSet::new())
    );
    assert_eq!(
        with_hidden[&FactKey::new("CountMarked", vec![FactValue::Count(1)])].len(),
        4,
        "one logical binding contributes once while provenance branches remain separate"
    );
    assert_ne!(
        render_visible(
            &with_hidden,
            &BTreeSet::from(["secret.a".to_owned(), "secret.b".to_owned()]),
        ),
        render_visible(&with_hidden, &BTreeSet::new())
    );
}

#[test]
fn aggregate_checker_rejects_nongrouped_projection_and_invalid_shapes() {
    let (model, _, _, _) = seed_ownership_model();
    let mut schemas = derived_schemas();
    for schema in [
        RelationSchema::new(
            "BadProjection",
            vec![
                entity_column("person"),
                entity_column("item"),
                count_column("count"),
            ],
        ),
        RelationSchema::new(
            "CountByPerson",
            vec![entity_column("person"), count_column("count")],
        ),
        RelationSchema::new(
            "BadBoundOutput",
            vec![entity_column("person"), count_column("item")],
        ),
        RelationSchema::new(
            "SumByPerson",
            vec![entity_column("person"), int_column("sum")],
        ),
        RelationSchema::new(
            "WrongOutput",
            vec![entity_column("person"), count_column("sum")],
        ),
    ] {
        schemas.insert(schema.name.clone(), schema);
    }
    let count_rule = |head_relation: &str, head: Vec<Term>, group_by: Vec<&str>, input| RulePlan {
        id: format!("derive.{head_relation}"),
        head_relation: head_relation.to_owned(),
        head,
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::var("person"), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input,
            output: "count".to_owned(),
            group_by: group_by.into_iter().map(str::to_owned).collect(),
        }),
    };
    assert_eq!(
        count_rule(
            "BadProjection",
            vec![Term::var("person"), Term::var("item"), Term::var("count")],
            vec!["person"],
            None,
        )
        .validate(&model.relations, &schemas),
        Err("derivation.aggregate_head_projection")
    );
    let bound_output = RulePlan {
        id: "derive.BadBoundOutput".to_owned(),
        head_relation: "BadBoundOutput".to_owned(),
        head: vec![Term::var("person"), Term::var("item")],
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::var("person"), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input: None,
            output: "item".to_owned(),
            group_by: vec!["person".to_owned()],
        }),
    };
    assert_eq!(
        bound_output.validate(&model.relations, &schemas),
        Err("derivation.aggregate_output_not_fresh")
    );
    assert_eq!(
        count_rule(
            "CountByPerson",
            vec![Term::var("person"), Term::var("count")],
            vec!["person", "person"],
            None,
        )
        .validate(&model.relations, &schemas),
        Err("derivation.duplicate_group")
    );
    assert_eq!(
        count_rule(
            "CountByPerson",
            vec![Term::var("person"), Term::var("count")],
            vec!["person"],
            Some("item".to_owned()),
        )
        .validate(&model.relations, &schemas),
        Err("derivation.count_input")
    );

    let invalid_sum = |head_relation: &str, input: &str| RulePlan {
        id: format!("derive.{head_relation}"),
        head_relation: head_relation.to_owned(),
        head: vec![Term::var("person"), Term::var("sum")],
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::var("person"), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Sum,
            input: Some(input.to_owned()),
            output: "sum".to_owned(),
            group_by: vec!["person".to_owned()],
        }),
    };
    assert_eq!(
        invalid_sum("SumByPerson", "item").validate(&model.relations, &schemas),
        Err("derivation.aggregate_type")
    );

    let wrong_output = RulePlan {
        id: "derive.WrongOutput".to_owned(),
        head_relation: "WrongOutput".to_owned(),
        head: vec![Term::var("person"), Term::var("sum")],
        atoms: vec![
            Atom::new("Owns", vec![Term::var("person"), Term::var("item")]),
            Atom::new("ItemWeight", vec![Term::var("item"), Term::var("weight")]),
        ],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Sum,
            input: Some("weight".to_owned()),
            output: "sum".to_owned(),
            group_by: vec!["person".to_owned()],
        }),
    };
    assert_eq!(
        wrong_output.validate(&model.relations, &schemas),
        Err("derivation.head_type")
    );

    let bad_atom_arity = RulePlan {
        id: "derive.BadArity".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::Constant(FactValue::Int(1))],
        atoms: vec![Atom::new("Owns", vec![Term::var("person")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        bad_atom_arity.validate(&model.relations, &schemas),
        Err("derivation.atom_arity")
    );
    let bad_atom_type = RulePlan {
        id: "derive.BadType".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::Constant(FactValue::Int(1))],
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::Constant(FactValue::Int(1)), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        bad_atom_type.validate(&model.relations, &schemas),
        Err("derivation.atom_type")
    );

    let incompatible_heads = vec![
        RulePlan {
            id: "derive.Marked.weight".to_owned(),
            head_relation: "Marked".to_owned(),
            head: vec![Term::var("weight")],
            atoms: vec![Atom::new(
                "ItemWeight",
                vec![Term::var("item"), Term::var("weight")],
            )],
            predicates: Vec::new(),
            aggregate: None,
        },
        RulePlan {
            id: "derive.Marked.person".to_owned(),
            head_relation: "Marked".to_owned(),
            head: vec![Term::var("person")],
            atoms: vec![Atom::new(
                "Owns",
                vec![Term::var("person"), Term::var("item")],
            )],
            predicates: Vec::new(),
            aggregate: None,
        },
    ];
    assert_eq!(
        derive_all(&model.relations, &schemas, &incompatible_heads, limits(32)),
        Err("derivation.head_type")
    );
}

#[test]
fn derivation_limits_bound_proofs_depth_supports_capabilities_and_bytes_atomically() {
    let mut model = WorldModel::default();
    for source in ["SourceA", "SourceB", "SourceC", "SourceD"] {
        model
            .relations
            .register(RelationSchema::new(source, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: ["SourceA", "SourceB", "SourceC", "SourceD"]
                .into_iter()
                .map(|source| {
                    PendingOperation::Insert(
                        pending_key(source, vec![int(1)]),
                        OperationMetadata::cause(source).with_capability(&format!("read.{source}")),
                    )
                })
                .collect(),
            ..Transaction::default()
        })
        .unwrap();
    let mut schemas = derived_schemas();
    for schema in [
        RelationSchema::new("D1", vec![int_column("value")]),
        RelationSchema::new("D2", vec![int_column("value")]),
        RelationSchema::new("D3", vec![int_column("value")]),
        RelationSchema::new("Left", vec![int_column("value")]),
        RelationSchema::new("Right", vec![int_column("value")]),
        RelationSchema::new("Combined", vec![int_column("value")]),
        RelationSchema::new("CountBranches", vec![count_column("count")]),
    ] {
        schemas.insert(schema.name.clone(), schema);
    }
    let source_rule = |source: &str, head: &str| RulePlan {
        id: format!("derive.{head}.{source}"),
        head_relation: head.to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new(source, vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    let marked_rules = ["SourceA", "SourceB", "SourceC"]
        .into_iter()
        .map(|source| source_rule(source, "Marked"))
        .collect::<Vec<_>>();

    let bounded_result = derive_all(
        &model.relations,
        &schemas,
        &marked_rules[..1],
        DerivationLimits::generous(),
    )
    .unwrap();
    let exact_canonical_bytes = canonical_derivation_bytes(&bounded_result).len();
    let mut exact_byte_limit = DerivationLimits::generous();
    exact_byte_limit.max_canonical_bytes = exact_canonical_bytes;
    assert!(derive_all(
        &model.relations,
        &schemas,
        &marked_rules[..1],
        exact_byte_limit,
    )
    .is_ok());
    exact_byte_limit.max_canonical_bytes -= 1;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &marked_rules[..1],
            exact_byte_limit,
        ),
        Err("derivation.canonical_byte_limit")
    );

    let mut proof_limit = DerivationLimits::generous();
    proof_limit.max_proofs_per_fact = 2;
    assert!(derive_all(&model.relations, &schemas, &marked_rules[..2], proof_limit,).is_ok());
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules, proof_limit),
        Err("derivation.proofs_per_fact_limit")
    );

    let mut capability_limit = DerivationLimits::generous();
    capability_limit.max_capability_alternatives = 2;
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules, capability_limit),
        Err("derivation.capability_alternative_limit")
    );
    let mut support_limit = DerivationLimits::generous();
    support_limit.max_support_nodes = 0;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &marked_rules[..1],
            support_limit
        ),
        Err("derivation.support_limit")
    );
    let mut byte_limit = DerivationLimits::generous();
    byte_limit.max_canonical_bytes = 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules[..1], byte_limit),
        Err("derivation.canonical_byte_limit")
    );
    let mut fact_limit = DerivationLimits::generous();
    fact_limit.max_facts = 0;
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules[..1], fact_limit),
        Err("derivation.fact_limit")
    );
    let mut total_proof_limit = DerivationLimits::generous();
    total_proof_limit.max_total_proofs = 0;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &marked_rules[..1],
            total_proof_limit,
        ),
        Err("derivation.total_proof_limit")
    );

    let depth_rules = vec![
        source_rule("SourceA", "D1"),
        source_rule("D1", "D2"),
        source_rule("D2", "D3"),
    ];
    let mut depth_limit = DerivationLimits::generous();
    depth_limit.max_proof_depth = 2;
    assert_eq!(
        derive_all(&model.relations, &schemas, &depth_rules, depth_limit),
        Err("derivation.depth_limit")
    );

    let branch_rules = vec![
        source_rule("SourceA", "Left"),
        source_rule("SourceB", "Left"),
        source_rule("SourceC", "Right"),
        source_rule("SourceD", "Right"),
        RulePlan {
            id: "derive.Combined".to_owned(),
            head_relation: "Combined".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![
                Atom::new("Left", vec![Term::var("value")]),
                Atom::new("Right", vec![Term::var("value")]),
            ],
            predicates: Vec::new(),
            aggregate: None,
        },
    ];
    let mut branch_limit = DerivationLimits::generous();
    branch_limit.max_capability_alternatives = 3;
    assert_eq!(
        derive_all(&model.relations, &schemas, &branch_rules, branch_limit),
        Err("derivation.capability_alternative_limit")
    );

    let aggregate_rules = marked_rules
        .iter()
        .cloned()
        .chain([RulePlan {
            id: "derive.CountBranches".to_owned(),
            head_relation: "CountBranches".to_owned(),
            head: vec![Term::var("count")],
            atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind: AggregateKind::Count,
                input: None,
                output: "count".to_owned(),
                group_by: Vec::new(),
            }),
        }])
        .collect::<Vec<_>>();
    let mut aggregate_branch_limit = DerivationLimits::generous();
    aggregate_branch_limit.max_proof_combination_attempts = 3;
    assert!(derive_all(
        &model.relations,
        &schemas,
        &aggregate_rules,
        aggregate_branch_limit,
    )
    .is_ok());
    aggregate_branch_limit.max_proof_combination_attempts = 2;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &aggregate_rules,
            aggregate_branch_limit,
        ),
        Err("derivation.proof_combination_limit")
    );
    aggregate_branch_limit.max_proof_combination_attempts = usize::MAX;
    aggregate_branch_limit.max_capability_alternatives = 2;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &aggregate_rules,
            aggregate_branch_limit,
        ),
        Err("derivation.capability_alternative_limit")
    );

    let mut incremental_model = WorldModel::default();
    for source in ["SourceA", "SourceB", "SourceC"] {
        incremental_model
            .relations
            .register(RelationSchema::new(source, vec![int_column("value")]))
            .unwrap();
    }
    incremental_model
        .apply_transaction(Transaction {
            operations: vec![insert(pending_key("SourceA", vec![int(1)]), "a")],
            ..Transaction::default()
        })
        .unwrap();
    let mut incremental = AffectedRelationProjectionHarness::new(
        incremental_model.clone(),
        schemas.clone(),
        marked_rules.clone(),
        proof_limit,
    )
    .unwrap();
    let overflow = Transaction {
        operations: vec![
            insert(pending_key("SourceB", vec![int(1)]), "b"),
            insert(pending_key("SourceC", vec![int(1)]), "c"),
        ],
        ..Transaction::default()
    };
    let mut full_candidate = incremental_model.clone();
    full_candidate.apply_transaction(overflow.clone()).unwrap();
    let full_error = derive_all(
        &full_candidate.relations,
        &schemas,
        &marked_rules,
        proof_limit,
    );
    assert_eq!(full_error, Err("derivation.proofs_per_fact_limit"));
    assert_eq!(incremental.apply(overflow), full_error.map(|_| ()));
    assert_eq!(incremental.model, incremental_model);
}

#[test]
fn aggregates_require_positive_input_and_empty_scans_produce_no_row() {
    let mut model = WorldModel::default();
    model
        .relations
        .register(RelationSchema::new("Numbers", vec![int_column("value")]))
        .unwrap();
    let schemas = BTreeMap::from([
        (
            "GlobalCount".to_owned(),
            RelationSchema::new("GlobalCount", vec![count_column("count")]),
        ),
        (
            "GlobalInt".to_owned(),
            RelationSchema::new("GlobalInt", vec![int_column("value")]),
        ),
    ]);
    for kind in [
        AggregateKind::Count,
        AggregateKind::Sum,
        AggregateKind::Min,
        AggregateKind::Max,
    ] {
        let (head_relation, output, input) = if matches!(kind, AggregateKind::Count) {
            ("GlobalCount", "count", None)
        } else {
            ("GlobalInt", "value", Some("input".to_owned()))
        };
        let atomless = RulePlan {
            id: format!("derive.atomless.{kind:?}"),
            head_relation: head_relation.to_owned(),
            head: vec![Term::var(output)],
            atoms: Vec::new(),
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind,
                input,
                output: output.to_owned(),
                group_by: Vec::new(),
            }),
        };
        assert_eq!(
            atomless.validate(&model.relations, &schemas),
            Err("derivation.aggregate_requires_positive_input")
        );
    }

    let count = RulePlan {
        id: "derive.GlobalCount".to_owned(),
        head_relation: "GlobalCount".to_owned(),
        head: vec![Term::var("count")],
        atoms: vec![Atom::new("Numbers", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input: None,
            output: "count".to_owned(),
            group_by: Vec::new(),
        }),
    };
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&count),
        DerivationLimits::generous(),
    )
    .unwrap()
    .is_empty());
    model
        .apply_transaction(Transaction {
            operations: vec![insert(pending_key("Numbers", vec![int(7)]), "number")],
            ..Transaction::default()
        })
        .unwrap();
    let derived = derive_all(
        &model.relations,
        &schemas,
        &[count],
        DerivationLimits::generous(),
    )
    .unwrap();
    assert!(derived.contains_key(&FactKey::new("GlobalCount", vec![FactValue::Count(1)],)));
}
