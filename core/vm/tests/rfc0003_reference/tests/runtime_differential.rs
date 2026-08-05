use super::*;
use rad_vm::relation_derivation as production_derivation;
use rad_vm::relation_frontend::{
    compile, AggregateKind as ProductionAggregateKind, FrontendOptions, Literal, RuleTerm,
    SealedRulePlan,
};
use rad_vm::relation_runtime as runtime;
use rad_vm::world::World;
use std::sync::Arc;

const OWNS: &str = "diff::Owns";
const ALLIED: &str = "diff::Allied";

#[derive(Clone)]
enum Action {
    InsertOwns(usize, usize, &'static str),
    RemoveOwns(usize, usize, &'static str),
    ReplaceOwner(usize, usize, &'static str),
    InsertAllied(usize, usize, &'static str),
    Despawn(usize, &'static str),
}

fn oracle_model() -> (WorldModel, Vec<EntityRef>) {
    let mut model = WorldModel::default();
    model
        .relations
        .register(
            RelationSchema::new(
                OWNS,
                vec![
                    ColumnSchema::new("owner", ValueKind::Entity),
                    ColumnSchema::new("item", ValueKind::Entity).cascade(),
                ],
            )
            .unique("item", &[1]),
        )
        .unwrap();
    model
        .relations
        .register(
            RelationSchema::new(
                ALLIED,
                vec![
                    ColumnSchema::new("left", ValueKind::Entity).cascade(),
                    ColumnSchema::new("right", ValueKind::Entity).cascade(),
                ],
            )
            .symmetric(),
        )
        .unwrap();
    let entities = (0..4).map(|_| model.entities.spawn().unwrap()).collect();
    (model, entities)
}

fn runtime_world() -> (World, Vec<runtime::EntityRef>) {
    let source = r#"
relation Owns(owner: entity, item: entity on delete cascade)
    unique item
relation Allied(left: entity, right: entity)
    symmetric
    on delete cascade
"#;
    let artifacts = compile(
        source,
        &FrontendOptions {
            enabled: true,
            module_id: "diff".into(),
            ..FrontendOptions::default()
        },
    )
    .unwrap();
    let manifest = Arc::new(runtime::RelationRuntimeManifest::from_frontend(&artifacts).unwrap());
    let mut world = World::new();
    world
        .install_relation_manifest(manifest, artifacts.manifest_digest)
        .unwrap();
    let entities = (0..4)
        .map(|_| {
            let slot = world.spawn_entity(None).unwrap();
            world.entity_ref(slot).unwrap()
        })
        .collect();
    (world, entities)
}

fn oracle_transaction(actions: &[Action], entities: &[EntityRef]) -> Transaction {
    let mut transaction = Transaction::default();
    for action in actions {
        match *action {
            Action::InsertOwns(owner, item, cause) => transaction.operations.push(insert(
                pending_key(
                    OWNS,
                    vec![existing(entities[owner]), existing(entities[item])],
                ),
                cause,
            )),
            Action::RemoveOwns(owner, item, cause) => transaction.operations.push(remove(
                pending_key(
                    OWNS,
                    vec![existing(entities[owner]), existing(entities[item])],
                ),
                cause,
            )),
            Action::ReplaceOwner(owner, item, cause) => {
                transaction.operations.push(PendingOperation::ReplaceBy {
                    relation: OWNS.into(),
                    unique_constraint: "item".into(),
                    selected_key: vec![existing(entities[item])],
                    tuple: vec![existing(entities[owner]), existing(entities[item])],
                    metadata: OperationMetadata::cause(cause),
                });
            }
            Action::InsertAllied(left, right, cause) => transaction.operations.push(insert(
                pending_key(
                    ALLIED,
                    vec![existing(entities[left]), existing(entities[right])],
                ),
                cause,
            )),
            Action::Despawn(entity, cause) => {
                transaction.despawns.push(despawn(entities[entity], cause));
            }
        }
    }
    transaction
}

fn runtime_value(entity: runtime::EntityRef) -> runtime::PendingRelationValue {
    runtime::PendingRelationValue::Entity(runtime::EntityOperand::Existing(entity))
}

fn runtime_transaction(
    actions: &[Action],
    entities: &[runtime::EntityRef],
) -> runtime::RelationTransaction {
    let mut transaction = runtime::RelationTransaction::default();
    for action in actions {
        match *action {
            Action::InsertOwns(owner, item, cause) => {
                transaction
                    .operations
                    .push(runtime::PendingRelationOperation::Insert {
                        fact: runtime::PendingFactKey::new(
                            OWNS,
                            vec![
                                runtime_value(entities[owner]),
                                runtime_value(entities[item]),
                            ],
                        ),
                        metadata: runtime::OperationMetadata::cause(cause),
                    });
            }
            Action::RemoveOwns(owner, item, cause) => {
                transaction
                    .operations
                    .push(runtime::PendingRelationOperation::Remove {
                        fact: runtime::PendingFactKey::new(
                            OWNS,
                            vec![
                                runtime_value(entities[owner]),
                                runtime_value(entities[item]),
                            ],
                        ),
                        metadata: runtime::OperationMetadata::cause(cause),
                    });
            }
            Action::ReplaceOwner(owner, item, cause) => {
                transaction
                    .operations
                    .push(runtime::PendingRelationOperation::ReplaceBy {
                        relation: OWNS.into(),
                        unique_constraint: "item".into(),
                        selected_key: vec![runtime_value(entities[item])],
                        tuple: vec![
                            runtime_value(entities[owner]),
                            runtime_value(entities[item]),
                        ],
                        metadata: runtime::OperationMetadata::cause(cause),
                    });
            }
            Action::InsertAllied(left, right, cause) => {
                transaction
                    .operations
                    .push(runtime::PendingRelationOperation::Insert {
                        fact: runtime::PendingFactKey::new(
                            ALLIED,
                            vec![
                                runtime_value(entities[left]),
                                runtime_value(entities[right]),
                            ],
                        ),
                        metadata: runtime::OperationMetadata::cause(cause),
                    });
            }
            Action::Despawn(entity, cause) => {
                transaction.despawns.push(runtime::PendingDespawn {
                    entity: entities[entity],
                    metadata: runtime::OperationMetadata::cause(cause),
                });
            }
        }
    }
    transaction
}

fn oracle_rows(model: &WorldModel) -> Vec<(u64, String, Vec<String>, Vec<String>)> {
    model
        .relations
        .assertions
        .values()
        .map(|assertion| {
            (
                assertion.id,
                assertion.key.relation.clone(),
                assertion.key.tuple.iter().map(oracle_text).collect(),
                assertion.causes.iter().cloned().collect(),
            )
        })
        .collect()
}

fn oracle_text(value: &FactValue) -> String {
    match value {
        FactValue::Entity(entity) => format!("e{}:{}", entity.slot, entity.generation),
        FactValue::Int(value) => format!("i{value}"),
        FactValue::Count(value) => format!("c{value}"),
        FactValue::Text(value) => format!("t{value}"),
    }
}

fn runtime_rows(world: &World) -> Vec<(u64, String, Vec<String>, Vec<String>)> {
    world
        .relation_state()
        .assertions()
        .values()
        .map(|assertion| {
            (
                assertion.assertion_id,
                assertion.fact_key.relation.clone(),
                assertion.fact_key.tuple.iter().map(runtime_text).collect(),
                assertion.causes.iter().cloned().collect(),
            )
        })
        .collect()
}

fn runtime_text(value: &runtime::FactValue) -> String {
    match value {
        runtime::FactValue::Entity(entity) => format!("e{}:{}", entity.slot, entity.generation),
        runtime::FactValue::Int(value) => format!("i{value}"),
        runtime::FactValue::Count(value) => format!("c{value}"),
        runtime::FactValue::Text(value) => format!("t{value}"),
    }
}

fn oracle_rule(plan: &SealedRulePlan) -> RulePlan {
    fn term(value: &RuleTerm) -> Term {
        match value {
            RuleTerm::Variable(name) => Term::Variable(name.clone()),
            RuleTerm::Literal(Literal::Int(value)) => Term::Constant(FactValue::Int(*value)),
            RuleTerm::Literal(Literal::Count(value)) => Term::Constant(FactValue::Count(*value)),
            RuleTerm::Literal(Literal::Text(value)) => {
                Term::Constant(FactValue::Text(value.clone()))
            }
        }
    }

    let typed = plan.typed_plan();
    RulePlan {
        id: plan.identity().to_owned(),
        head_relation: typed.head_relation.clone(),
        head: typed.head.iter().map(term).collect(),
        atoms: typed
            .atoms
            .iter()
            .map(|atom| Atom::new(&atom.relation, atom.terms.iter().map(term).collect()))
            .collect(),
        predicates: typed
            .predicates
            .iter()
            .map(|predicate| match predicate {
                rad_vm::relation_frontend::RulePredicate::Greater(left, right) => {
                    Predicate::Greater(left.clone(), right.clone())
                }
            })
            .collect(),
        aggregate: typed.aggregate.as_ref().map(|aggregate| AggregateSpec {
            kind: match aggregate.kind {
                ProductionAggregateKind::Count => AggregateKind::Count,
                ProductionAggregateKind::Sum => AggregateKind::Sum,
                ProductionAggregateKind::Min => AggregateKind::Min,
                ProductionAggregateKind::Max => AggregateKind::Max,
            },
            input: aggregate.input.clone(),
            output: aggregate.output.clone(),
            group_by: aggregate.group_by.clone(),
        }),
    }
}

fn permutations(actions: &[Action]) -> Vec<Vec<Action>> {
    fn build(prefix: Vec<Action>, rest: Vec<Action>, out: &mut Vec<Vec<Action>>) {
        if rest.is_empty() {
            out.push(prefix);
            return;
        }
        for index in 0..rest.len() {
            let mut next_prefix = prefix.clone();
            next_prefix.push(rest[index].clone());
            let mut next_rest = rest.clone();
            next_rest.remove(index);
            build(next_prefix, next_rest, out);
        }
    }
    let mut out = Vec::new();
    build(Vec::new(), actions.to_vec(), &mut out);
    out
}

fn compare(actions: &[Action]) -> (Result<(), &'static str>, Result<(), &'static str>) {
    let (mut oracle, oracle_entities) = oracle_model();
    let (mut runtime, runtime_entities) = runtime_world();
    let oracle_result = oracle
        .apply_transaction(oracle_transaction(actions, &oracle_entities))
        .map(|_| ());
    let runtime_result = runtime
        .apply_relation_transaction(&runtime_transaction(actions, &runtime_entities))
        .map(|_| ())
        .map_err(|error| error.code);
    assert_eq!(oracle_result, runtime_result);
    assert_eq!(oracle_rows(&oracle), runtime_rows(&runtime));
    assert_eq!(
        oracle.relations.next_assertion_id,
        runtime.relation_state().next_assertion_id()
    );
    (oracle_result, runtime_result)
}

#[test]
fn production_authoritative_runtime_matches_the_accepted_oracle() {
    let independent = vec![
        Action::InsertOwns(0, 2, "a"),
        Action::InsertOwns(1, 3, "b"),
        Action::InsertAllied(0, 1, "allied-left"),
        Action::InsertAllied(1, 0, "allied-right"),
    ];
    for permutation in permutations(&independent) {
        assert!(compare(&permutation).0.is_ok());
    }

    let conflict = vec![Action::InsertOwns(0, 2, "a"), Action::InsertOwns(1, 2, "b")];
    for permutation in permutations(&conflict) {
        assert_eq!(compare(&permutation).0, Err("relation.unique_conflict"));
    }

    assert!(compare(&[
        Action::InsertOwns(0, 2, "base"),
        Action::RemoveOwns(0, 2, "remove"),
    ])
    .0
    .is_err());
    assert!(compare(&[
        Action::InsertOwns(0, 2, "base"),
        Action::Despawn(2, "cascade"),
    ])
    .0
    .is_ok());
}

#[test]
fn replacement_lifetime_matches_the_accepted_oracle() {
    let (mut oracle, oracle_entities) = oracle_model();
    let (mut runtime, runtime_entities) = runtime_world();
    oracle
        .apply_transaction(oracle_transaction(
            &[Action::InsertOwns(0, 2, "base")],
            &oracle_entities,
        ))
        .unwrap();
    runtime
        .apply_relation_transaction(&runtime_transaction(
            &[Action::InsertOwns(0, 2, "base")],
            &runtime_entities,
        ))
        .unwrap();
    oracle
        .apply_transaction(oracle_transaction(
            &[Action::ReplaceOwner(1, 2, "transfer")],
            &oracle_entities,
        ))
        .unwrap();
    runtime
        .apply_relation_transaction(&runtime_transaction(
            &[Action::ReplaceOwner(1, 2, "transfer")],
            &runtime_entities,
        ))
        .unwrap();
    assert_eq!(oracle_rows(&oracle), runtime_rows(&runtime));
    assert_eq!(
        oracle.relations.next_assertion_id,
        runtime.relation_state().next_assertion_id()
    );
}

#[test]
fn production_full_recompute_derivation_matches_the_accepted_oracle_bytes() {
    const VISIBLE: &str = "diff::Visible";
    const HIDDEN: &str = "diff::Hidden";
    const MARKED: &str = "diff::Marked";
    const COUNTED: &str = "diff::Counted";
    const PUBLIC: &str = "diff::Public";

    let source = r#"
relation Visible(value: int)
relation Hidden(value: int)
derive Marked(value)
    when Visible(value)
derive Marked(value)
    when Hidden(value)
derive Counted(count())
    when Marked(value)
derive Public(count)
    when Counted(count)
"#;
    let artifacts = compile(
        source,
        &FrontendOptions {
            enabled: true,
            module_id: "diff".into(),
            ..FrontendOptions::default()
        },
    )
    .unwrap();

    let mut oracle = WorldModel::default();
    oracle
        .relations
        .register(RelationSchema::new(
            VISIBLE,
            vec![ColumnSchema::new("value", ValueKind::Int)],
        ))
        .unwrap();
    oracle
        .relations
        .register(RelationSchema::new(
            HIDDEN,
            vec![ColumnSchema::new("value", ValueKind::Int)],
        ))
        .unwrap();
    oracle
        .apply_transaction(Transaction {
            operations: vec![
                insert(pending_key(VISIBLE, vec![int(7)]), "visible"),
                PendingOperation::Insert(
                    pending_key(HIDDEN, vec![int(7)]),
                    OperationMetadata::cause("hidden").with_capability("secret"),
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();

    let derived_schemas = [
        RelationSchema::new(MARKED, vec![ColumnSchema::new("value", ValueKind::Int)]),
        RelationSchema::new(COUNTED, vec![ColumnSchema::new("count", ValueKind::Count)]),
        RelationSchema::new(PUBLIC, vec![ColumnSchema::new("count", ValueKind::Count)]),
    ]
    .into_iter()
    .map(|schema| (schema.name.clone(), schema))
    .collect::<BTreeMap<_, _>>();
    let oracle_rules = artifacts
        .rules
        .iter()
        .map(|plan| oracle_rule(plan))
        .collect::<Vec<_>>();
    let oracle_derived = derive_all(
        &oracle.relations,
        &derived_schemas,
        &oracle_rules,
        DerivationLimits::generous(),
    )
    .unwrap();

    let manifest = Arc::new(runtime::RelationRuntimeManifest::from_frontend(&artifacts).unwrap());
    let mut world = World::new();
    world
        .install_relation_manifest(manifest, artifacts.manifest_digest)
        .unwrap();
    world
        .apply_relation_transaction(&runtime::RelationTransaction {
            operations: vec![
                runtime::PendingRelationOperation::Insert {
                    fact: runtime::PendingFactKey::new(
                        VISIBLE,
                        vec![runtime::PendingRelationValue::Int(7)],
                    ),
                    metadata: runtime::OperationMetadata::cause("visible"),
                },
                runtime::PendingRelationOperation::Insert {
                    fact: runtime::PendingFactKey::new(
                        HIDDEN,
                        vec![runtime::PendingRelationValue::Int(7)],
                    ),
                    metadata: runtime::OperationMetadata::cause("hidden").with_capability("secret"),
                },
            ],
            ..runtime::RelationTransaction::default()
        })
        .unwrap();
    let production_derived = production_derivation::derive_all(
        world.relation_state(),
        world.relation_state().manifest().unwrap(),
        production_derivation::DerivationLimits::default(),
    )
    .unwrap();

    assert_eq!(
        production_derived.canonical_bytes(),
        canonical_derivation_bytes(&oracle_derived)
    );
}
