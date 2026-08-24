
    fn parse_owned_source(source: &str, file: u32) -> Program {
        let tokens = crate::lexer::Lexer::new(source).tokenize().0;
        crate::parser::Parser::new(tokens)
            .with_file_id(FileId(file))
            .parse()
    }

    fn check_owned_program(program: &Program) -> Vec<TypeError> {
        let mut checker = Checker::new();
        checker.check(program)
    }

    #[test]
    fn owner_module_requires_an_explicit_owned_write_grant() {
        let program = parse_owned_source(
            r#"
            component Live owned { value: int = 0 }
            fn bypass(target: entity) { set(target, Live { value: 1 }) }
            "#,
            1,
        );
        let errors = check_owned_program(&program);
        assert!(errors.iter().any(|error| {
            error.message.contains("Ownership violation")
                && error.message.contains("bypass")
                && error.message.contains("no owned-write grant")
        }), "{errors:?}");
    }

    #[test]
    fn typed_component_values_cannot_hide_owned_writes() {
        let program = parse_owned_source(
            r#"
            component Live owned { value: int = 0 }
            fn replace_param(target: entity, replacement: Live) {
                set(target, replacement)
            }
            fn replace_local(target: entity) {
                let replacement: Live = Live { value: 2 }
                set(target, replacement)
            }
            "#,
            1,
        );
        let errors = check_owned_program(&program);
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("replace_param")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("replace_local")),
            "{errors:?}"
        );
    }

    #[test]
    fn owner_module_can_expose_one_authorized_mutator_and_readonly_query() {
        let program = parse_owned_source(
            r#"
            pub component Live owned { value: int = 0 }
            pub fn retire(target: entity) -> nil writes owned [Live] {
                set(target, Live { value: 1 })
            }
            pub readonly fn inspect(target: entity) -> Option<Live> {
                return get(target, Live)
            }
            "#,
            1,
        );
        let errors = check_owned_program(&program);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn another_module_cannot_forge_an_owned_write_grant() {
        let mut owner = parse_owned_source("pub component Live owned { value: int = 0 }", 1);
        let writer = parse_owned_source(
            "pub fn bypass(target: entity) writes owned [Live] { set(target, Live { value: 1 }) }",
            2,
        );
        owner.declarations.extend(writer.declarations);
        let errors = check_owned_program(&owner);
        assert!(errors.iter().any(|error| {
            error.message.contains("cannot acquire owned-write capability")
        }), "{errors:?}");
    }

    #[test]
    fn explicitly_named_coowner_module_can_acquire_the_capability() {
        let owner = parse_owned_source(
            "pub component Live owned with [fixture] { value: int = 0 }",
            1,
        );
        let fixture = parse_owned_source(
            "pub fn publish(target: entity) -> nil writes owned [Live] { set(target, Live { value: 1 }) }\npub fn missing_grant(target: entity) { set(target, Live { value: 2 }) }",
            2,
        );
        let mut checker = Checker::new();
        let canonical_binding = crate::ast::ModuleAlias::namespaced(
            fixture.declarations,
            "test:fixture".to_string(),
        );
        checker.set_aliases(std::collections::HashMap::from([
            ("fixture".to_string(), canonical_binding.clone()),
            ("alternate_name".to_string(), canonical_binding),
        ]));
        let errors = checker.check(&owner);
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("publish cannot")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("missing_grant")),
            "the canonical module must be checked exactly once: {errors:?}"
        );
    }

    #[test]
    fn explicit_transfer_revokes_the_declaring_module_and_moves_ownership() {
        let owner = parse_owned_source(
            r#"
            pub component Live owned transfer to successor { value: int = 0 }
            pub fn former_owner(target: entity) -> nil writes owned [Live] {
                set(target, Live { value: 1 })
            }
            "#,
            1,
        );
        let successor = parse_owned_source(
            "pub fn publish(target: entity) -> nil writes owned [Live] { set(target, Live { value: 2 }) }",
            2,
        );
        let mut checker = Checker::new();
        checker.set_aliases(std::collections::HashMap::from([(
            "successor".to_string(),
            crate::ast::ModuleAlias::namespaced(
                successor.declarations,
                "test:successor".to_string(),
            ),
        )]));
        let errors = checker.check(&owner);
        assert!(errors.iter().any(|error| error.message.contains("former_owner")), "{errors:?}");
        assert!(!errors.iter().any(|error| error.message.contains("successor.publish cannot")), "{errors:?}");
    }

    #[test]
    fn test_owned_capability_is_scoped_to_the_test_body() {
        let mut program = parse_owned_source(
            "pub component Live owned { value: int = 0 }",
            1,
        );
        let tests = parse_owned_source(
            r#"
            test "fixture" writes owned [Live] {
                let target = spawn(Live { value: 1 })
                set(target, Live { value: 2 })
            }
            fn production(target: entity) { set(target, Live { value: 3 }) }
            "#,
            2,
        );
        program.declarations.extend(tests.declarations);
        let errors = check_owned_program(&program);
        assert!(!errors.iter().any(|error| error.message.contains("test 'fixture'")), "{errors:?}");
        assert!(errors.iter().any(|error| error.message.contains("production")), "{errors:?}");
    }

    #[test]
    fn field_ownership_allows_unowned_updates_and_bounds_owned_fields() {
        let program = parse_owned_source(
            r#"
            component Profile {
                name: str = "",
                owned rank: int = 0
            }
            fn rename(target: entity) { update(target, Profile) { name = "new" } }
            fn rerank_bad(target: entity) { update(target, Profile) { rank = 7 } }
            fn rerank(target: entity) writes owned [Profile.rank] {
                update(target, Profile) { rank = 8 }
            }
            "#,
            1,
        );
        let Decl::Component(profile) = &program.declarations[0] else {
            panic!("expected Profile component");
        };
        assert!(profile
            .fields
            .iter()
            .any(|field| field.name == "rank" && field.is_owned));
        let errors = check_owned_program(&program);
        assert!(!errors.iter().any(|error| error.message.contains("rename")), "{errors:?}");
        assert!(errors.iter().any(|error| error.message.contains("rerank_bad")), "{errors:?}");
        assert!(!errors.iter().any(|error| error.message.contains("rerank cannot")), "{errors:?}");
    }

    #[test]
    fn owned_resources_and_handlers_use_the_same_semantic_boundary() {
        let program = parse_owned_source(
            r#"
            resource Registry owned { revision: int = 0 }
            event Refresh {}
            on Refresh(evt) writes owned [Registry] {
                update(Registry) { revision = res(Registry).revision + 1 }
            }
            fn bypass() { update(Registry) { revision = 99 } }
            "#,
            1,
        );
        let errors = check_owned_program(&program);
        assert!(!errors.iter().any(|error| error.message.contains("on Refresh")), "{errors:?}");
        assert!(errors.iter().any(|error| error.message.contains("bypass")), "{errors:?}");
    }

    #[test]
    fn mutable_system_and_despawn_require_owned_capabilities() {
        let program = parse_owned_source(
            r#"
            component Live owned { value: int = 0 }
            system Mutates(live: mut Live) { live.value = live.value + 1 }
            fn destroy(target: entity) { despawn(target) }
            "#,
            1,
        );
        let errors = check_owned_program(&program);
        assert!(errors.iter().any(|error| error.message.contains("Mutates")), "{errors:?}");
        assert!(errors.iter().any(|error| error.message.contains("destroy")), "{errors:?}");
    }

    #[test]
    fn direct_compiler_entrypoint_enforces_owned_writes() {
        let program = parse_owned_source(
            r#"
            component Live owned { value: int = 0 }
            fn bypass(target: entity) { set(target, Live { value: 1 }) }
            "#,
            1,
        );
        let error = crate::compiler::Compiler::new()
            .compile(&program)
            .expect_err("direct compilation must enforce semantic ownership");
        assert!(error.message.contains("Ownership violation"), "{error:?}");
    }
