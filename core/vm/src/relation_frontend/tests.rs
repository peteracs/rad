use super::*;

fn enabled() -> FrontendOptions {
    FrontendOptions {
        enabled: true,
        module_id: "game::inventory".to_string(),
        ..FrontendOptions::default()
    }
}

const OWNERSHIP: &str = r#"
relation Owns(owner: entity, item: entity)
    unique item

relation ItemWeight(item: entity, weight: int)
    unique item

relation CarryCapacity(person: entity, capacity: int)
    unique person

relation AlliedWith(left: entity, right: entity)
    symmetric
    on delete cascade

derive TotalWeight(person, sum(weight))
    when Owns(person, item)
    and ItemWeight(item, weight)

derive Encumbered(person)
    when TotalWeight(person, total)
    and CarryCapacity(person, capacity)
    and total > capacity
"#;

#[test]
fn feature_gate_precedes_relation_parsing() {
    let error = compile(OWNERSHIP, &FrontendOptions::default()).unwrap_err();
    assert_eq!(error[0].code, DiagnosticCode::FeatureDisabled);
}

#[test]
fn syntax_diagnostics_report_token_start_positions() {
    let source = "relation Broken(value int)\n";
    let error = compile(source, &enabled()).unwrap_err().remove(0);
    assert_eq!(error.code, DiagnosticCode::Syntax);
    assert_eq!(error.line, 1);
    assert_eq!(error.column as usize, source.find("int").unwrap() + 1);
}

#[test]
fn accepted_rfc_examples_emit_sealed_frontend_artifacts() {
    let artifacts = compile(OWNERSHIP, &enabled()).unwrap();
    assert_eq!(artifacts.relations.schemas().len(), 6);
    assert_eq!(artifacts.rules.len(), 2);
    assert_eq!(artifacts.dependency_dag.edges().len(), 1);
    assert!(artifacts
        .relations
        .schemas()
        .iter()
        .any(|schema| schema.identity == "game::inventory::AlliedWith" && schema.symmetric));
    for rule in artifacts.rules.iter() {
        assert!(rule.identity().starts_with("game::inventory::rule::"));
        assert_eq!(rule.digest(), canonical::digest(rule.canonical_bytes()));
        assert!(!rule.canonical_bytes().is_empty());
    }
}

#[test]
fn authoritative_operations_are_typed_frontend_artifacts_only() {
    let source = r#"
relation Owns(owner: entity, item: entity)
    unique item
Insert(Owns, (alice, sword))
Remove(Owns, (alice, sword))
ReplaceBy(Owns, item, sword, (bob, sword))
"#;
    let artifacts = compile(source, &enabled()).unwrap();
    assert_eq!(artifacts.operations.len(), 3);
    assert!(matches!(
        artifacts.operations[0].kind,
        RelationOperationKind::Insert
    ));
    assert!(matches!(
        artifacts.operations[2].kind,
        RelationOperationKind::ReplaceBy { .. }
    ));
}

#[test]
fn replace_by_checks_single_and_composite_unique_keys() {
    let source = r#"
relation Placement(owner: entity, item: entity, slot: int)
    unique item
    unique owner, slot as owner_slot
ReplaceBy(Placement, owner_slot, (alice, 2), (alice, sword, 2))
"#;
    let artifacts = compile(source, &enabled()).unwrap();
    let RelationOperationKind::ReplaceBy { key, .. } = &artifacts.operations[0].kind else {
        panic!("expected ReplaceBy");
    };
    assert_eq!(key.len(), 2);
    let formatted = format_source(source, &enabled()).unwrap();
    assert_eq!(
        compile(&formatted, &enabled()).unwrap().manifest_digest,
        artifacts.manifest_digest
    );

    let wrong_key = source.replace("(alice, 2), (alice", "alice, (alice");
    assert_eq!(
        compile(&wrong_key, &enabled()).unwrap_err()[0].code,
        DiagnosticCode::Arity
    );
}

#[test]
fn parser_owned_rules_carry_exact_read_only_summaries() {
    let raw = parse_bounded(OWNERSHIP, &enabled()).unwrap();
    let summaries = raw.rule_summaries().collect::<Vec<_>>();
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].atoms, 2);
    assert_eq!(summaries[0].head_terms, 2);
    assert_eq!(summaries[0].aggregate_groups, 1);
    assert!(
        summaries
            .iter()
            .map(|summary| summary.maximum_identifier_length)
            .max()
            .unwrap()
            >= "CarryCapacity".len()
    );
    assert!(raw.input_stats().ast_nodes >= summaries.iter().map(|s| s.ast_nodes).sum());
}

#[test]
fn raw_limits_reject_during_bounded_construction() {
    let mut options = enabled();
    options.raw_limits.max_source_bytes = 8;
    assert_eq!(
        compile(OWNERSHIP, &options).unwrap_err()[0].code,
        DiagnosticCode::RawSourceByteLimit
    );

    options = enabled();
    options.raw_limits.max_tokens = 4;
    assert_eq!(
        compile(OWNERSHIP, &options).unwrap_err()[0].code,
        DiagnosticCode::RawTokenLimit
    );

    options = enabled();
    options.raw_limits.max_identifier_bytes = 8;
    assert_eq!(
        compile(OWNERSHIP, &options).unwrap_err()[0].code,
        DiagnosticCode::RawIdentifierByteLimit
    );

    options = enabled();
    options.raw_limits.max_terms_per_rule = 1;
    options.raw_limits.max_atoms_per_rule = 0;
    options.raw_limits.max_predicates_per_rule = 0;
    options.raw_limits.max_aggregate_groups_per_rule = 0;
    assert_eq!(
        compile(OWNERSHIP, &options).unwrap_err()[0].code,
        DiagnosticCode::RawTermLimit
    );
}

#[test]
fn declaration_and_rule_permutations_have_one_manifest_identity() {
    let permuted = r#"
relation AlliedWith(left: entity, right: entity)
    on delete cascade
    symmetric
relation CarryCapacity(person: entity, capacity: int)
    unique person
relation ItemWeight(item: entity, weight: int)
    unique item
relation Owns(owner: entity, item: entity)
    unique item
derive Encumbered(person)
    when CarryCapacity(person, capacity)
    and TotalWeight(person, total)
    and total > capacity
derive TotalWeight(person, sum(weight))
    when ItemWeight(item, weight)
    and Owns(person, item)
"#;
    let original = compile(OWNERSHIP, &enabled()).unwrap();
    let permuted = compile(permuted, &enabled()).unwrap();
    assert_eq!(original.manifest_digest, permuted.manifest_digest);
    let original_rules = original
        .rules
        .iter()
        .map(|rule| rule.digest())
        .collect::<Vec<_>>();
    let permuted_rules = permuted
        .rules
        .iter()
        .map(|rule| rule.digest())
        .collect::<Vec<_>>();
    assert_eq!(original_rules, permuted_rules);
}

#[test]
fn module_load_permutations_have_one_manifest_and_rule_identity() {
    let items = SourceModule {
        module_id: "game::items",
        source: r#"
relation ItemWeight(item: entity, weight: int)
    unique item
"#,
    };
    let inventory = SourceModule {
        module_id: "game::inventory",
        source: r#"
relation Owns(owner: entity, item: entity)
    unique item
derive TotalWeight(person, sum(weight))
    when Owns(person, item)
    and game::items::ItemWeight(item, weight)
"#,
    };
    let forward = compile_modules(&[items, inventory], &enabled()).unwrap();
    let reverse = compile_modules(&[inventory, items], &enabled()).unwrap();
    assert_eq!(forward.manifest_digest, reverse.manifest_digest);
    assert_eq!(
        forward.modules.as_ref(),
        &["game::inventory".to_string(), "game::items".to_string()]
    );
    assert_eq!(forward.rules[0].identity(), reverse.rules[0].identity());
    assert!(forward.rules[0]
        .dependencies()
        .contains(&"game::items::ItemWeight".to_string()));

    let formatted_inventory = format_source(inventory.source, &enabled()).unwrap();
    assert!(formatted_inventory.contains("game::items::ItemWeight(item, weight)"));
    let formatted = SourceModule {
        module_id: inventory.module_id,
        source: &formatted_inventory,
    };
    let round_trip = compile_modules(&[items, formatted], &enabled()).unwrap();
    assert_eq!(forward.manifest_digest, round_trip.manifest_digest);
}

#[test]
fn module_aggregation_obeys_one_global_raw_profile() {
    let module = SourceModule {
        module_id: "game::one",
        source: "relation One(value: int)\n",
    };
    let other = SourceModule {
        module_id: "game::two",
        source: "relation Two(value: int)\n",
    };
    let mut options = enabled();
    options.raw_limits.max_relations = 1;
    let error = compile_modules(&[module, other], &options).unwrap_err();
    assert_eq!(error[0].code, DiagnosticCode::RawRelationLimit);

    let options = enabled();
    let empty_one = compile_modules(
        &[SourceModule {
            module_id: "game::empty_one",
            source: "",
        }],
        &options,
    )
    .unwrap();
    let empty_two = compile_modules(
        &[SourceModule {
            module_id: "game::empty_two",
            source: "",
        }],
        &options,
    )
    .unwrap();
    assert_ne!(empty_one.manifest_digest, empty_two.manifest_digest);
}

#[test]
fn bounded_reader_and_collection_limits_reject_before_truncation() {
    let disabled = compile_reader(
        std::io::Cursor::new(vec![b'x'; 64]),
        &FrontendOptions::default(),
    )
    .unwrap_err();
    assert_eq!(disabled[0].code, DiagnosticCode::FeatureDisabled);

    let mut options = enabled();
    options.raw_limits.max_source_bytes = 8;
    let reader = std::io::Cursor::new(vec![b'x'; 64]);
    assert_eq!(
        compile_reader(reader, &options).unwrap_err()[0].code,
        DiagnosticCode::RawSourceByteLimit
    );

    options = enabled();
    options.raw_limits.max_columns_per_relation = 1;
    assert_eq!(
        compile("relation Pair(left: int, right: int)\n", &options).unwrap_err()[0].code,
        DiagnosticCode::RawColumnLimit
    );

    options = enabled();
    options.raw_limits.max_unique_constraints_per_relation = 1;
    let source = r#"
relation Pair(left: int, right: int)
    unique left
    unique right
"#;
    assert_eq!(
        compile(source, &options).unwrap_err()[0].code,
        DiagnosticCode::RawUniqueConstraintLimit
    );
}

#[test]
fn formatter_and_symbol_surface_round_trip_the_frontend_manifest() {
    let original = compile(OWNERSHIP, &enabled()).unwrap();
    let formatted = format_source(OWNERSHIP, &enabled()).unwrap();
    let round_trip = compile(&formatted, &enabled()).unwrap();
    assert_eq!(original.manifest_digest, round_trip.manifest_digest);
    assert!(formatted.contains("relation Owns(owner: entity, item: entity)"));
    assert!(formatted.contains("derive TotalWeight(person, sum(weight))"));

    let symbols = symbols(OWNERSHIP, &enabled()).unwrap();
    assert!(symbols.iter().any(|symbol| {
        symbol.identity == "game::inventory::Owns"
            && symbol.kind == FrontendSymbolKind::AuthoritativeRelation
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol.identity == "game::inventory::Encumbered"
            && symbol.kind == FrontendSymbolKind::DerivedRelation
    }));
}

#[test]
fn checker_rejects_symmetric_unique_namespace_and_recursion() {
    let symmetric_unique = r#"
relation Friendship(left: entity, right: entity)
    symmetric
    unique left
"#;
    assert_eq!(
        compile(symmetric_unique, &enabled()).unwrap_err()[0].code,
        DiagnosticCode::SymmetricUnique
    );

    let collision = r#"
relation Same(value: int)
derive Same(value)
    when Same(value)
"#;
    assert_eq!(
        compile(collision, &enabled()).unwrap_err()[0].code,
        DiagnosticCode::NamespaceCollision
    );

    let recursive = r#"
derive A(value)
    when B(value)
derive B(value)
    when A(value)
"#;
    assert_eq!(
        compile(recursive, &enabled()).unwrap_err()[0].code,
        DiagnosticCode::RecursiveDerivation
    );
}

#[test]
fn invalid_atom_and_rule_permutations_select_one_canonical_diagnostic() {
    let first = r#"
relation Known(value: int)
derive Bad(value)
    when Known(value, extra)
    and Missing(value)
"#;
    let second = r#"
relation Known(value: int)
derive Bad(value)
    when Missing(value)
    and Known(value, extra)
"#;
    let left = compile(first, &enabled()).unwrap_err().remove(0);
    let right = compile(second, &enabled()).unwrap_err().remove(0);
    assert_eq!(left.code, DiagnosticCode::UnknownRelation);
    assert_eq!(left.code, right.code);
    assert_eq!(left.witness, right.witness);

    let recursive_and_unknown = r#"
derive A(value)
    when B(value)
    and Missing(value)
derive B(value)
    when A(value)
"#;
    assert_eq!(
        compile(recursive_and_unknown, &enabled()).unwrap_err()[0].code,
        DiagnosticCode::UnknownRelation
    );
}

#[test]
fn every_relation_frontend_source_file_stays_below_one_thousand_lines() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/relation_frontend");
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            let lines = std::fs::read_to_string(&path).unwrap().lines().count();
            assert!(lines <= 1_000, "{} has {lines} lines", path.display());
        }
    }
}
