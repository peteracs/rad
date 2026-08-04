use super::*;
use crate::relation_frontend::{compile, FrontendOptions};
use crate::value::ComponentData;
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
    world.set_id_allocator(u32::MAX, Vec::new()).unwrap();
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

#[test]
fn relation_runtime_sources_stay_below_one_thousand_lines() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/relation_runtime");
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let lines = std::fs::read_to_string(&path).unwrap().lines().count();
            assert!(
                lines <= 1_000,
                "{} has {lines} lines; runtime files are capped at 1000",
                path.display()
            );
        }
    }
}
