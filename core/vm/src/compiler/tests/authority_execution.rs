    #[test]
    fn compiler_entrypoint_enforces_transitive_system_authority() {
        let message = compile_err(
            r#"
            component Tick { n: 0 }
            resource Audit { n: 0 }
            fn hidden_write() { set_resource(Audit, Audit { n: 1 }) }
            system Run(t: Tick) { hidden_write() }
            "#,
        );
        assert!(
            message.contains("System 'Run' exceeds its declared authority")
                && message.contains("writes [Audit]")
                && message.contains("Run -> hidden_write"),
            "got: {message}"
        );
    }

    #[test]
    fn scheduler_metadata_uses_exact_synchronous_authority() {
        let source = r#"
            component Tick { n: 0 }
            resource Audit { n: 0 }
            event Deferred { n: int }

            fn hidden_write() { set_resource(Audit, Audit { n: 1 }) }
            on Deferred(e) { set_resource(Audit, Audit { n: e.n }) }

            system WritesNow(t: Tick, writes Audit) { hidden_write() }
            system EmitsLater(t: Tick, emits Deferred) { emit Deferred { n: t.n } }
        "#;
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();
        let result = Compiler::new().compile(&program).expect("compile");
        let writes_now = result
            .systems
            .iter()
            .find(|system| system.name == "WritesNow")
            .expect("WritesNow metadata");
        assert!(writes_now.params.iter().any(|param| {
            param.name == "__body_write" && param.comp_type == "Audit" && param.is_mut
        }));
        assert!(!writes_now.params.iter().any(|param| param.comp_type == "*"));

        let emits_later = result
            .systems
            .iter()
            .find(|system| system.name == "EmitsLater")
            .expect("EmitsLater metadata");
        assert!(!emits_later
            .params
            .iter()
            .any(|param| param.comp_type == "Audit" || param.comp_type == "*"));
    }

    #[test]
    fn checker_output_cannot_authorize_a_different_program() {
        let parse = |source: &str| {
            let tokens = Lexer::new(source).tokenize().0;
            Parser::new(tokens).parse()
        };
        let program_a = parse(
            r#"
            component Root {}
            component Hidden {}
            system Run(root: Root) {}
            "#,
        );
        let program_b = parse(
            r#"
            component Root {}
            component Hidden {}
            fn hidden_write(target: entity) { set(target, Hidden {}) }
            system Run(root: Root) { hidden_write(self) }
            "#,
        );
        let mut checker = crate::checker::Checker::new();
        let errors = checker.check(&program_a);
        assert!(errors.is_empty(), "Program A should check: {errors:?}");
        let checked_a = checker.output();

        Compiler::new()
            .with_checker_output(checked_a.clone())
            .compile(&program_a)
            .expect("the matching semantic product must compile");
        let mismatch = Compiler::new()
            .with_checker_output(checked_a)
            .compile(&program_b)
            .expect_err("Program A's checker output must never authorize Program B");
        assert!(mismatch.message.contains("different program or module graph"));
    }

    #[test]
    fn fingerprintless_checker_output_is_rejected_before_lowering() {
        let tokens = Lexer::new("component Root {} system Run(root: Root) {}").tokenize().0;
        let program = Parser::new(tokens).parse();
        let error = Compiler::new()
            .with_checker_output(crate::types::CheckerOutput::default())
            .compile(&program)
            .expect_err("an unchecked default product must never reach lowering");
        assert!(error.message.contains("not a checked semantic product"));
    }

    #[test]
    fn checker_output_is_bound_to_enabled_features() {
        let tokens = Lexer::new("component Root {} system Run(root: Root) {}").tokenize().0;
        let program = Parser::new(tokens).parse();
        let mut checker = crate::checker::Checker::new();
        assert!(checker.check(&program).is_empty());

        let error = Compiler::new()
            .with_checker_output(checker.output())
            .with_features(vec!["causal_laws".to_string()])
            .compile(&program)
            .expect_err("a product checked under another feature set must be rejected");
        assert!(error
            .message
            .contains("different enabled-feature configuration"));
    }

    #[test]
    fn checker_output_is_bound_to_all_semantic_options() {
        let tokens = Lexer::new("component Root {} system Run(root: Root) {}").tokenize().0;
        let program = Parser::new(tokens).parse();
        let checked_options = crate::checker::CheckerOptions::default();
        let mut checker = crate::checker::Checker::new_with_options(checked_options);
        assert!(checker.check(&program).is_empty());

        let compile_options = crate::checker::CheckerOptions {
            strict_types: true,
            ..crate::checker::CheckerOptions::default()
        };
        let error = Compiler::new()
            .with_checker_output(checker.output())
            .with_checker_options(compile_options)
            .compile(&program)
            .expect_err("every checker semantic option must be provenance-bound");
        assert!(error
            .message
            .contains("different checker semantic options"));
    }

    #[test]
    fn mutated_checker_output_fails_integrity_validation() {
        let tokens = Lexer::new("component Root {} system Run(root: Root) {}").tokenize().0;
        let program = Parser::new(tokens).parse();
        let mut checker = crate::checker::Checker::new();
        assert!(checker.check(&program).is_empty());
        let mut output = checker.output();
        output
            .for_iter_kinds
            .insert(crate::ast::NodeId(u32::MAX), crate::types::ForIterKind::List);

        let error = Compiler::new()
            .with_checker_output(output)
            .compile(&program)
            .expect_err("mutated lowering metadata must be rejected");
        assert!(error
            .message
            .contains("semantic product integrity validation"));
    }

    #[test]
    fn scheduler_metadata_uses_call_site_specialized_callback_effects() {
        let source = r#"
            component Root {}
            component X {}
            component Y {}
            fn invoke(callback: fn(entity) -> nil, target: entity) { callback(target) }
            fn write_x(target: entity) { set(target, X {}) }
            fn write_y(target: entity) { set(target, Y {}) }
            system SystemX(root: Root, writes X) { invoke(write_x, self) }
            system SystemY(root: Root, writes Y) { invoke(write_y, self) }
        "#;
        let tokens = Lexer::new(source).tokenize().0;
        let program = Parser::new(tokens).parse();
        let result = Compiler::new().compile(&program).expect("compile");
        let body_writes = |name: &str| {
            result
                .systems
                .iter()
                .find(|system| system.name == name)
                .expect("system metadata")
                .params
                .iter()
                .filter(|param| param.name == "__body_write")
                .map(|param| param.comp_type.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(body_writes("SystemX"), vec!["X"]);
        assert_eq!(body_writes("SystemY"), vec!["Y"]);
    }
