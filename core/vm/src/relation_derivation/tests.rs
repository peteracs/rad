use super::*;
use crate::relation_frontend::{compile, FrontendArtifacts, FrontendOptions};
use crate::relation_runtime::{
    EntityOperand, FactKey, FactValue, OperationMetadata, PendingFactKey, PendingRelationOperation,
    PendingRelationValue, RelationRuntimeManifest, RelationTransaction,
};
use crate::world::World;
use std::collections::BTreeSet;
use std::sync::Arc;

fn artifacts(source: &str) -> FrontendArtifacts {
    compile(
        source,
        &FrontendOptions {
            enabled: true,
            module_id: "game::derived".into(),
            ..FrontendOptions::default()
        },
    )
    .unwrap()
}

fn install(world: &mut World, artifacts: &FrontendArtifacts) {
    let manifest = Arc::new(RelationRuntimeManifest::from_frontend(artifacts).unwrap());
    world
        .install_relation_manifest(manifest, artifacts.manifest_digest)
        .unwrap();
}

fn insert(
    relation: &str,
    tuple: Vec<PendingRelationValue>,
    metadata: OperationMetadata,
) -> PendingRelationOperation {
    PendingRelationOperation::Insert {
        fact: PendingFactKey::new(relation, tuple),
        metadata,
    }
}

#[test]
fn ownership_weight_capacity_derives_reality_with_proofs() {
    let source = r#"
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
"#;
    let artifacts = artifacts(source);
    let mut world = World::new();
    install(&mut world, &artifacts);
    let alice = world.spawn_entity(Some("alice")).unwrap();
    let sword = world.spawn_entity(Some("sword")).unwrap();
    let alice = world.entity_ref(alice).unwrap();
    let sword = world.entity_ref(sword).unwrap();
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![
                insert(
                    "game::derived::Owns",
                    vec![
                        PendingRelationValue::Entity(EntityOperand::Existing(alice)),
                        PendingRelationValue::Entity(EntityOperand::Existing(sword)),
                    ],
                    OperationMetadata::cause("ownership"),
                ),
                insert(
                    "game::derived::ItemWeight",
                    vec![
                        PendingRelationValue::Entity(EntityOperand::Existing(sword)),
                        PendingRelationValue::Int(40),
                    ],
                    OperationMetadata::cause("weight"),
                ),
                insert(
                    "game::derived::CarryCapacity",
                    vec![
                        PendingRelationValue::Entity(EntityOperand::Existing(alice)),
                        PendingRelationValue::Int(25),
                    ],
                    OperationMetadata::cause("capacity"),
                ),
            ],
            ..RelationTransaction::default()
        })
        .unwrap();

    let derived = derive_all(
        world.relation_state(),
        world.relation_state().manifest().unwrap(),
        DerivationLimits::default(),
    )
    .unwrap();
    let total = FactKey::new(
        "game::derived::TotalWeight",
        vec![FactValue::Entity(alice), FactValue::Int(40)],
    );
    let encumbered = FactKey::new("game::derived::Encumbered", vec![FactValue::Entity(alice)]);
    assert_eq!(derived.proofs(&total).unwrap().len(), 1);
    assert_eq!(derived.proofs(&encumbered).unwrap().len(), 1);
    assert_eq!(
        derived.proofs(&total).unwrap().iter().next().unwrap().depth,
        2
    );
    assert_eq!(
        derived
            .proofs(&encumbered)
            .unwrap()
            .iter()
            .next()
            .unwrap()
            .depth,
        3
    );
    assert_eq!(derived.stats().facts, 2);
    assert_eq!(
        world.derived_relation_state().canonical_bytes(),
        derived.canonical_bytes()
    );
}

#[test]
fn hidden_alternatives_do_not_change_public_visibility() {
    let source = r#"
relation Visible(value: int)
relation Hidden(value: int)
derive Marked(value)
    when Visible(value)
derive Marked(value)
    when Hidden(value)
derive Public(value)
    when Marked(value)
"#;
    let artifacts = artifacts(source);
    let mut world = World::new();
    install(&mut world, &artifacts);
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![
                insert(
                    "game::derived::Visible",
                    vec![PendingRelationValue::Int(1)],
                    OperationMetadata::cause("visible"),
                ),
                insert(
                    "game::derived::Hidden",
                    vec![PendingRelationValue::Int(1)],
                    OperationMetadata::cause("hidden").with_capability("secret"),
                ),
            ],
            ..RelationTransaction::default()
        })
        .unwrap();

    let derived = derive_all(
        world.relation_state(),
        world.relation_state().manifest().unwrap(),
        DerivationLimits::default(),
    )
    .unwrap();
    let public = FactKey::new("game::derived::Public", vec![FactValue::Int(1)]);
    let proofs = derived.proofs(&public).unwrap();
    assert_eq!(proofs.len(), 2);
    assert_eq!(
        proofs
            .iter()
            .map(|proof| proof.required_capabilities.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([BTreeSet::new(), BTreeSet::from(["secret".into()])])
    );
    assert!(derived.visible_facts(&BTreeSet::new()).contains(&public));
}

#[test]
fn aggregate_deduplicates_logical_bindings_but_retains_proof_branches() {
    let source = r#"
relation Left(value: int)
relation Right(value: int)
derive Marked(value)
    when Left(value)
derive Marked(value)
    when Right(value)
derive Counted(count())
    when Marked(value)
"#;
    let artifacts = artifacts(source);
    let mut world = World::new();
    install(&mut world, &artifacts);
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![
                insert(
                    "game::derived::Left",
                    vec![PendingRelationValue::Int(7)],
                    OperationMetadata::cause("left"),
                ),
                insert(
                    "game::derived::Right",
                    vec![PendingRelationValue::Int(7)],
                    OperationMetadata::cause("right"),
                ),
            ],
            ..RelationTransaction::default()
        })
        .unwrap();

    let derived = derive_all(
        world.relation_state(),
        world.relation_state().manifest().unwrap(),
        DerivationLimits::default(),
    )
    .unwrap();
    let count = FactKey::new("game::derived::Counted", vec![FactValue::Count(1)]);
    assert_eq!(derived.proofs(&count).unwrap().len(), 2);
}

#[test]
fn work_limit_failure_never_changes_authoritative_state() {
    let source = r#"
relation Left(value: int)
relation Right(value: int)
derive Joined(left, right)
    when Left(left)
    and Right(right)
"#;
    let artifacts = artifacts(source);
    let mut world = World::new();
    install(&mut world, &artifacts);
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: (0..8)
                .flat_map(|value| {
                    [
                        insert(
                            "game::derived::Left",
                            vec![PendingRelationValue::Int(value)],
                            OperationMetadata::cause("left"),
                        ),
                        insert(
                            "game::derived::Right",
                            vec![PendingRelationValue::Int(value)],
                            OperationMetadata::cause("right"),
                        ),
                    ]
                })
                .collect(),
            ..RelationTransaction::default()
        })
        .unwrap();
    let before = world.relation_state().operational_checkpoint_bytes();
    let limits = DerivationLimits {
        max_join_attempts: 1,
        ..DerivationLimits::default()
    };
    let error = derive_all(
        world.relation_state(),
        world.relation_state().manifest().unwrap(),
        limits,
    )
    .unwrap_err();
    assert_eq!(error.code, "derivation.join_attempt_limit");
    assert_eq!(
        world.relation_state().operational_checkpoint_bytes(),
        before
    );
}

#[test]
fn explanation_caps_construction_for_one_large_value() {
    let artifacts = artifacts("relation Note(value: text)\n");
    let mut world = World::new();
    install(&mut world, &artifacts);
    let value = "x".repeat(256 * 1024);
    world
        .apply_relation_transaction(&RelationTransaction {
            operations: vec![insert(
                "game::derived::Note",
                vec![PendingRelationValue::Text(value.clone())],
                OperationMetadata::cause("large-note"),
            )],
            ..RelationTransaction::default()
        })
        .unwrap();
    let fact = FactKey::new("game::derived::Note", vec![FactValue::Text(value)]);
    let explanation = explain_fact(
        &fact,
        world.relation_state(),
        world.derived_relation_state(),
        &crate::causality::CausalityLedger::default(),
    );
    assert!(explanation.len() <= 64 * 1024, "{}", explanation.len());
    assert!(
        explanation.ends_with("… (explanation byte limit reached)"),
        "{}",
        explanation.len()
    );
}
