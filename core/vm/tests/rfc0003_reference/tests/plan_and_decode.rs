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
    let assert_permutations = |rules: Vec<RulePlan>,
                               limits: RulePlanLimits,
                               expected: RuleDiagnosticCode| {
        let forward =
            select_rule_diagnostic(&rules, RawRuleInputLimits::generous(), limits).unwrap();
        let mut reverse_rules = rules.clone();
        reverse_rules.reverse();
        let reverse =
            select_rule_diagnostic(&reverse_rules, RawRuleInputLimits::generous(), limits).unwrap();
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
            select_rule_diagnostic(&rules, RawRuleInputLimits::generous(), three_limits).unwrap()
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
fn raw_rule_envelope_bounds_hostile_input_before_complete_fingerprinting() {
    let plan = |id: &str| RulePlan {
        id: id.to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new("Input", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    let sealed = RulePlanLimits::generous();

    let mut raw = RawRuleInputLimits::generous();
    raw.max_rules = 1;
    let million_rule_preflight = analyze_rule_diagnostics(
        &[],
        RawRuleInputStats {
            rules_seen: 1_000_000,
            ..RawRuleInputStats::default()
        },
        raw,
        sealed,
    );
    assert_eq!(
        million_rule_preflight.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawRuleLimit
    );
    assert_eq!(million_rule_preflight.usage, RawValidationUsage::default());

    raw = RawRuleInputLimits::generous();
    raw.max_source_bytes = 8;
    let source_preflight = analyze_rule_diagnostics(
        &[],
        RawRuleInputStats {
            source_bytes: usize::MAX,
            ..RawRuleInputStats::default()
        },
        raw,
        sealed,
    );
    assert_eq!(
        source_preflight.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawSourceByteLimit
    );
    assert_eq!(source_preflight.usage.complete_fingerprints, 0);

    raw = RawRuleInputLimits::generous();
    raw.max_tokens = 4;
    let token_preflight = analyze_rule_diagnostics(
        &[],
        RawRuleInputStats {
            tokens: usize::MAX,
            ..RawRuleInputStats::default()
        },
        raw,
        sealed,
    );
    assert_eq!(
        token_preflight.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawTokenLimit
    );
    assert_eq!(token_preflight.usage.complete_fingerprints, 0);

    let mut enormous_body = plan("");
    enormous_body.atoms = (0..10_000)
        .map(|index| Atom::new(&format!("Input{index}"), vec![Term::var("value")]))
        .collect();
    raw = RawRuleInputLimits::generous();
    raw.max_atoms_per_rule = 1;
    let empty_id_wins = analyze_rule_diagnostics(
        std::slice::from_ref(&enormous_body),
        RawRuleInputStats::for_rules(std::slice::from_ref(&enormous_body)),
        raw,
        sealed,
    );
    assert_eq!(
        empty_id_wins.diagnostic.unwrap().code,
        RuleDiagnosticCode::EmptyId
    );
    assert_eq!(empty_id_wins.usage.complete_fingerprints, 0);

    enormous_body.id = "rules.enormous".to_owned();
    let atom_limit = analyze_rule_diagnostics(
        std::slice::from_ref(&enormous_body),
        RawRuleInputStats::for_rules(std::slice::from_ref(&enormous_body)),
        raw,
        sealed,
    );
    assert_eq!(
        atom_limit.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawAtomLimit
    );
    assert_eq!(atom_limit.usage.complete_fingerprints, 0);
    assert!(atom_limit.usage.metadata_visits <= raw.max_validation_node_visits);

    let enormous_identifier = plan(&format!("rules.{}", "x".repeat(100_000)));
    raw = RawRuleInputLimits::generous();
    raw.max_identifier_bytes = 32;
    let identifier_limit = analyze_rule_diagnostics(
        std::slice::from_ref(&enormous_identifier),
        RawRuleInputStats::for_rules(std::slice::from_ref(&enormous_identifier)),
        raw,
        sealed,
    );
    assert_eq!(
        identifier_limit.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawIdentifierByteLimit
    );
    assert_eq!(identifier_limit.usage.complete_fingerprints, 0);

    let mut enormous_groups = plan("rules.groups");
    enormous_groups.aggregate = Some(AggregateSpec {
        kind: AggregateKind::Count,
        input: None,
        output: "count".to_owned(),
        group_by: vec!["value".to_owned(); 10_000],
    });
    raw = RawRuleInputLimits::generous();
    raw.max_aggregate_groups_per_rule = 1;
    let group_limit = analyze_rule_diagnostics(
        std::slice::from_ref(&enormous_groups),
        RawRuleInputStats::for_rules(std::slice::from_ref(&enormous_groups)),
        raw,
        sealed,
    );
    assert_eq!(
        group_limit.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawAggregateGroupLimit
    );
    assert_eq!(group_limit.usage.complete_fingerprints, 0);

    let bounded = plan("rules.bounded");
    raw = RawRuleInputLimits::generous();
    raw.max_ast_nodes = 1;
    let node_limit = analyze_rule_diagnostics(
        std::slice::from_ref(&bounded),
        RawRuleInputStats::for_rules(std::slice::from_ref(&bounded)),
        raw,
        sealed,
    );
    assert_eq!(
        node_limit.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawAstNodeLimit
    );
    assert_eq!(node_limit.usage.complete_fingerprints, 0);

    raw = RawRuleInputLimits::generous();
    raw.max_total_structural_cost = 1;
    let structural_limit = analyze_rule_diagnostics(
        std::slice::from_ref(&bounded),
        RawRuleInputStats::for_rules(std::slice::from_ref(&bounded)),
        raw,
        sealed,
    );
    assert_eq!(
        structural_limit.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawStructuralCostLimit
    );
    assert_eq!(structural_limit.usage.complete_fingerprints, 0);

    raw = RawRuleInputLimits::generous();
    raw.max_validation_node_visits = 1;
    let work_limit = analyze_rule_diagnostics(
        std::slice::from_ref(&bounded),
        RawRuleInputStats::for_rules(std::slice::from_ref(&bounded)),
        raw,
        sealed,
    );
    assert_eq!(
        work_limit.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawValidationWorkLimit
    );
    assert_eq!(work_limit.usage.complete_fingerprints, 0);

    let mut predicate_heavy = plan("rules.predicates");
    predicate_heavy.predicates = vec![
        Predicate::Greater("value".to_owned(), "value".to_owned()),
        Predicate::Greater("value".to_owned(), "value".to_owned()),
    ];
    let mut atom_heavy = plan("rules.atoms");
    atom_heavy
        .atoms
        .push(Atom::new("Other", vec![Term::var("value")]));
    raw = RawRuleInputLimits::generous();
    raw.max_atoms_per_rule = 1;
    raw.max_predicates_per_rule = 1;
    let forward = analyze_rule_diagnostics(
        &[predicate_heavy.clone(), atom_heavy.clone()],
        RawRuleInputStats {
            rules_seen: 2,
            ..RawRuleInputStats::default()
        },
        raw,
        sealed,
    );
    let reverse = analyze_rule_diagnostics(
        &[atom_heavy, predicate_heavy],
        RawRuleInputStats {
            rules_seen: 2,
            ..RawRuleInputStats::default()
        },
        raw,
        sealed,
    );
    assert_eq!(forward.diagnostic, reverse.diagnostic);
    assert_eq!(
        forward.diagnostic.unwrap().code,
        RuleDiagnosticCode::RawAtomLimit
    );
}

#[test]
fn diagnostic_witnesses_bind_exact_duplicate_groups_and_digest_multisets() {
    let plan = |relation: &str| RulePlan {
        id: "rules.duplicate".to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new(relation, vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    let a = plan("A");
    let b = plan("B");
    let c = plan("C");
    let diagnose = |rules: &[RulePlan]| {
        select_rule_diagnostic(
            rules,
            RawRuleInputLimits::generous(),
            RulePlanLimits::generous(),
        )
        .unwrap()
    };
    let ab = diagnose(&[a.clone(), b.clone()]);
    let ba = diagnose(&[b.clone(), a.clone()]);
    let ac = diagnose(&[a.clone(), c]);
    assert_eq!(ab.code, RuleDiagnosticCode::DuplicateId);
    assert_eq!(ab, ba);
    assert_ne!(ab.rule_fingerprint, ac.rule_fingerprint);

    let mut reordered = a.clone();
    reordered
        .atoms
        .push(Atom::new("B", vec![Term::var("value")]));
    let fingerprint = raw_rule_fingerprint(&reordered);
    reordered.atoms.reverse();
    assert_eq!(raw_rule_fingerprint(&reordered), fingerprint);
    reordered.atoms.push(reordered.atoms[0].clone());
    assert_ne!(raw_rule_fingerprint(&reordered), fingerprint);

    assert!(RuleDiagnosticCode::EmptyId.priority() < RuleDiagnosticCode::UnqualifiedId.priority());
    assert!(RuleDiagnosticCode::RuleLimit.priority() < RuleDiagnosticCode::DuplicateId.priority());
    assert!(RuleDiagnosticCode::DuplicateId.priority() < RuleDiagnosticCode::AtomLimit.priority());
    assert!(
        RuleDiagnosticCode::AtomLimit.priority() < RuleDiagnosticCode::PredicateLimit.priority()
    );
    assert!(
        RuleDiagnosticCode::PredicateLimit.priority() < RuleDiagnosticCode::TermLimit.priority()
    );
    assert!(
        RuleDiagnosticCode::TermLimit.priority()
            < RuleDiagnosticCode::CanonicalByteLimit.priority()
    );
    assert!(
        RuleDiagnosticCode::CanonicalByteLimit.priority()
            < RuleDiagnosticCode::DependencyEdgeLimit.priority()
    );
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
