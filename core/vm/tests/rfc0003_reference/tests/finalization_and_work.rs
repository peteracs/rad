//! Final-candidate phase ordering and intermediate-work contracts.

use super::*;

#[test]
fn final_candidate_precedes_uniqueness_and_assertion_allocation() {
    let mut transient = WorldModel::default();
    transient
        .relations
        .register(RelationSchema::new(
            "Temporary",
            vec![entity_column("entity").cascade()],
        ))
        .unwrap();
    let doomed = transient.entities.spawn().unwrap();
    transient.relations.next_assertion_id = u64::MAX;
    transient
        .apply_transaction(Transaction {
            despawns: vec![despawn(doomed, "despawn.temporary")],
            operations: vec![insert(
                pending_key("Temporary", vec![existing(doomed)]),
                "insert.temporary",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert!(transient.relations.assertions.is_empty());
    assert_eq!(transient.relations.next_assertion_id, u64::MAX);

    let mut surviving = WorldModel::default();
    surviving
        .relations
        .register(RelationSchema::new("Marker", vec![int_column("value")]))
        .unwrap();
    surviving.relations.next_assertion_id = u64::MAX;
    let before = surviving.clone();
    assert_eq!(
        surviving.apply_transaction(Transaction {
            operations: vec![insert(pending_key("Marker", vec![int(1)]), "survives")],
            ..Transaction::default()
        }),
        Err("relation.assertion_id_overflow")
    );
    assert_eq!(surviving, before);

    let owns_model = |reverse_owner_ids: bool| {
        let mut model = WorldModel::default();
        model
            .relations
            .register(
                RelationSchema::new(
                    "Owns",
                    vec![entity_column("owner"), entity_column("item").cascade()],
                )
                .unique("item", &[1]),
            )
            .unwrap();
        let first_owner = model.entities.spawn().unwrap();
        let second_owner = model.entities.spawn().unwrap();
        let (alice, bob) = if reverse_owner_ids {
            (second_owner, first_owner)
        } else {
            (first_owner, second_owner)
        };
        let sword = model.entities.spawn().unwrap();
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("Owns", vec![existing(alice), existing(sword)]),
                    "owns.alice",
                )],
                ..Transaction::default()
            })
            .unwrap();
        (model, alice, bob, sword)
    };
    for reverse in [false, true] {
        let (mut model, _, bob, sword) = owns_model(reverse);
        model
            .apply_transaction(Transaction {
                despawns: vec![despawn(sword, "despawn.sword")],
                operations: vec![insert(
                    pending_key("Owns", vec![existing(bob), existing(sword)]),
                    "owns.bob",
                )],
                ..Transaction::default()
            })
            .unwrap();
        assert!(model.relations.assertions.is_empty());
    }

    let mut one_cascades = WorldModel::default();
    one_cascades
        .relations
        .register(
            RelationSchema::new(
                "Holds",
                vec![entity_column("owner"), entity_column("item").cascade()],
            )
            .unique("owner", &[0]),
        )
        .unwrap();
    let owner = one_cascades.entities.spawn().unwrap();
    let retained_item = one_cascades.entities.spawn().unwrap();
    let doomed_item = one_cascades.entities.spawn().unwrap();
    one_cascades
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Holds", vec![existing(owner), existing(retained_item)]),
                "base.holds",
            )],
            ..Transaction::default()
        })
        .unwrap();
    let base = one_cascades.clone();
    one_cascades
        .apply_transaction(Transaction {
            despawns: vec![despawn(doomed_item, "despawn.new.item")],
            operations: vec![insert(
                pending_key("Holds", vec![existing(owner), existing(doomed_item)]),
                "candidate.holds",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(one_cascades.relations.assertions, base.relations.assertions);
    let mut conflicting = base.clone();
    assert_eq!(
        conflicting.apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Holds", vec![existing(owner), existing(doomed_item)]),
                "candidate.holds",
            )],
            ..Transaction::default()
        }),
        Err("relation.unique_conflict")
    );
    assert_eq!(conflicting, base);
}

#[test]
fn intermediate_work_limits_cover_no_match_text_groups_and_proof_products() {
    const WIDTH: i64 = 32;
    const JOIN_ATTEMPTS: usize = 1_056;
    const MATCH_ALL_BINDINGS: usize = 1_024;
    let mut model = WorldModel::default();
    for relation in ["A", "B", "Numbers"] {
        model
            .relations
            .register(RelationSchema::new(relation, vec![int_column("value")]))
            .unwrap();
    }
    for relation in ["TextSource", "TextMirror"] {
        model
            .relations
            .register(RelationSchema::new(
                relation,
                vec![ColumnSchema::new("value", ValueKind::Text)],
            ))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: (0..WIDTH)
                .map(|value| insert(pending_key("A", vec![int(value)]), "a"))
                .chain(
                    (WIDTH..(2 * WIDTH))
                        .map(|value| insert(pending_key("B", vec![int(value)]), "b")),
                )
                .chain(
                    (0..WIDTH)
                        .map(|value| insert(pending_key("Numbers", vec![int(value)]), "number")),
                )
                .chain([
                    PendingOperation::Insert(
                        pending_key("TextSource", vec![PendingValue::Text("x".repeat(8_192))]),
                        OperationMetadata::cause("text"),
                    ),
                    PendingOperation::Insert(
                        pending_key("TextMirror", vec![PendingValue::Text("x".repeat(8_192))]),
                        OperationMetadata::cause("text.mirror"),
                    ),
                ])
                .collect(),
            ..Transaction::default()
        })
        .unwrap();
    let schemas = BTreeMap::from([
        (
            "NoMatch".to_owned(),
            RelationSchema::new("NoMatch", vec![int_column("value")]),
        ),
        (
            "Pairs".to_owned(),
            RelationSchema::new("Pairs", vec![int_column("left"), int_column("right")]),
        ),
        (
            "TextOut".to_owned(),
            RelationSchema::new("TextOut", vec![ColumnSchema::new("value", ValueKind::Text)]),
        ),
        (
            "CountByValue".to_owned(),
            RelationSchema::new(
                "CountByValue",
                vec![int_column("value"), count_column("count")],
            ),
        ),
    ]);
    let no_match = RulePlan {
        id: "derive.NoMatch".to_owned(),
        head_relation: "NoMatch".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![
            Atom::new("A", vec![Term::var("value")]),
            Atom::new("B", vec![Term::var("value")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut exact = DerivationLimits::generous();
    exact.max_join_attempts = JOIN_ATTEMPTS;
    exact.max_rows_scanned = JOIN_ATTEMPTS;
    exact.max_intermediate_states = WIDTH as usize + 1;
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&no_match),
        exact,
    )
    .unwrap()
    .is_empty());
    for (mut limits, error) in [
        (
            {
                let mut value = exact;
                value.max_join_attempts = JOIN_ATTEMPTS - 1;
                value
            },
            "derivation.join_attempt_limit",
        ),
        (
            {
                let mut value = exact;
                value.max_rows_scanned = JOIN_ATTEMPTS - 1;
                value
            },
            "derivation.rows_scanned_limit",
        ),
        (
            {
                let mut value = exact;
                value.max_intermediate_states = WIDTH as usize;
                value
            },
            "derivation.intermediate_state_limit",
        ),
    ] {
        // Isolate overlapping bounds so the asserted meter wins canonically.
        if error == "derivation.join_attempt_limit" {
            limits.max_rows_scanned = usize::MAX;
        } else if error == "derivation.rows_scanned_limit" {
            limits.max_join_attempts = usize::MAX;
        }
        assert_eq!(
            derive_all(
                &model.relations,
                &schemas,
                std::slice::from_ref(&no_match),
                limits,
            ),
            Err(error)
        );
    }

    let match_all = RulePlan {
        id: "derive.Pairs".to_owned(),
        head_relation: "Pairs".to_owned(),
        head: vec![Term::var("left"), Term::var("right")],
        atoms: vec![
            Atom::new("A", vec![Term::var("left")]),
            Atom::new("B", vec![Term::var("right")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut match_limit = DerivationLimits::generous();
    match_limit.max_bindings = MATCH_ALL_BINDINGS;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            std::slice::from_ref(&match_all),
            match_limit,
        )
        .unwrap()
        .len(),
        MATCH_ALL_BINDINGS
    );
    match_limit.max_bindings = MATCH_ALL_BINDINGS - 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &[match_all], match_limit),
        Err("derivation.binding_limit")
    );

    let text_rule = RulePlan {
        id: "derive.TextOut".to_owned(),
        head_relation: "TextOut".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![
            Atom::new("TextSource", vec![Term::var("value")]),
            Atom::new("TextMirror", vec![Term::var("value")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut low = 0_usize;
    let mut high = DerivationLimits::generous().max_intermediate_bytes;
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        let mut limits = DerivationLimits::generous();
        limits.max_intermediate_bytes = middle;
        if derive_all(
            &model.relations,
            &schemas,
            std::slice::from_ref(&text_rule),
            limits,
        )
        .is_ok()
        {
            high = middle;
        } else {
            low = middle;
        }
    }
    let mut text_limit = DerivationLimits::generous();
    text_limit.max_intermediate_bytes = high;
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&text_rule),
        text_limit,
    )
    .is_ok());
    text_limit.max_intermediate_bytes = high - 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &[text_rule], text_limit),
        Err("derivation.intermediate_byte_limit")
    );

    let grouped = RulePlan {
        id: "derive.CountByValue".to_owned(),
        head_relation: "CountByValue".to_owned(),
        head: vec![Term::var("value"), Term::var("count")],
        atoms: vec![Atom::new("Numbers", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input: None,
            output: "count".to_owned(),
            group_by: vec!["value".to_owned()],
        }),
    };
    let mut group_limit = DerivationLimits::generous();
    group_limit.max_aggregate_group_entries = WIDTH as usize;
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&grouped),
        group_limit,
    )
    .is_ok());
    group_limit.max_aggregate_group_entries = WIDTH as usize - 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &[grouped], group_limit),
        Err("derivation.aggregate_group_limit")
    );

    let mut incremental_model = WorldModel::default();
    for relation in ["A", "B"] {
        incremental_model
            .relations
            .register(RelationSchema::new(relation, vec![int_column("value")]))
            .unwrap();
    }
    incremental_model
        .apply_transaction(Transaction {
            operations: (0..WIDTH)
                .map(|value| insert(pending_key("A", vec![int(value)]), "a"))
                .collect(),
            ..Transaction::default()
        })
        .unwrap();
    let mut work_limit = DerivationLimits::generous();
    work_limit.max_join_attempts = 64;
    let mut incremental = AffectedRelationProjectionHarness::new(
        incremental_model.clone(),
        schemas.clone(),
        vec![no_match.clone()],
        work_limit,
    )
    .unwrap();
    let overflow = Transaction {
        operations: vec![
            insert(pending_key("B", vec![int(10)]), "b10"),
            insert(pending_key("B", vec![int(11)]), "b11"),
        ],
        ..Transaction::default()
    };
    let mut full_candidate = incremental_model.clone();
    full_candidate.apply_transaction(overflow.clone()).unwrap();
    let full_error = derive_all(&full_candidate.relations, &schemas, &[no_match], work_limit);
    assert_eq!(full_error, Err("derivation.join_attempt_limit"));
    assert_eq!(incremental.apply(overflow), full_error.map(|_| ()));
    assert_eq!(incremental.model, incremental_model);
}
