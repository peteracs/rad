//! Wire, operational identity, replay, and simultaneous-deletion contracts.

use super::*;

#[test]
fn semantic_wire_round_trips_while_portable_replay_binds_assertion_identity() {
    let (mut model, owner, item, _) = seed_ownership_model();
    let key = FactKey::new(
        "Owns",
        vec![FactValue::Entity(owner), FactValue::Entity(item)],
    );
    assert_eq!(decode_fact_key(&fact_key_bytes(&key)).unwrap(), key);
    let generic_values = pending_key(
        "GenericValues",
        vec![PendingValue::Count(3), PendingValue::Text("tag".to_owned())],
    )
    .resolve(&BTreeMap::new())
    .unwrap();
    assert_eq!(
        decode_fact_key(&fact_key_bytes(&generic_values)).unwrap(),
        generic_values
    );
    let semantic_before = semantic_relation_bytes(&model.relations);
    assert_eq!(
        decode_semantic_relation_bytes(
            &semantic_before,
            &model.relations.schemas,
            &model.entities,
            DecodeLimits::generous(),
        )
        .unwrap(),
        model.relations.assertions.keys().cloned().collect()
    );
    let schemas = derived_schemas();
    let rules = ownership_rules();
    let limits = limits(128);
    let derived_before = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    let attempt = PortableAttempt {
        checkpoint: portable_checkpoint_bytes(&model, &derived_before),
    };
    model
        .apply_transaction(Transaction {
            operations: vec![remove(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "remove",
            )],
            ..Transaction::default()
        })
        .unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "reinsert",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(semantic_relation_bytes(&model.relations), semantic_before);
    let derived_after = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    assert_ne!(
        derivation_checkpoint_bytes(&derived_before),
        derivation_checkpoint_bytes(&derived_after)
    );
    let mut instructions = 0;
    assert_eq!(
        replay_portable(&model, &derived_after, &attempt, &mut instructions),
        Err("attempt.checkpoint_mismatch")
    );
    assert_eq!(instructions, 0);
}

#[test]
fn simultaneous_despawns_are_classified_as_one_set_before_any_cascade() {
    fn link_model(reverse_columns: bool) -> (WorldModel, EntityRef, EntityRef, EntityRef) {
        let mut model = WorldModel::default();
        let columns = if reverse_columns {
            vec![entity_column("target"), entity_column("source").cascade()]
        } else {
            vec![entity_column("source").cascade(), entity_column("target")]
        };
        model
            .relations
            .register(
                RelationSchema::new("Link", columns)
                    .unique("source", &[usize::from(reverse_columns)]),
            )
            .unwrap();
        let a = model.entities.spawn().unwrap();
        let b = model.entities.spawn().unwrap();
        let c = model.entities.spawn().unwrap();
        let tuple = if reverse_columns {
            vec![existing(b), existing(a)]
        } else {
            vec![existing(a), existing(b)]
        };
        model
            .apply_transaction(Transaction {
                operations: vec![insert(pending_key("Link", tuple), "link.insert")],
                ..Transaction::default()
            })
            .unwrap();
        (model, a, b, c)
    }

    for reverse_columns in [false, true] {
        let (model, a, b, _) = link_model(reverse_columns);
        for despawns in [
            vec![despawn(a, "despawn.a"), despawn(b, "despawn.b")],
            vec![despawn(b, "despawn.b"), despawn(a, "despawn.a")],
        ] {
            let mut candidate = model.clone();
            assert_eq!(
                candidate.apply_transaction(Transaction {
                    despawns,
                    ..Transaction::default()
                }),
                Err("entity.delete_restricted")
            );
            assert_eq!(candidate, model);
        }
    }

    let (mut explicit_remove, a, b, _) = link_model(false);
    explicit_remove
        .apply_transaction(Transaction {
            despawns: vec![despawn(a, "despawn.a"), despawn(b, "despawn.b")],
            operations: vec![remove(
                pending_key("Link", vec![existing(a), existing(b)]),
                "link.explicit_remove",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert!(explicit_remove.relations.assertions.is_empty());

    let (mut replaced, a, b, c) = link_model(false);
    replaced
        .apply_transaction(Transaction {
            despawns: vec![despawn(b, "despawn.old_target")],
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Link".to_owned(),
                unique_constraint: "source".to_owned(),
                selected_key: vec![existing(a)],
                tuple: vec![existing(a), existing(c)],
                metadata: OperationMetadata::cause("link.replace"),
            }],
            ..Transaction::default()
        })
        .unwrap();
    assert!(replaced.relations.assertions.contains_key(&FactKey::new(
        "Link",
        vec![FactValue::Entity(a), FactValue::Entity(c)],
    )));

    let (mut cascade, a, _, _) = link_model(false);
    cascade
        .apply_transaction(Transaction {
            despawns: vec![PendingDespawn {
                entity: a,
                metadata: OperationMetadata::cause("settlement.despawn.a")
                    .with_capability("world.delete"),
            }],
            ..Transaction::default()
        })
        .unwrap();
    let cascade_change = cascade
        .relations
        .last_changes
        .iter()
        .find(|change| change.kind == ChangeKind::Cascade)
        .unwrap();
    assert_eq!(
        cascade_change.causes,
        BTreeSet::from(["settlement.despawn.a".to_owned()])
    );
    assert_eq!(
        cascade_change.required_capabilities,
        BTreeSet::from(["world.delete".to_owned()])
    );

    let (mut same_entity, a, _, _) = link_model(false);
    same_entity
        .apply_transaction(Transaction {
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Link".to_owned(),
                unique_constraint: "source".to_owned(),
                selected_key: vec![existing(a)],
                tuple: vec![existing(a), existing(a)],
                metadata: OperationMetadata::cause("link.self"),
            }],
            ..Transaction::default()
        })
        .unwrap();
    let before = same_entity.clone();
    assert_eq!(
        same_entity.apply_transaction(Transaction {
            despawns: vec![despawn(a, "despawn.self")],
            ..Transaction::default()
        }),
        Err("entity.delete_restricted")
    );
    assert_eq!(same_entity, before);

    let mut inserted_then_despawned = WorldModel::default();
    inserted_then_despawned
        .relations
        .register(RelationSchema::new(
            "CascadeOnly",
            vec![entity_column("source").cascade(), entity_column("target")],
        ))
        .unwrap();
    let source = inserted_then_despawned.entities.spawn().unwrap();
    let target = inserted_then_despawned.entities.spawn().unwrap();
    let next_assertion = inserted_then_despawned.relations.next_assertion_id;
    inserted_then_despawned
        .apply_transaction(Transaction {
            despawns: vec![despawn(source, "despawn.inserted_source")],
            operations: vec![insert(
                pending_key("CascadeOnly", vec![existing(source), existing(target)]),
                "insert.before.cascade",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert!(inserted_then_despawned.relations.assertions.is_empty());
    assert!(inserted_then_despawned.relations.last_changes.is_empty());
    assert_eq!(
        inserted_then_despawned.relations.next_assertion_id, next_assertion,
        "a row that never commits has no assertion lifetime"
    );
}

#[test]
fn operational_state_binds_generations_assertion_allocator_and_restoration() {
    let mut generation_a = WorldModel::default();
    let first = generation_a.entities.spawn().unwrap();
    generation_a.entities.despawn(first).unwrap();
    let mut generation_b = generation_a.clone();
    let reused = generation_b.entities.spawn().unwrap();
    generation_b.entities.despawn(reused).unwrap();
    assert_eq!(generation_a.entities.live, generation_b.entities.live);
    assert_eq!(
        generation_a.entities.free_slots,
        generation_b.entities.free_slots
    );
    assert_ne!(
        operational_checkpoint_bytes(&generation_a),
        operational_checkpoint_bytes(&generation_b)
    );

    let state = OperationalRelationState::capture(&generation_a);
    let mut restored = state.restore(generation_a.relations.schemas.clone());
    let mut original = generation_a.clone();
    assert_eq!(
        restored.entities.spawn().unwrap(),
        original.entities.spawn().unwrap()
    );

    let mut assertion_a = WorldModel::default();
    assertion_a
        .relations
        .register(RelationSchema::new("Marker", vec![int_column("value")]))
        .unwrap();
    let mut assertion_b = assertion_a.clone();
    assertion_b
        .apply_transaction(Transaction {
            operations: vec![insert(pending_key("Marker", vec![int(1)]), "insert")],
            ..Transaction::default()
        })
        .unwrap();
    assertion_b
        .apply_transaction(Transaction {
            operations: vec![remove(pending_key("Marker", vec![int(1)]), "remove")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(assertion_a.relations.assertions.is_empty());
    assert!(assertion_b.relations.assertions.is_empty());
    assert_ne!(
        operational_checkpoint_bytes(&assertion_a),
        operational_checkpoint_bytes(&assertion_b)
    );
    let attempt = PortableAttempt {
        checkpoint: portable_checkpoint_bytes(&assertion_a, &DerivationResult::new()),
    };
    let mut instructions = 0;
    assert_eq!(
        replay_portable(
            &assertion_b,
            &DerivationResult::new(),
            &attempt,
            &mut instructions,
        ),
        Err("attempt.checkpoint_mismatch")
    );
    assert_eq!(instructions, 0);

    let state = OperationalRelationState::capture(&assertion_b);
    let mut restored = state.restore(assertion_b.relations.schemas.clone());
    for model in [&mut assertion_b, &mut restored] {
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("Marker", vec![int(2)]),
                    "next.assertion",
                )],
                ..Transaction::default()
            })
            .unwrap();
    }
    assert_eq!(
        assertion_b.relations.assertions,
        restored.relations.assertions
    );
}

#[test]
fn schemas_and_semantic_wire_reject_ambiguous_or_noncanonical_input() {
    assert_eq!(
        RelationSchema::new("Ambiguous", vec![int_column("value")])
            .unique("same", &[0])
            .unique("same", &[0])
            .validate_declaration(),
        Err("relation.duplicate_unique_name")
    );
    assert_eq!(
        RelationSchema::new(
            "DuplicateColumns",
            vec![int_column("value"), int_column("value")],
        )
        .validate_declaration(),
        Err("relation.duplicate_column_name")
    );
    assert_eq!(
        RelationSchema::new("DuplicateUniqueIndex", vec![int_column("value")])
            .unique("value", &[0, 0])
            .validate_declaration(),
        Err("relation.unique_shape")
    );
    assert_eq!(
        RelationSchema::new("NonEntityDelete", vec![int_column("value").cascade()])
            .validate_declaration(),
        Err("relation.delete_policy_non_entity")
    );
    assert_eq!(
        RelationSchema::new("", vec![int_column("value")]).validate_declaration(),
        Err("relation.empty_name")
    );
    assert_eq!(
        RelationSchema::new("EmptyColumn", vec![int_column("")]).validate_declaration(),
        Err("relation.empty_column_name")
    );
    assert_eq!(
        RelationSchema::new("EmptyUnique", vec![int_column("value")])
            .unique("", &[0])
            .validate_declaration(),
        Err("relation.empty_unique_name")
    );

    let a = FactKey::new("A", vec![FactValue::Int(1)]);
    let b = FactKey::new("B", vec![FactValue::Int(2)]);
    let schemas = BTreeMap::from([
        (
            "A".to_owned(),
            RelationSchema::new("A", vec![int_column("value")]),
        ),
        (
            "B".to_owned(),
            RelationSchema::new("B", vec![int_column("value")]),
        ),
        (
            "S".to_owned(),
            RelationSchema::new("S", vec![entity_column("left"), entity_column("right")])
                .symmetric(),
        ),
    ]);
    let mut duplicate = b"rfc0003.semantic.v1".to_vec();
    write_u64(&mut duplicate, 2);
    encode_fact_key(&mut duplicate, &a);
    encode_fact_key(&mut duplicate, &a);
    assert_eq!(
        decode_semantic_relation_bytes(
            &duplicate,
            &schemas,
            &EntityTable::default(),
            DecodeLimits::generous(),
        ),
        Err("wire.noncanonical_fact_order")
    );
    let mut out_of_order = b"rfc0003.semantic.v1".to_vec();
    write_u64(&mut out_of_order, 2);
    encode_fact_key(&mut out_of_order, &b);
    encode_fact_key(&mut out_of_order, &a);
    assert_eq!(
        decode_semantic_relation_bytes(
            &out_of_order,
            &schemas,
            &EntityTable::default(),
            DecodeLimits::generous(),
        ),
        Err("wire.noncanonical_fact_order")
    );
    let one_fact = |fact: &FactKey| {
        let mut bytes = b"rfc0003.semantic.v1".to_vec();
        write_u64(&mut bytes, 1);
        encode_fact_key(&mut bytes, fact);
        bytes
    };
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new("Unknown", vec![FactValue::Int(1)])),
            &schemas,
            &EntityTable::default(),
            DecodeLimits::generous(),
        ),
        Err("wire.unknown_relation")
    );
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new("A", Vec::new())),
            &schemas,
            &EntityTable::default(),
            DecodeLimits::generous(),
        ),
        Err("wire.relation_arity")
    );
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new("A", vec![FactValue::Text("1".to_owned())])),
            &schemas,
            &EntityTable::default(),
            DecodeLimits::generous(),
        ),
        Err("wire.relation_type")
    );
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new(
                "S",
                vec![
                    FactValue::Entity(EntityRef {
                        slot: 9,
                        generation: 0,
                    }),
                    FactValue::Entity(EntityRef {
                        slot: 4,
                        generation: 0,
                    }),
                ],
            )),
            &schemas,
            &EntityTable::default(),
            DecodeLimits::generous(),
        ),
        Err("wire.noncanonical_tuple")
    );
    let dead = EntityRef {
        slot: 4,
        generation: 0,
    };
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new(
                "S",
                vec![FactValue::Entity(dead), FactValue::Entity(dead)],
            )),
            &schemas,
            &EntityTable::default(),
            DecodeLimits::generous(),
        ),
        Err("wire.entity_not_live")
    );
}
