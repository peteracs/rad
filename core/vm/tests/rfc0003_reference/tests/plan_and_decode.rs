//! Sealed-plan diagnostics and bounded canonical decoding contracts.

use super::*;

#[test]
fn canonical_rule_plans_make_resource_failures_permutation_invariant() {
    let mut model = WorldModel::default();
    for relation in ["One", "TwoLeft", "TwoRight"] {
        model
            .relations
            .register(RelationSchema::new(relation, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(pending_key("One", vec![int(1)]), "one"),
                insert(pending_key("TwoLeft", vec![int(2)]), "left"),
                insert(pending_key("TwoRight", vec![int(2)]), "right"),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let schemas = BTreeMap::from([(
        "Out".to_owned(),
        RelationSchema::new("Out", vec![int_column("value")]),
    )]);
    let one = RulePlan {
        id: "a.one".to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new("One", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    let two = RulePlan {
        id: "b.two".to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![
            Atom::new("TwoLeft", vec![Term::var("value")]),
            Atom::new("TwoRight", vec![Term::var("value")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut limits = DerivationLimits::generous();
    limits.max_facts = 1;
    limits.max_support_nodes = 1;
    let forward = derive_all(
        &model.relations,
        &schemas,
        &[one.clone(), two.clone()],
        limits,
    );
    let reverse = derive_all(
        &model.relations,
        &schemas,
        &[two.clone(), one.clone()],
        limits,
    );
    assert_eq!(forward, Err("derivation.fact_limit"));
    assert_eq!(reverse, forward);

    let mut two_reversed = two.clone();
    two_reversed.atoms.reverse();
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[one.clone(), two],
            DerivationLimits::generous(),
        ),
        derive_all(
            &model.relations,
            &schemas,
            &[two_reversed, one.clone()],
            DerivationLimits::generous(),
        )
    );
    let mut duplicate = one.clone();
    duplicate.head = vec![Term::Constant(FactValue::Int(9))];
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[one.clone(), duplicate],
            DerivationLimits::generous(),
        ),
        Err("derivation.duplicate_rule_id")
    );
    let mut empty_id = one;
    empty_id.id.clear();
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[empty_id],
            DerivationLimits::generous(),
        ),
        Err("derivation.empty_rule_id")
    );
    let mut unqualified = RulePlan {
        id: "a.one".to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::Constant(FactValue::Int(1))],
        atoms: vec![Atom::new("One", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    unqualified.id = "one".to_owned();
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[unqualified],
            DerivationLimits::generous(),
        ),
        Err("derivation.unqualified_rule_id")
    );

    let collision = BTreeMap::from([(
        "One".to_owned(),
        RelationSchema::new("One", vec![int_column("value")]),
    )]);
    assert_eq!(
        derive_all(
            &model.relations,
            &collision,
            &[],
            DerivationLimits::generous(),
        ),
        Err("derivation.relation_namespace_collision")
    );
}

#[test]
fn invalid_rule_diagnostics_are_canonical_under_every_registration_order() {
    let plan = |id: &str, atoms: usize, predicates: usize| RulePlan {
        id: id.to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::var("value")],
        atoms: (0..atoms)
            .map(|index| Atom::new(&format!("Input{index}"), vec![Term::var("value")]))
            .collect(),
        predicates: (0..predicates)
            .map(|_| Predicate::Greater("value".to_owned(), "value".to_owned()))
            .collect(),
        aggregate: None,
    };
    let assert_permutations =
        |rules: Vec<RulePlan>, limits: RulePlanLimits, expected: RuleDiagnosticCode| {
            let forward = select_rule_diagnostic(&rules, limits).unwrap();
            let mut reverse_rules = rules.clone();
            reverse_rules.reverse();
            let reverse = select_rule_diagnostic(&reverse_rules, limits).unwrap();
            assert_eq!(forward, reverse);
            assert_eq!(forward.code, expected);
            let schemas = BTreeMap::from([(
                "Out".to_owned(),
                RelationSchema::new("Out", vec![int_column("value")]),
            )]);
            let mut derivation_limits = DerivationLimits::generous();
            derivation_limits.rule_plans = limits;
            let forward_error = derive_all(
                &RelationStore::default(),
                &schemas,
                &rules,
                derivation_limits,
            );
            let reverse_error = derive_all(
                &RelationStore::default(),
                &schemas,
                &reverse_rules,
                derivation_limits,
            );
            assert_eq!(forward_error, Err(expected.error()));
            assert_eq!(reverse_error, forward_error);
        };

    assert_permutations(
        vec![plan("", 1, 0), plan("unqualified", 1, 0)],
        RulePlanLimits::generous(),
        RuleDiagnosticCode::EmptyId,
    );

    let mut overlapping = RulePlanLimits::generous();
    overlapping.max_atoms_per_rule = 1;
    overlapping.max_predicates_per_rule = 0;
    assert_permutations(
        vec![plan("rules.atom", 2, 0), plan("rules.predicate", 1, 1)],
        overlapping,
        RuleDiagnosticCode::AtomLimit,
    );

    overlapping.max_predicates_per_rule = usize::MAX;
    overlapping.max_terms = 1;
    assert_permutations(
        vec![plan("rules.atom", 2, 0), plan("rules.terms", 1, 0)],
        overlapping,
        RuleDiagnosticCode::AtomLimit,
    );

    let byte_plan = plan("rules.bytes", 1, 0);
    let mut totals = RulePlanLimits::generous();
    totals.max_terms = 1;
    totals.max_canonical_plan_bytes = byte_plan.canonical_len().unwrap() - 1;
    assert_permutations(
        vec![byte_plan, plan("rules.terms", 1, 0)],
        totals,
        RuleDiagnosticCode::TermLimit,
    );

    assert_permutations(
        vec![plan("rules.duplicate", 1, 0), plan("rules.duplicate", 2, 0)],
        RulePlanLimits::generous(),
        RuleDiagnosticCode::DuplicateId,
    );

    let three = [
        plan("", 1, 0),
        plan("rules.atom", 2, 0),
        plan("rules.predicate", 1, 1),
    ];
    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut three_limits = RulePlanLimits::generous();
    three_limits.max_atoms_per_rule = 1;
    three_limits.max_predicates_per_rule = 0;
    let expected = permutations
        .iter()
        .map(|order| {
            let rules = order
                .iter()
                .map(|index| three[*index].clone())
                .collect::<Vec<_>>();
            select_rule_diagnostic(&rules, three_limits).unwrap()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 1);
    assert_eq!(
        expected.iter().next().unwrap().code,
        RuleDiagnosticCode::EmptyId
    );

    let mut atom_order = plan("rules.atom_order", 2, 0);
    let fingerprint = raw_rule_fingerprint(&atom_order);
    atom_order.atoms.reverse();
    assert_eq!(raw_rule_fingerprint(&atom_order), fingerprint);
}

#[test]
fn sealed_rule_plan_limits_bound_static_predicate_and_dependency_work() {
    let (model, _, _, _) = seed_ownership_model();
    let schemas = derived_schemas();
    let rules = ownership_rules();
    let canonical_bytes = rules
        .iter()
        .map(|rule| {
            let encoded = rule.canonical_bytes();
            assert_eq!(rule.canonical_len().unwrap(), encoded.len());
            encoded.len()
        })
        .sum::<usize>();

    let mut exact = DerivationLimits::generous();
    exact.rule_plans.max_rules = rules.len();
    exact.rule_plans.max_atoms_per_rule = rules.iter().map(|rule| rule.atoms.len()).max().unwrap();
    exact.rule_plans.max_predicates_per_rule = rules
        .iter()
        .map(|rule| rule.predicates.len())
        .max()
        .unwrap();
    exact.rule_plans.max_terms = rules
        .iter()
        .map(|rule| {
            rule.head.len()
                + rule
                    .atoms
                    .iter()
                    .map(|atom| atom.terms.len())
                    .sum::<usize>()
        })
        .sum();
    exact.rule_plans.max_canonical_plan_bytes = canonical_bytes;
    exact.rule_plans.max_dependency_edges = rules
        .iter()
        .flat_map(|rule| {
            rule.atoms
                .iter()
                .map(move |atom| (&rule.head_relation, &atom.relation))
        })
        .collect::<BTreeSet<_>>()
        .len();
    assert!(derive_all(&model.relations, &schemas, &rules, exact).is_ok());

    for (constrained, error) in [
        (
            {
                let mut value = exact;
                value.rule_plans.max_rules -= 1;
                value
            },
            "derivation.rule_limit",
        ),
        (
            {
                let mut value = exact;
                value.rule_plans.max_atoms_per_rule = 0;
                value
            },
            "derivation.atom_limit",
        ),
        (
            {
                let mut value = exact;
                value.rule_plans.max_predicates_per_rule = 0;
                value
            },
            "derivation.predicate_limit",
        ),
        (
            {
                let mut value = exact;
                value.rule_plans.max_terms = 0;
                value
            },
            "derivation.term_limit",
        ),
        (
            {
                let mut value = exact;
                value.rule_plans.max_canonical_plan_bytes -= 1;
                value
            },
            "derivation.rule_plan_byte_limit",
        ),
        (
            {
                let mut value = exact;
                value.rule_plans.max_dependency_edges = 0;
                value
            },
            "derivation.dependency_edge_limit",
        ),
    ] {
        assert_eq!(
            derive_all(&model.relations, &schemas, &rules, constrained),
            Err(error)
        );
    }
}

#[test]
fn semantic_wire_structural_limits_reject_before_oversized_retention() {
    let schema = RelationSchema::new("Textual", vec![ColumnSchema::new("value", ValueKind::Text)]);
    let schemas = BTreeMap::from([("Textual".to_owned(), schema)]);
    let fact = FactKey::new("Textual", vec![FactValue::Text("payload".to_owned())]);
    let mut bytes = b"rfc0003.semantic.v1".to_vec();
    write_u64(&mut bytes, 1);
    encode_fact_key(&mut bytes, &fact);
    let structural_bytes = std::mem::size_of::<FactKey>()
        + 2 * std::mem::size_of::<String>()
        + std::mem::size_of::<FactValue>()
        + "Textual".len()
        + "payload".len();
    let exact = DecodeLimits {
        max_input_bytes: bytes.len(),
        max_facts: 1,
        max_values: 1,
        max_text_bytes: "Textual".len() + "payload".len(),
        max_structural_bytes: structural_bytes,
    };
    assert_eq!(
        decode_semantic_relation_bytes(&bytes, &schemas, &EntityTable::default(), exact).unwrap(),
        BTreeSet::from([fact])
    );

    for (limits, error) in [
        (
            DecodeLimits {
                max_input_bytes: bytes.len() - 1,
                ..exact
            },
            "wire.input_byte_limit",
        ),
        (
            DecodeLimits {
                max_facts: 0,
                ..exact
            },
            "wire.fact_limit",
        ),
        (
            DecodeLimits {
                max_values: 0,
                ..exact
            },
            "wire.value_limit",
        ),
        (
            DecodeLimits {
                max_text_bytes: exact.max_text_bytes - 1,
                ..exact
            },
            "wire.text_byte_limit",
        ),
        (
            DecodeLimits {
                max_structural_bytes: structural_bytes - 1,
                ..exact
            },
            "wire.structural_byte_limit",
        ),
    ] {
        assert_eq!(
            decode_semantic_relation_bytes(&bytes, &schemas, &EntityTable::default(), limits,),
            Err(error)
        );
    }
}
