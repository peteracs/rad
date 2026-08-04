//! Authoritative relation/component candidate and entity-lifetime contracts.

use super::*;

#[test]
fn entity_foreign_keys_restrict_cascade_and_never_retarget_reused_slots() {
    let mut model = WorldModel::default();
    model
        .relations
        .register(RelationSchema::new(
            "Restricts",
            vec![entity_column("owner"), entity_column("target")],
        ))
        .unwrap();
    model
        .relations
        .register(RelationSchema::new(
            "Cascades",
            vec![entity_column("owner"), entity_column("target").cascade()],
        ))
        .unwrap();
    let owner = model.entities.spawn().unwrap();
    let target = model.entities.spawn().unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Restricts", vec![existing(owner), existing(target)]),
                "settlement.restrict",
            )],
            ..Transaction::default()
        })
        .unwrap();
    let before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            despawns: vec![despawn(target, "settlement.restricted_despawn")],
            ..Transaction::default()
        }),
        Err("entity.delete_restricted")
    );
    assert_eq!(model, before);

    model
        .apply_transaction(Transaction {
            operations: vec![
                remove(
                    pending_key("Restricts", vec![existing(owner), existing(target)]),
                    "settlement.remove_restrict",
                ),
                insert(
                    pending_key("Cascades", vec![existing(owner), existing(target)]),
                    "settlement.cascade_source",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    model
        .apply_transaction(Transaction {
            despawns: vec![despawn(target, "settlement.cascade_despawn")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(model
        .relations
        .assertions
        .keys()
        .all(|key| { !key.tuple.contains(&FactValue::Entity(target)) }));
    assert!(model
        .relations
        .last_changes
        .iter()
        .any(|change| change.kind == ChangeKind::Cascade));
    let replacement = model.entities.spawn().unwrap();
    assert_eq!(replacement.slot, target.slot);
    assert_ne!(replacement.generation, target.generation);

    let handles = model
        .apply_transaction(Transaction {
            spawn_handles: BTreeSet::from([9, 3]),
            operations: vec![
                insert(
                    pending_key("Cascades", vec![existing(owner), candidate(9)]),
                    "settlement.same_candidate_spawn.9",
                ),
                insert(
                    pending_key("Cascades", vec![existing(owner), candidate(3)]),
                    "settlement.same_candidate_spawn.3",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    assert!(model.entities.contains(handles[&9]));
    assert!(model.entities.contains(handles[&3]));
    assert!(handles[&3].slot < handles[&9].slot);
}

#[test]
fn symmetric_storage_has_two_logical_orientations_and_one_self_orientation() {
    let mut model = WorldModel::default();
    let a = model.entities.spawn().unwrap();
    let b = model.entities.spawn().unwrap();
    model
        .relations
        .register(
            RelationSchema::new(
                "AlliedWith",
                vec![entity_column("left"), entity_column("right")],
            )
            .symmetric(),
        )
        .unwrap();
    assert_eq!(
        RelationSchema::new(
            "InvalidPartner",
            vec![entity_column("left"), entity_column("right")],
        )
        .symmetric()
        .unique("left", &[0])
        .validate_declaration(),
        Err("relation.symmetric_unique_forbidden")
    );
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key("AlliedWith", vec![existing(b), existing(a)]),
                    "law.b",
                ),
                insert(
                    pending_key("AlliedWith", vec![existing(a), existing(b)]),
                    "law.a",
                ),
                insert(
                    pending_key("AlliedWith", vec![existing(a), existing(a)]),
                    "law.self",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(model.relations.assertions.len(), 2);
    assert!(model
        .relations
        .contains_ground(FactKey::new(
            "AlliedWith",
            vec![FactValue::Entity(b), FactValue::Entity(a)],
        ))
        .unwrap());
    let rows = model.relations.logical_rows("AlliedWith").unwrap();
    assert_eq!(rows.len(), 3);

    let schemas = derived_schemas();
    let rules = vec![
        RulePlan {
            id: "derive.HasAlly".to_owned(),
            head_relation: "HasAlly".to_owned(),
            head: vec![Term::var("person")],
            atoms: vec![Atom::new(
                "AlliedWith",
                vec![Term::var("person"), Term::var("other")],
            )],
            predicates: Vec::new(),
            aggregate: None,
        },
        RulePlan {
            id: "derive.CountAllies".to_owned(),
            head_relation: "CountAllies".to_owned(),
            head: vec![Term::var("person"), Term::var("count")],
            atoms: vec![Atom::new(
                "AlliedWith",
                vec![Term::var("person"), Term::var("other")],
            )],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind: AggregateKind::Count,
                input: None,
                output: "count".to_owned(),
                group_by: vec!["person".to_owned()],
            }),
        },
    ];
    let derived = derive_all(&model.relations, &schemas, &rules, limits(64)).unwrap();
    assert!(derived.contains_key(&FactKey::new("HasAlly", vec![FactValue::Entity(a)],)));
    assert!(derived.contains_key(&FactKey::new("HasAlly", vec![FactValue::Entity(b)],)));
    assert!(derived.contains_key(&FactKey::new(
        "CountAllies",
        vec![FactValue::Entity(a), FactValue::Count(2)],
    )));
    assert!(derived.contains_key(&FactKey::new(
        "CountAllies",
        vec![FactValue::Entity(b), FactValue::Count(1)],
    )));
}

#[test]
fn patch_algebra_is_base_relative_named_and_order_independent() {
    let (mut model, owner, item, _) = seed_ownership_model();
    let original = model
        .relations
        .assertions
        .get(&FactKey::new(
            "Owns",
            vec![FactValue::Entity(owner), FactValue::Entity(item)],
        ))
        .unwrap()
        .clone();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "ignored.duplicate",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(model.relations.assertions[&original.key], original);

    let absent = pending_key("Owns", vec![existing(item), existing(owner)]);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(absent, "ignored.absent")],
            ..Transaction::default()
        })
        .unwrap();

    let conflicting = pending_key("Owns", vec![existing(owner), existing(item)]);
    let before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![
                insert(conflicting.clone(), "law.insert"),
                remove(conflicting, "law.remove"),
            ],
            ..Transaction::default()
        }),
        Err("relation.operation_conflict")
    );
    assert_eq!(model, before);

    let new_owner = model.entities.spawn().unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(new_owner), existing(item)],
                    metadata: OperationMetadata::cause("law.transfer.b"),
                },
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(new_owner), existing(item)],
                    metadata: OperationMetadata::cause("law.transfer.a"),
                },
            ],
            ..Transaction::default()
        })
        .unwrap();
    let transferred = &model.relations.assertions[&FactKey::new(
        "Owns",
        vec![FactValue::Entity(new_owner), FactValue::Entity(item)],
    )];
    assert_eq!(
        transferred.causes,
        BTreeSet::from(["law.transfer.a".to_owned(), "law.transfer.b".to_owned()])
    );
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Owns".to_owned(),
                unique_constraint: "missing".to_owned(),
                selected_key: vec![existing(item)],
                tuple: vec![existing(owner), existing(item)],
                metadata: OperationMetadata::cause("bad"),
            }],
            ..Transaction::default()
        }),
        Err("relation.unknown_unique")
    );

    let before_conflicting_replacements = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(owner), existing(item)],
                    metadata: OperationMetadata::cause("replace.one"),
                },
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(new_owner), existing(item)],
                    metadata: OperationMetadata::cause("replace.two"),
                },
            ],
            ..Transaction::default()
        }),
        Err("relation.replacement_conflict")
    );
    assert_eq!(model, before_conflicting_replacements);

    model
        .relations
        .register(
            RelationSchema::new(
                "Account",
                vec![
                    ColumnSchema::new("user", ValueKind::Text),
                    ColumnSchema::new("email", ValueKind::Text),
                ],
            )
            .unique("user", &[0])
            .unique("email", &[1]),
        )
        .unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key(
                        "Account",
                        vec![
                            PendingValue::Text("alice".to_owned()),
                            PendingValue::Text("a@example".to_owned()),
                        ],
                    ),
                    "account.alice",
                ),
                insert(
                    pending_key(
                        "Account",
                        vec![
                            PendingValue::Text("bob".to_owned()),
                            PendingValue::Text("b@example".to_owned()),
                        ],
                    ),
                    "account.bob",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let before_other_unique = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Account".to_owned(),
                unique_constraint: "user".to_owned(),
                selected_key: vec![PendingValue::Text("alice".to_owned())],
                tuple: vec![
                    PendingValue::Text("alice".to_owned()),
                    PendingValue::Text("b@example".to_owned()),
                ],
                metadata: OperationMetadata::cause("account.conflict"),
            }],
            ..Transaction::default()
        }),
        Err("relation.unique_conflict")
    );
    assert_eq!(model, before_other_unique);

    model
        .relations
        .register(
            RelationSchema::new(
                "AlliedPatch",
                vec![entity_column("left"), entity_column("right")],
            )
            .symmetric(),
        )
        .unwrap();
    let symmetric_before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key("AlliedPatch", vec![existing(owner), existing(new_owner)]),
                    "symmetric.insert",
                ),
                remove(
                    pending_key("AlliedPatch", vec![existing(new_owner), existing(owner)]),
                    "symmetric.remove",
                ),
            ],
            ..Transaction::default()
        }),
        Err("relation.operation_conflict")
    );
    assert_eq!(model, symmetric_before);
}

#[test]
fn all_authoritative_rows_are_schema_validated_and_component_relation_failures_are_atomic() {
    let (mut model, owner, item, _) = seed_ownership_model();
    model.components.insert(
        (owner, "Position".to_owned()),
        FactValue::Text("base".to_owned()),
    );
    let before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            spawn_handles: BTreeSet::from([1]),
            component_writes: vec![PendingComponentWrite {
                entity: EntityOperand::Existing(owner),
                component: "Position".to_owned(),
                value: FactValue::Text("candidate".to_owned()),
            }],
            operations: vec![insert(
                pending_key("ItemWeight", vec![existing(item), int(99)]),
                "duplicate.weight",
            )],
            ..Transaction::default()
        }),
        Err("relation.unique_conflict")
    );
    assert_eq!(
        model, before,
        "component, entity, and relation writes are one atomic candidate"
    );
}

#[test]
fn component_writes_are_canonical_coalesced_and_conflict_atomically() {
    let (mut seed, owner, _, _) = seed_ownership_model();
    let standalone = seed.entities.spawn().unwrap();
    seed.relations
        .register(RelationSchema::new("Marker", vec![int_column("value")]))
        .unwrap();

    let write = |value: &str| PendingComponentWrite {
        entity: EntityOperand::Existing(owner),
        component: "Position".to_owned(),
        value: FactValue::Text(value.to_owned()),
    };
    let mut forward = seed.clone();
    forward
        .apply_transaction(Transaction {
            component_writes: vec![write("same"), write("same")],
            ..Transaction::default()
        })
        .unwrap();
    let mut reverse = seed.clone();
    reverse
        .apply_transaction(Transaction {
            component_writes: vec![write("same"), write("same")]
                .into_iter()
                .rev()
                .collect(),
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(forward, reverse);

    for writes in [
        vec![write("first"), write("second")],
        vec![write("second"), write("first")],
    ] {
        let mut model = seed.clone();
        let before = model.clone();
        assert_eq!(
            model.apply_transaction(Transaction {
                spawn_handles: BTreeSet::from([7]),
                despawns: vec![despawn(standalone, "despawn.standalone")],
                component_writes: writes,
                operations: vec![insert(pending_key("Marker", vec![int(1)]), "marker.insert",)],
            }),
            Err("component.write_conflict")
        );
        assert_eq!(model, before);
    }

    let mut candidate_local = seed.clone();
    let before = candidate_local.clone();
    assert_eq!(
        candidate_local.apply_transaction(Transaction {
            spawn_handles: BTreeSet::from([3]),
            component_writes: vec![
                PendingComponentWrite {
                    entity: EntityOperand::Candidate(3),
                    component: "Position".to_owned(),
                    value: FactValue::Text("a".to_owned()),
                },
                PendingComponentWrite {
                    entity: EntityOperand::Candidate(3),
                    component: "Position".to_owned(),
                    value: FactValue::Text("b".to_owned()),
                },
            ],
            ..Transaction::default()
        }),
        Err("component.write_conflict")
    );
    assert_eq!(candidate_local, before);

    let handles = candidate_local
        .apply_transaction(Transaction {
            spawn_handles: BTreeSet::from([4]),
            component_writes: vec![
                PendingComponentWrite {
                    entity: EntityOperand::Candidate(4),
                    component: "Position".to_owned(),
                    value: FactValue::Text("same".to_owned()),
                },
                PendingComponentWrite {
                    entity: EntityOperand::Candidate(4),
                    component: "Position".to_owned(),
                    value: FactValue::Text("same".to_owned()),
                },
            ],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(
        candidate_local
            .components
            .get(&(handles[&4], "Position".to_owned())),
        Some(&FactValue::Text("same".to_owned()))
    );
}

#[test]
fn entity_allocator_is_total_retires_exhausted_slots_and_fails_atomically() {
    let mut fresh = EntityTable {
        next_slot: u32::MAX,
        ..EntityTable::default()
    };
    assert_eq!(
        fresh.spawn().unwrap(),
        EntityRef {
            slot: u32::MAX,
            generation: 0,
        }
    );
    assert_eq!(fresh.spawn(), Err("entity.id_space_exhausted"));

    let mut reusable = EntityTable {
        generations: BTreeMap::from([(2, u32::MAX), (7, 4)]),
        free_slots: BTreeSet::from([2, 7]),
        next_slot: 8,
        ..EntityTable::default()
    };
    assert_eq!(
        reusable.spawn().unwrap(),
        EntityRef {
            slot: 7,
            generation: 5,
        }
    );
    assert_eq!(reusable.retired_slots, BTreeSet::from([2]));

    let mut exhausted = WorldModel::default();
    exhausted
        .relations
        .register(RelationSchema::new("Marker", vec![int_column("value")]))
        .unwrap();
    exhausted.entities = EntityTable {
        next_slot: u32::MAX,
        fresh_slots_exhausted: true,
        ..EntityTable::default()
    };
    let before = exhausted.clone();
    assert_eq!(
        exhausted.apply_transaction(Transaction {
            spawn_handles: BTreeSet::from([1]),
            component_writes: vec![PendingComponentWrite {
                entity: EntityOperand::Candidate(1),
                component: "Position".to_owned(),
                value: FactValue::Int(1),
            }],
            operations: vec![insert(pending_key("Marker", vec![int(1)]), "marker.insert",)],
            ..Transaction::default()
        }),
        Err("entity.id_space_exhausted")
    );
    assert_eq!(exhausted, before);

    exhausted
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Marker", vec![int(2)]),
                "marker.after.failure",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert!(exhausted
        .relations
        .assertions
        .contains_key(&FactKey::new("Marker", vec![FactValue::Int(2)],)));

    let mut checkpoint_drift = before.clone();
    checkpoint_drift.entities.retired_slots.insert(4);
    assert_ne!(
        operational_checkpoint_bytes(&before),
        operational_checkpoint_bytes(&checkpoint_drift)
    );
    let mut fresh_capacity = before.clone();
    fresh_capacity.entities.fresh_slots_exhausted = false;
    assert_ne!(
        operational_checkpoint_bytes(&before),
        operational_checkpoint_bytes(&fresh_capacity)
    );
}

#[test]
fn symmetric_endpoint_metadata_is_logically_symmetric() {
    assert!(RelationSchema::new(
        "CascadeFriend",
        vec![
            entity_column("left").cascade(),
            entity_column("right").cascade()
        ],
    )
    .symmetric()
    .validate_declaration()
    .is_ok());
    assert!(RelationSchema::new(
        "RestrictFriend",
        vec![entity_column("left"), entity_column("right")],
    )
    .symmetric()
    .validate_declaration()
    .is_ok());
    assert_eq!(
        RelationSchema::new(
            "MixedFriend",
            vec![entity_column("left").cascade(), entity_column("right")],
        )
        .symmetric()
        .validate_declaration(),
        Err("relation.symmetric_endpoint_metadata")
    );

    for delete_second in [false, true] {
        let mut model = WorldModel::default();
        model
            .relations
            .register(
                RelationSchema::new(
                    "RestrictedFriend",
                    vec![entity_column("left"), entity_column("right")],
                )
                .symmetric(),
            )
            .unwrap();
        let a = model.entities.spawn().unwrap();
        let b = model.entities.spawn().unwrap();
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("RestrictedFriend", vec![existing(a), existing(b)]),
                    "restricted.friend",
                )],
                ..Transaction::default()
            })
            .unwrap();
        let before = model.clone();
        assert_eq!(
            model.apply_transaction(Transaction {
                despawns: vec![despawn(
                    if delete_second { b } else { a },
                    "despawn.restricted.friend",
                )],
                ..Transaction::default()
            }),
            Err("entity.delete_restricted")
        );
        assert_eq!(model, before);
    }

    for reverse in [false, true] {
        let mut model = WorldModel::default();
        model
            .relations
            .register(
                RelationSchema::new(
                    "Friend",
                    vec![
                        entity_column("left").cascade(),
                        entity_column("right").cascade(),
                    ],
                )
                .symmetric(),
            )
            .unwrap();
        let a = model.entities.spawn().unwrap();
        let b = model.entities.spawn().unwrap();
        let tuple = if reverse { (b, a) } else { (a, b) };
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("Friend", vec![existing(tuple.0), existing(tuple.1)]),
                    "friend",
                )],
                ..Transaction::default()
            })
            .unwrap();
        model
            .apply_transaction(Transaction {
                despawns: vec![despawn(tuple.0, "despawn.friend")],
                ..Transaction::default()
            })
            .unwrap();
        assert!(model.relations.assertions.is_empty());
    }

    let mut self_edge = WorldModel::default();
    self_edge
        .relations
        .register(
            RelationSchema::new(
                "Friend",
                vec![
                    entity_column("left").cascade(),
                    entity_column("right").cascade(),
                ],
            )
            .symmetric(),
        )
        .unwrap();
    let entity = self_edge.entities.spawn().unwrap();
    self_edge
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Friend", vec![existing(entity), existing(entity)]),
                "self.friend",
            )],
            ..Transaction::default()
        })
        .unwrap();
    self_edge
        .apply_transaction(Transaction {
            despawns: vec![despawn(entity, "despawn.self")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(self_edge.relations.assertions.is_empty());
}
