use super::*;
use crate::relation_frontend::{compile, FrontendOptions};
use crate::value::{Builtin, ComponentData, Value};
use crate::vm::VM;
use crate::world::World;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

fn options() -> FrontendOptions {
    FrontendOptions {
        enabled: true,
        module_id: "game::inventory".into(),
        ..FrontendOptions::default()
    }
}

fn artifacts(source: &str) -> crate::relation_frontend::FrontendArtifacts {
    compile(source, &options()).unwrap()
}

fn install(world: &mut World, source: &str) -> crate::relation_frontend::FrontendArtifacts {
    let artifacts = artifacts(source);
    let manifest = Arc::new(RelationRuntimeManifest::from_frontend(&artifacts).unwrap());
    world
        .install_relation_manifest(manifest, artifacts.manifest_digest)
        .unwrap();
    artifacts
}

fn existing(entity: EntityRef) -> PendingRelationValue {
    PendingRelationValue::Entity(EntityOperand::Existing(entity))
}

fn component(name: &str) -> ComponentData {
    ComponentData {
        type_name: name.into(),
        layout: Arc::new(Vec::new()),
        values: Vec::new(),
    }
}

fn insert(
    relation: &str,
    tuple: Vec<PendingRelationValue>,
    cause: &str,
) -> PendingRelationOperation {
    PendingRelationOperation::Insert {
        fact: PendingFactKey::new(relation, tuple),
        metadata: OperationMetadata::cause(cause),
    }
}

#[test]
fn manifest_binds_kind_owner_and_frontend_identity() {
    let authoritative = artifacts("relation Source(value: int)\n");
    let derived =
        artifacts("relation Input(value: int)\nderive Source(value)\n    when Input(value)\n");
    let left = RelationRuntimeManifest::from_frontend(&authoritative).unwrap();
    let right = RelationRuntimeManifest::from_frontend(&derived).unwrap();
    assert_ne!(left.digest(), right.digest());
    assert_ne!(left.frontend_digest(), right.frontend_digest());
}

#[test]
fn runtime_rejects_frontend_artifacts_detached_from_their_digest() {
    let mut left = artifacts("relation Left(value: int)\n");
    let right = artifacts("relation Right(value: int)\n");
    left.relations = right.relations;
    let error = RelationRuntimeManifest::from_frontend(&left).unwrap_err();
    assert_eq!(error.code, "relation.frontend_manifest_mismatch");
}

#[test]
fn frontend_operations_install_and_commit_canonical_assertions() {
    let source = r#"
relation Owns(owner: entity, item: entity)
    unique item
Insert(Owns, (alice, sword))
"#;
    let artifacts = artifacts(source);
    let mut vm = VM::new_with_seed(7);
    vm.get_world_mut().spawn_entity(Some("alice"));
    vm.get_world_mut().spawn_entity(Some("sword"));
    let before = vm.compiled_program_manifest().unwrap().digest().to_string();
    let changes = vm.apply_frontend_relation_operations(&artifacts).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(vm.get_world().relation_state().assertions().len(), 1);
    assert_eq!(vm.get_world().relation_state().next_assertion_id(), 1);
    assert_ne!(before, vm.compiled_program_manifest().unwrap().digest());
}

#[test]
fn symmetric_aliases_share_one_physical_assertion() {
    let mut world = World::new();
    install(
        &mut world,
        "relation Allied(left: entity, right: entity)\n    symmetric\n    on delete cascade\n",
    );
    let a = world.spawn_entity(Some("a"));
    let b = world.spawn_entity(Some("b"));
    let a = world.entity_ref(a).unwrap();
    let b = world.entity_ref(b).unwrap();
    let relation = "game::inventory::Allied";
    let transaction = RelationTransaction {
        spawns: Vec::new(),
        component_writes: Vec::new(),
        operations: vec![
            insert(relation, vec![existing(a), existing(b)], "left"),
            insert(relation, vec![existing(b), existing(a)], "right"),
        ],
        despawns: Vec::new(),
    };
    world.apply_relation_transaction(&transaction).unwrap();
    let assertions = world.relation_state().assertions();
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions.values().next().unwrap().causes.len(), 2);
    let rows = world.relation_state().logical_rows(relation).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].assertion_id, rows[1].assertion_id);
    assert_ne!(rows[0].tuple, rows[1].tuple);
}

#[test]
fn replacement_allocates_a_new_assertion_lifetime() {
    let mut world = World::new();
    install(
        &mut world,
        "relation Owns(owner: entity, item: entity)\n    unique item\n",
    );
    let alice_id = world.spawn_entity(Some("alice"));
    let bob_id = world.spawn_entity(Some("bob"));
    let sword_id = world.spawn_entity(Some("sword"));
    let alice = world.entity_ref(alice_id).unwrap();
    let bob = world.entity_ref(bob_id).unwrap();
    let sword = world.entity_ref(sword_id).unwrap();
    let relation = "game::inventory::Owns";
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                relation,
                vec![existing(alice), existing(sword)],
                "settlement.a",
            )],
            despawns: Vec::new(),
        })
        .unwrap();
    let first = world
        .relation_state()
        .assertions()
        .values()
        .next()
        .unwrap()
        .assertion_id;
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![PendingRelationOperation::ReplaceBy {
                relation: relation.into(),
                unique_constraint: "item".into(),
                selected_key: vec![existing(sword)],
                tuple: vec![existing(bob), existing(sword)],
                metadata: OperationMetadata::cause("settlement.b"),
            }],
            despawns: Vec::new(),
        })
        .unwrap();
    let assertion = world.relation_state().assertions().values().next().unwrap();
    assert!(assertion.assertion_id > first);
    assert_eq!(assertion.causes, BTreeSet::from(["settlement.b".into()]));
}

#[test]
fn restrict_and_unique_failures_are_atomic() {
    let mut world = World::new();
    install(
        &mut world,
        "relation Owns(owner: entity, item: entity)\n    unique item\n",
    );
    let alice_id = world.spawn_entity(Some("alice"));
    let bob_id = world.spawn_entity(Some("bob"));
    let sword_id = world.spawn_entity(Some("sword"));
    let alice = world.entity_ref(alice_id).unwrap();
    let bob = world.entity_ref(bob_id).unwrap();
    let sword = world.entity_ref(sword_id).unwrap();
    let relation = "game::inventory::Owns";
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                relation,
                vec![existing(alice), existing(sword)],
                "base",
            )],
            despawns: Vec::new(),
        })
        .unwrap();
    let before = world.snapshot();
    let before_bytes = world.relation_state().operational_checkpoint_bytes();
    let error = world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                relation,
                vec![existing(bob), existing(sword)],
                "conflict",
            )],
            despawns: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(error.code, "relation.unique_conflict");
    assert_eq!(
        before_bytes,
        world.relation_state().operational_checkpoint_bytes()
    );
    assert!(world.entity_exists(sword.slot));

    let error = world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: Vec::new(),
            despawns: vec![PendingDespawn {
                entity: sword,
                metadata: OperationMetadata::cause("despawn"),
            }],
        })
        .unwrap_err();
    assert_eq!(error.code, "entity.delete_restricted");
    assert!(world.entity_exists(sword.slot));
    let mut restored = World::new();
    restored.restore(before);
    assert_eq!(
        restored.relation_state().operational_checkpoint_bytes(),
        world.relation_state().operational_checkpoint_bytes()
    );
}

#[test]
fn cascade_and_generation_reuse_never_retarget_a_fact() {
    let mut world = World::new();
    install(
        &mut world,
        "relation Carries(owner: entity, item: entity on delete cascade)\n",
    );
    let owner_id = world.spawn_entity(Some("owner"));
    let item_id = world.spawn_entity(Some("item"));
    let owner = world.entity_ref(owner_id).unwrap();
    let item = world.entity_ref(item_id).unwrap();
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                "game::inventory::Carries",
                vec![existing(owner), existing(item)],
                "base",
            )],
            despawns: Vec::new(),
        })
        .unwrap();
    assert!(world.destroy_entity(item.slot));
    assert!(world.relation_state().assertions().is_empty());
    let replacement_id = world.spawn_entity(Some("monster"));
    let replacement = world.entity_ref(replacement_id).unwrap();
    assert_eq!(replacement.slot, item.slot);
    assert_ne!(replacement.generation, item.generation);
}

#[test]
fn checkpoint_inventory_binds_assertions_indexes_and_allocator() {
    let mut world = World::new();
    install(
        &mut world,
        "relation Marker(value: int)\n    unique value\n",
    );
    let before = world.snapshot();
    let before_bytes = world.relation_state().operational_checkpoint_bytes();
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                "game::inventory::Marker",
                vec![PendingRelationValue::Int(1)],
                "one",
            )],
            despawns: Vec::new(),
        })
        .unwrap();
    assert_ne!(
        before_bytes,
        world.relation_state().operational_checkpoint_bytes()
    );
    world.restore(before);
    assert_eq!(
        before_bytes,
        world.relation_state().operational_checkpoint_bytes()
    );
}

#[test]
fn canonical_relation_transport_round_trips_assertion_identity_and_ancestry() {
    let mut world = World::new();
    install(
        &mut world,
        "relation Marker(value: int)\n    unique value\n",
    );
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![PendingRelationOperation::Insert {
                fact: PendingFactKey::new(
                    "game::inventory::Marker",
                    vec![PendingRelationValue::Int(7)],
                ),
                metadata: OperationMetadata::cause("settlement:7")
                    .with_capability("inventory.read"),
            }],
            ..RelationTransaction::default()
        })
        .unwrap();

    let state = world.relation_state();
    let encoded = state.transport_hex().unwrap();
    let restored = AuthoritativeRelationState::from_transport_hex(
        &encoded,
        Arc::clone(state.manifest().unwrap()),
    )
    .unwrap();
    assert_eq!(
        state.operational_checkpoint_bytes(),
        restored.operational_checkpoint_bytes()
    );
    assert_eq!(state.assertions(), restored.assertions());
    assert_eq!(state.next_assertion_id(), restored.next_assertion_id());

    let mut incompatible = World::new();
    install(&mut incompatible, "relation Marker(value: text)\n");
    let error = AuthoritativeRelationState::from_transport_hex(
        &encoded,
        Arc::clone(incompatible.relation_state().manifest().unwrap()),
    )
    .unwrap_err();
    assert_eq!(error.code, "relation.transport_manifest_mismatch");
}

#[test]
fn save_load_preserves_authoritative_relation_state_exactly() {
    let front_end = artifacts("relation Owns(owner: entity, item: entity)\n    unique item\n");
    let mut source = VM::new_with_seed(17);
    source.install_relation_frontend(&front_end).unwrap();
    let recycled = source.get_world_mut().spawn_entity(Some("recycled"));
    assert!(source.get_world_mut().destroy_entity(recycled));
    let alice_id = source.get_world_mut().spawn_entity(Some("alice"));
    let sword_id = source.get_world_mut().spawn_entity(Some("sword"));
    let alice = source.get_world().entity_ref(alice_id).unwrap();
    let sword = source.get_world().entity_ref(sword_id).unwrap();
    source
        .get_world_mut()
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![PendingRelationOperation::Insert {
                fact: PendingFactKey::new(
                    "game::inventory::Owns",
                    vec![existing(alice), existing(sword)],
                ),
                metadata: OperationMetadata::cause("settlement.acquire")
                    .with_capability("inventory.read"),
            }],
            ..RelationTransaction::default()
        })
        .unwrap();
    let expected = source
        .get_world()
        .relation_state()
        .operational_checkpoint_bytes();
    let saved = source
        .call_builtin(Builtin::SaveWorld, Vec::new())
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();

    let mut restored = VM::new_with_seed(19);
    restored.install_relation_frontend(&front_end).unwrap();
    let payload = Value::from_string(restored.gc_mut(), saved.clone());
    restored
        .call_builtin(Builtin::LoadWorld, vec![payload])
        .unwrap();
    assert_eq!(
        expected,
        restored
            .get_world()
            .relation_state()
            .operational_checkpoint_bytes()
    );
    assert_eq!(
        restored.get_world().get_entity_by_name("alice"),
        Some(alice.slot)
    );
    assert_eq!(restored.get_world().entity_ref(alice.slot), Some(alice));
    assert_eq!(
        restored.get_world().get_entity_by_name("sword"),
        Some(sword.slot)
    );

    let plain = crate::radpack::open(&saved).unwrap();
    let rest = plain.strip_prefix("RADWORLD3 ").unwrap();
    let (_, body) = rest.split_once(' ').unwrap();
    let mut document = serde_json::from_str::<serde_json::Value>(body).unwrap();
    document.as_object_mut().unwrap().remove("relations");
    let incomplete = crate::radpack::seal("RADWORLD3", &document.to_string());
    let payload = Value::from_string(restored.gc_mut(), incomplete);
    let error = restored
        .call_builtin(Builtin::LoadWorld, vec![payload])
        .unwrap_err();
    assert!(
        error.contains("payload omits authoritative relation state"),
        "got: {error}"
    );
    assert_eq!(
        expected,
        restored
            .get_world()
            .relation_state()
            .operational_checkpoint_bytes()
    );
}

#[test]
fn full_fork_wire_preserves_authoritative_relation_state_exactly() {
    let front_end = artifacts("relation Marker(value: int)\n    unique value\n");
    let mut vm = VM::new_with_seed(23);
    vm.install_relation_frontend(&front_end).unwrap();
    vm.get_world_mut()
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![insert(
                "game::inventory::Marker",
                vec![PendingRelationValue::Int(9)],
                "wire",
            )],
            ..RelationTransaction::default()
        })
        .unwrap();
    let expected = vm
        .get_world()
        .relation_state()
        .operational_checkpoint_bytes();
    let snapshot = Arc::new(vm.get_world().snapshot());
    let fork = Value::world_fork(vm.gc_mut(), snapshot);
    let encoded = vm
        .call_builtin(Builtin::ForkToBytes, vec![fork])
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let payload = Value::from_string(vm.gc_mut(), encoded);
    let decoded = vm
        .call_builtin(Builtin::ForkFromBytes, vec![payload])
        .unwrap();
    let result = decoded.as_sum_type().unwrap();
    assert_eq!(result.variant, "Ok");
    let snapshot = result.fields["value"].as_world_fork().unwrap();
    assert_eq!(
        expected,
        snapshot.relation_state().operational_checkpoint_bytes()
    );
}

#[test]
fn fork_delta_preserves_authoritative_relation_state_exactly() {
    let front_end = artifacts("relation Marker(value: int)\n");
    let mut vm = VM::new_with_seed(29);
    vm.install_relation_frontend(&front_end).unwrap();
    let base_snapshot = Arc::new(vm.get_world().snapshot());
    vm.get_world_mut()
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![insert(
                "game::inventory::Marker",
                vec![PendingRelationValue::Int(3)],
                "delta",
            )],
            ..RelationTransaction::default()
        })
        .unwrap();
    let expected = vm
        .get_world()
        .relation_state()
        .operational_checkpoint_bytes();
    let target_snapshot = Arc::new(vm.get_world().snapshot());
    let base = Value::world_fork(vm.gc_mut(), Arc::clone(&base_snapshot));
    let target = Value::world_fork(vm.gc_mut(), target_snapshot);
    let delta = vm
        .call_builtin(Builtin::ForkDelta, vec![base, target])
        .unwrap();
    let local_base = Value::world_fork(vm.gc_mut(), base_snapshot);
    let applied = vm
        .call_builtin(Builtin::ForkApply, vec![local_base, delta])
        .unwrap();
    let result = applied.as_sum_type().unwrap();
    assert_eq!(result.variant, "Ok");
    let snapshot = result.fields["value"].as_world_fork().unwrap();
    assert_eq!(
        expected,
        snapshot.relation_state().operational_checkpoint_bytes()
    );
}

#[test]
fn relation_only_changes_affect_content_digest() {
    let mut world = World::new();
    install(&mut world, "relation Marker(value: int)\n");
    let before = world.content_digest();
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![insert(
                "game::inventory::Marker",
                vec![PendingRelationValue::Int(1)],
                "digest",
            )],
            ..RelationTransaction::default()
        })
        .unwrap();
    assert_ne!(before, world.content_digest());
}

#[test]
fn relation_only_changes_affect_world_digest_builtin() {
    let front_end = artifacts("relation Marker(value: int)\n");
    let mut vm = VM::new_with_seed(31);
    vm.install_relation_frontend(&front_end).unwrap();
    let before = vm
        .call_builtin(Builtin::WorldDigest, Vec::new())
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    vm.get_world_mut()
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![insert(
                "game::inventory::Marker",
                vec![PendingRelationValue::Int(1)],
                "digest",
            )],
            ..RelationTransaction::default()
        })
        .unwrap();
    let after = vm
        .call_builtin(Builtin::WorldDigest, Vec::new())
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(before, after);
}

#[test]
fn merge_fails_closed_before_relation_only_branch_changes_can_be_lost() {
    let mut world = World::new();
    install(&mut world, "relation Marker(value: int)\n");
    let base = world.snapshot();
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![insert(
                "game::inventory::Marker",
                vec![PendingRelationValue::Int(1)],
                "ours",
            )],
            ..RelationTransaction::default()
        })
        .unwrap();
    let ours = world.snapshot();
    let errors = crate::merge::merge_worlds(
        &base,
        &ours,
        &base,
        &mut crate::value::PersistentStore,
        &crate::merge::Resolutions::default(),
    )
    .err()
    .expect("relation divergence must fail closed");
    assert!(matches!(
        errors.as_slice(),
        [crate::merge::MergeConflict::Relations { .. }]
    ));
}

#[test]
fn bounded_transaction_rejects_before_world_mutation() {
    let mut world = World::new();
    install(&mut world, "relation Marker(value: int)\n");
    let before = world.snapshot();
    let profile = RelationTransactionProfile {
        max_operations: 0,
        ..RelationTransactionProfile::default()
    };
    let mut builder = BoundedRelationTransactionBuilder::new(profile);
    let error = builder
        .push_operation(insert(
            "game::inventory::Marker",
            vec![PendingRelationValue::Int(1)],
            "oversized",
        ))
        .unwrap_err();
    assert_eq!(error.code, "relation.transaction_operation_limit");
    assert_eq!(
        before.relation_state().operational_checkpoint_bytes(),
        world.relation_state().operational_checkpoint_bytes()
    );
    assert_eq!(before.next_id, world.snapshot().next_id);
}

#[test]
fn portable_attempt_checkpoint_binds_authoritative_relation_state() {
    let artifacts = artifacts("relation Marker(value: int)\n    unique value\n");
    let mut vm = VM::new_with_seed(11);
    vm.install_relation_frontend(&artifacts).unwrap();
    let before = vm.attempt_checkpoint_digest().unwrap();
    vm.get_world_mut()
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                "game::inventory::Marker",
                vec![PendingRelationValue::Int(1)],
                "checkpoint",
            )],
            despawns: Vec::new(),
        })
        .unwrap();
    assert_ne!(before, vm.attempt_checkpoint_digest().unwrap());
}

#[test]
fn candidate_handles_are_resolved_before_canonical_validation() {
    let mut state = AuthoritativeRelationState::default();
    let artifacts = artifacts("relation Tagged(entity: entity, label: text)\n");
    let manifest = Arc::new(RelationRuntimeManifest::from_frontend(&artifacts).unwrap());
    state
        .install_manifest(manifest, artifacts.manifest_digest)
        .unwrap();
    let spawned = EntityRef {
        slot: 4,
        generation: 2,
    };
    let entities = CandidateEntityState {
        live_after: BTreeSet::from([spawned]),
        candidate_handles: BTreeMap::from([(7, spawned)]),
    };
    let candidate = state
        .prepare_candidate(
            &RelationTransaction {
                spawns: Vec::new(),
                component_writes: Vec::new(),
                operations: vec![insert(
                    "game::inventory::Tagged",
                    vec![
                        PendingRelationValue::Entity(EntityOperand::Candidate(7)),
                        PendingRelationValue::Text("new".into()),
                    ],
                    "spawn",
                )],
                despawns: Vec::new(),
            },
            &entities,
        )
        .unwrap();
    assert_eq!(candidate.state().assertions().len(), 1);
}

#[test]
fn spawn_component_and_relation_patch_share_one_atomic_candidate() {
    let mut world = World::new();
    install(&mut world, "relation Tagged(entity: entity, label: text)\n");
    let transaction = RelationTransaction {
        spawns: vec![PendingSpawn {
            handle: 7,
            name: Some("newcomer".into()),
        }],
        component_writes: vec![PendingComponentWrite {
            entity: EntityOperand::Candidate(7),
            component: component("Marker"),
        }],
        operations: vec![insert(
            "game::inventory::Tagged",
            vec![
                PendingRelationValue::Entity(EntityOperand::Candidate(7)),
                PendingRelationValue::Text("fresh".into()),
            ],
            "spawn",
        )],
        despawns: Vec::new(),
    };
    world.apply_relation_transaction(&transaction).unwrap();
    let slot = world.get_entity_by_name("newcomer").unwrap();
    let entity = world.entity_ref(slot).unwrap();
    assert!(world.has_component(slot, "Marker"));
    assert!(world
        .relation_state()
        .contains(FactKey::new(
            "game::inventory::Tagged",
            vec![FactValue::Entity(entity), FactValue::Text("fresh".into())],
        ))
        .unwrap());
}

#[test]
fn relation_conflict_rolls_back_candidate_spawn_and_component_write() {
    let mut world = World::new();
    install(
        &mut world,
        "relation Owns(owner: entity, item: entity)\n    unique item\n",
    );
    let alice_id = world.spawn_entity(Some("alice"));
    let sword_id = world.spawn_entity(Some("sword"));
    let alice = world.entity_ref(alice_id).unwrap();
    let sword = world.entity_ref(sword_id).unwrap();
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                "game::inventory::Owns",
                vec![existing(alice), existing(sword)],
                "base",
            )],
            despawns: Vec::new(),
        })
        .unwrap();
    let before = world.snapshot_json_like();
    let relation_before = world.relation_state().operational_checkpoint_bytes();
    let error = world
        .apply_relation_transaction(&RelationTransaction {
            spawns: vec![PendingSpawn {
                handle: 9,
                name: Some("bob".into()),
            }],
            component_writes: vec![PendingComponentWrite {
                entity: EntityOperand::Candidate(9),
                component: component("Marker"),
            }],
            operations: vec![insert(
                "game::inventory::Owns",
                vec![
                    PendingRelationValue::Entity(EntityOperand::Candidate(9)),
                    existing(sword),
                ],
                "conflict",
            )],
            despawns: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(error.code, "relation.unique_conflict");
    assert_eq!(world.snapshot_json_like(), before);
    assert_eq!(
        world.relation_state().operational_checkpoint_bytes(),
        relation_before
    );
    assert!(world.get_entity_by_name("bob").is_none());
}

#[test]
fn entity_allocator_uses_the_last_fresh_slot_then_fails_as_typed_data() {
    let mut world = World::new();
    // This fixture jumps the fresh cursor to its final value so the boundary
    // is testable without materializing the preceding 2^32 identities.
    let mut snapshot = world.snapshot();
    snapshot.next_id = u32::MAX;
    world.restore(snapshot);
    assert_eq!(world.try_spawn_entity(Some("last")).unwrap(), u32::MAX);
    assert_eq!(
        world.try_spawn_entity(Some("overflow")),
        Err("entity.id_space_exhausted")
    );
    assert_eq!(world.get_entity_by_name("last"), Some(u32::MAX));
    assert!(world.get_entity_by_name("overflow").is_none());
}

#[test]
fn exhausted_reusable_generation_is_retired_without_blocking_other_capacity() {
    let mut world = World::new();
    let retired = world.spawn_entity(None);
    assert!(world.destroy_entity(retired));
    let mut snapshot = world.snapshot();
    snapshot.generations = Arc::new(std::collections::HashMap::from([(retired, u32::MAX)]));
    world.restore(snapshot);
    let replacement = world.try_spawn_entity(None).unwrap();
    assert_ne!(replacement, retired);
    assert_eq!(world.entity_ref(replacement).unwrap().generation, 0);
}

#[test]
fn ownership_weight_capacity_dogfood_exercises_authoritative_transfer() {
    let source = r#"
relation Owns(owner: entity, item: entity)
    unique item
relation ItemWeight(item: entity, weight: int)
    unique item
relation CarryCapacity(person: entity, capacity: int)
    unique person
"#;
    let mut world = World::new();
    install(&mut world, source);
    let alice_id = world.spawn_entity(Some("alice"));
    let bob_id = world.spawn_entity(Some("bob"));
    let sword_id = world.spawn_entity(Some("sword"));
    let alice = world.entity_ref(alice_id).unwrap();
    let bob = world.entity_ref(bob_id).unwrap();
    let sword = world.entity_ref(sword_id).unwrap();
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![
                insert(
                    "game::inventory::Owns",
                    vec![existing(alice), existing(sword)],
                    "settlement.acquire",
                ),
                insert(
                    "game::inventory::ItemWeight",
                    vec![existing(sword), PendingRelationValue::Int(7)],
                    "settlement.weigh",
                ),
                insert(
                    "game::inventory::CarryCapacity",
                    vec![existing(alice), PendingRelationValue::Int(10)],
                    "settlement.capacity",
                ),
            ],
            despawns: Vec::new(),
        })
        .unwrap();
    let before_conflict = world.relation_state().operational_checkpoint_bytes();
    let error = world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![insert(
                "game::inventory::Owns",
                vec![existing(bob), existing(sword)],
                "settlement.invalid_transfer",
            )],
            despawns: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(error.code, "relation.unique_conflict");
    assert_eq!(
        before_conflict,
        world.relation_state().operational_checkpoint_bytes()
    );
    world
        .apply_relation_transaction(&RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: vec![PendingRelationOperation::ReplaceBy {
                relation: "game::inventory::Owns".into(),
                unique_constraint: "item".into(),
                selected_key: vec![existing(sword)],
                tuple: vec![existing(bob), existing(sword)],
                metadata: OperationMetadata::cause("settlement.transfer"),
            }],
            despawns: Vec::new(),
        })
        .unwrap();
    let owns = world
        .relation_state()
        .logical_rows("game::inventory::Owns")
        .unwrap();
    assert_eq!(owns.len(), 1);
    assert_eq!(
        owns[0].tuple,
        vec![FactValue::Entity(bob), FactValue::Entity(sword)]
    );
    assert_eq!(
        owns[0].causes,
        BTreeSet::from(["settlement.transfer".into()])
    );
}
