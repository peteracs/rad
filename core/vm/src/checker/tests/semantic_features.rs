#[test]
fn frame_cost_contract_follows_hidden_full_scan() {
    let (errors, _) = authority_src(
        r#"
            component Root {}

            fn hidden_scan() -> nil {
                let _all = query { Root }
            }

            @frame
            system Frame(root: Root) {
                hidden_scan()
            }
            "#,
    );
    let violation = errors
        .iter()
        .find(|error| error.message.contains("no-full-scan contract"))
        .unwrap_or_else(|| panic!("transitive scan must be rejected: {errors:?}"));
    assert!(
        violation
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("Frame -> hidden_scan")),
        "diagnostic must show the transitive query-cost path: {violation:?}"
    );
}

#[test]
fn materialized_view_predicates_validate_dependencies_fields_and_types() {
    let (valid, _) = authority_src(
        r#"
            component Product { active: bool = false }
            component Inventory { available: int = 0 }
            materialized view Sellable {
                depends [Product, Inventory]
                where Product.active == true and Inventory.available > 0
            }
            "#,
    );
    assert!(valid.is_empty(), "{valid:?}");

    let (invalid, _) = authority_src(
        r#"
            component Product { active: bool = false }
            materialized view Broken {
                depends [Product]
                where Product.missing == 1
            }
            "#,
    );
    assert!(
        invalid
            .iter()
            .any(|error| error.message.contains("unknown field 'Product.missing'")),
        "{invalid:?}"
    );
}

#[test]
fn materialized_view_reads_expand_to_dependencies_without_a_full_scan() {
    let (errors, report) = authority_src(
        r#"
            component Product { active: bool = false }
            component Inventory { available: int = 0 }
            materialized view Sellable {
                depends [Product, Inventory]
                where Product.active == true and Inventory.available > 0
            }
            @no_full_scan
            system Inspect(reads Product, reads Inventory) {
                let current = entities(Sellable)
            }
            "#,
    );
    assert!(errors.is_empty(), "{errors:?}");
    let inspect = report.resolve("Inspect").expect("Inspect authority");
    assert_eq!(inspect.synchronous.reads, ["Inventory", "Product"]);
    assert!(!inspect.synchronous.full_scan);
    assert_eq!(inspect.synchronous.queries.len(), 1);
    assert_eq!(inspect.synchronous.queries[0].operation, "entities");
    assert_eq!(inspect.synchronous.queries[0].source, "Sellable");
    assert_eq!(
        inspect.synchronous.queries[0].complexity,
        crate::types::AuthorityQueryComplexity::OutputLinear
    );
}

#[test]
fn materialized_views_reject_direct_component_style_mutation() {
    let (errors, _) = authority_src(
        r#"
            component Product { active: bool = false }
            materialized view Sellable { depends [Product] }
            let product = spawn(Product { active: true })
            set(product, Sellable {})
            "#,
    );
    let error = errors
        .iter()
        .find(|error| error.message.contains("runtime-maintained"))
        .unwrap_or_else(|| panic!("view mutation needs its own diagnostic: {errors:?}"));
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("source components")),
        "diagnostic must direct the developer to the maintained sources: {error:?}"
    );
}

#[test]
fn allocation_free_view_visit_tracks_named_callback_effects() {
    let (errors, report) = authority_src(
        r#"
            component Active {}
            component Position { x: float = 0.0 }
            materialized view ActiveTrips { depends [Active, Position] }

            fn advance(target: entity) -> nil {
                let position = read_field(target, Position, "x")
                write_field(target, Position, "x", position + 1.0)
            }

            @frame
            @no_guest_allocation
            system Advance(reads Active, reads Position, writes Position) {
                visit_view(ActiveTrips, advance)
            }
            "#,
    );
    assert!(errors.is_empty(), "{errors:?}");
    let advance = report.resolve("Advance").expect("Advance authority");
    assert_eq!(advance.synchronous.reads, ["Active", "Position"]);
    assert_eq!(advance.synchronous.writes, ["Position"]);
    assert!(!advance.synchronous.allocates);
    assert!(!advance.synchronous.full_scan);
    assert!(advance.contracts.frame);
    assert!(advance.contracts.no_guest_allocation);
    assert_eq!(advance.synchronous.queries.len(), 1);
    assert_eq!(advance.synchronous.queries[0].operation, "visit_view");
    assert_eq!(advance.synchronous.queries[0].source, "ActiveTrips");
    assert!(!advance.synchronous.queries[0].allocates);
}

#[test]
fn no_guest_allocation_contract_follows_hidden_allocator() {
    let (errors, _) = authority_src(
        r#"
            component Root {}

            fn hidden_allocation() -> nil {
                let _items = range(0, 4)
            }

            @no_guest_allocation
            system Frame(root: Root) {
                hidden_allocation()
            }
            "#,
    );
    let violation = errors
        .iter()
        .find(|error| error.message.contains("no-guest-allocation contract"))
        .unwrap_or_else(|| panic!("transitive allocation must be rejected: {errors:?}"));
    assert!(
        violation
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("Frame -> hidden_allocation")),
        "diagnostic must show the transitive allocation path: {violation:?}"
    );
}

#[test]
fn exceptional_scan_requires_a_nonempty_reason_and_is_then_explicit() {
    let (errors, _) = authority_src(
        r#"
            component Root {}

            fn teardown_scan() -> nil {
                let _all = query { Root }
            }

            @frame
            @allow_full_scan(reason: "session teardown")
            system Teardown(root: Root) {
                teardown_scan()
            }
            "#,
    );
    assert!(
        !errors
            .iter()
            .any(|error| error.message.contains("no-full-scan contract")),
        "an explicit, reasoned exception must satisfy the contract: {errors:?}"
    );
}

#[test]
fn native_constructors_are_bounded_pure_allocators_in_authority_graphs() {
    let (errors, _) = authority_src(
        r#"
            opaque type Money = i64
            component Balance { amount: Money = Money(i64(0)) }
            transaction Credit(target: entity) {
                requires true
                changes_only [Balance]
                let amount: Money = Money(i64(7))
                set(target, Balance { amount: amount })
                ensures true
            }
            "#,
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn aliased_module_query_resolves_its_local_component() {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    let entry_program = Parser::new(Lexer::new("").tokenize().0).parse();
    let module_source = r#"
        pub component Cell { value: int = 0 }
        pub system Probe() {}
        pub fn selected() -> list<entity> {
            let _future: world_fork = simulate(fork(), [system::Probe], 1)
            return query { Cell } where Cell.value > 0
        }
    "#;
    let module_program = Parser::new(Lexer::new(module_source).tokenize().0).parse();
    let aliases = std::collections::HashMap::from([(
        "surface".to_string(),
        crate::ast::ModuleAlias::namespaced(
            module_program.declarations,
            "test:surface".to_string(),
        ),
    )]);
    let mut checker = Checker::new();
    checker.set_aliases(aliases);
    let errors = checker.check(&entry_program);
    assert!(errors.is_empty(), "got: {errors:?}");
    assert!(
        checker
            .warnings()
            .iter()
            .all(|warning| !warning.message.contains("Probe")
                || !warning.message.contains("never run")),
        "aliased system reference was ignored: {:?}",
        checker.warnings()
    );
}

#[test]
fn task_type_annotation_round_trips_async_result_type() {
    let errors = check_src(
        r#"
        async fn produce() -> int { return 7 }
        async fn consume() -> int {
            let pending: task<int> = async produce()
            let value: int = await pending
            return value
        }
        "#,
    );
    assert!(errors.is_empty(), "got: {errors:?}");
}
