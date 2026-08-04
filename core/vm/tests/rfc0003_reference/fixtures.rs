// Shared generic schemas, rules, and ownership/encumbrance seed data.

fn entity_column(name: &str) -> ColumnSchema {
    ColumnSchema::new(name, ValueKind::Entity)
}

fn int_column(name: &str) -> ColumnSchema {
    ColumnSchema::new(name, ValueKind::Int)
}

fn count_column(name: &str) -> ColumnSchema {
    ColumnSchema::new(name, ValueKind::Count)
}

fn register_authoritative_schemas(model: &mut WorldModel) {
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
    model
        .relations
        .register(
            RelationSchema::new(
                "ItemWeight",
                vec![entity_column("item").cascade(), int_column("weight")],
            )
            .unique("item", &[0]),
        )
        .unwrap();
    model
        .relations
        .register(
            RelationSchema::new(
                "CarryCapacity",
                vec![entity_column("person").cascade(), int_column("capacity")],
            )
            .unique("person", &[0]),
        )
        .unwrap();
}

fn derived_schemas() -> BTreeMap<String, RelationSchema> {
    [
        RelationSchema::new(
            "TotalWeight",
            vec![entity_column("person"), int_column("total")],
        ),
        RelationSchema::new("Encumbered", vec![entity_column("person")]),
        RelationSchema::new("HasAlly", vec![entity_column("person")]),
        RelationSchema::new(
            "CountAllies",
            vec![entity_column("person"), count_column("count")],
        ),
        RelationSchema::new("Marked", vec![int_column("value")]),
    ]
    .into_iter()
    .map(|schema| (schema.name.clone(), schema))
    .collect()
}

fn ownership_rules() -> Vec<RulePlan> {
    vec![
        RulePlan {
            id: "derive.TotalWeight".to_owned(),
            head_relation: "TotalWeight".to_owned(),
            head: vec![Term::var("person"), Term::var("total")],
            atoms: vec![
                Atom::new("Owns", vec![Term::var("person"), Term::var("item")]),
                Atom::new("ItemWeight", vec![Term::var("item"), Term::var("weight")]),
            ],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind: AggregateKind::Sum,
                input: Some("weight".to_owned()),
                output: "total".to_owned(),
                group_by: vec!["person".to_owned()],
            }),
        },
        RulePlan {
            id: "derive.Encumbered".to_owned(),
            head_relation: "Encumbered".to_owned(),
            head: vec![Term::var("person")],
            atoms: vec![
                Atom::new("TotalWeight", vec![Term::var("person"), Term::var("total")]),
                Atom::new(
                    "CarryCapacity",
                    vec![Term::var("person"), Term::var("capacity")],
                ),
            ],
            predicates: vec![Predicate::Greater(
                "total".to_owned(),
                "capacity".to_owned(),
            )],
            aggregate: None,
        },
    ]
}

fn seed_ownership_model() -> (WorldModel, EntityRef, EntityRef, EntityRef) {
    let mut model = WorldModel::default();
    register_authoritative_schemas(&mut model);
    let person = model.entities.spawn().unwrap();
    let item_a = model.entities.spawn().unwrap();
    let item_b = model.entities.spawn().unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key("Owns", vec![existing(person), existing(item_a)]),
                    "settlement.a",
                ),
                insert(
                    pending_key("Owns", vec![existing(person), existing(item_b)]),
                    "settlement.a",
                ),
                insert(
                    pending_key("ItemWeight", vec![existing(item_a), int(7)]),
                    "settlement.weights",
                ),
                insert(
                    pending_key("ItemWeight", vec![existing(item_b), int(6)]),
                    "settlement.weights",
                ),
                insert(
                    pending_key("CarryCapacity", vec![existing(person), int(10)]),
                    "settlement.capacity",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    (model, person, item_a, item_b)
}
