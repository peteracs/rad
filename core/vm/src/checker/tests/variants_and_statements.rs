

    #[test]
    fn zero_field_variant_shorthand_requires_compat_flag() {
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "AccessSignal".to_string(),
                    type_params: vec![],
                    variants: vec![VariantDefNode {
                        name: "MfaDisabled".to_string(),
                        annotations: vec![],
                        fields: vec![],
                    }],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(2),
                    names: vec!["sig".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::StateRef(
                        "AccessSignal".to_string(),
                        "MfaDisabled".to_string(),
                        span(2),
                    ),
                })),
            ],
        };

        let mut checker = Checker::new_with_options(CheckerOptions {
            features: vec![],
            compat_v0_5_dx: false,
            warn_compat: false,
            strict_types: false,
        });
        let errors = checker.check(&program);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("requires --compat-v0.5-dx")),
            "zero-field variant shorthand should require compat flag, got: {:?}",
            errors
        );
    }

    #[test]
    fn e2504_unknown_binding_in_match_with_rest() {
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Sig".to_string(),
                    type_params: vec![],
                    variants: vec![VariantDefNode {
                        name: "Alarm".to_string(),
                        annotations: vec![],
                        fields: vec![("severity".to_string(), Expr::IntLit(0, span(1)))],
                    }],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(2),
                    names: vec!["s".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::VariantExpr(
                        "Sig".to_string(),
                        "Alarm".to_string(),
                        vec![("severity".to_string(), Expr::IntLit(5, span(2)))],
                        span(2),
                    ),
                })),
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(3),
                    subject: Expr::Ident("s".to_string(), span(3)),
                    cases: vec![MatchCase {
                        id: nid(),
                        span: span(3),
                        pattern: Pattern::Variant {
                            path: vec!["Alarm".to_string()],
                            bindings: vec!["bogus".to_string()],
                            pattern_bindings: vec![],
                            has_rest: true,
                            is_bare_variant: false,
                        },
                        guard: None,
                        body: empty_block(),
                    }],
                })),
            ],
        };
        let mut checker = Checker::new_with_options(CheckerOptions {
            features: vec![],
            compat_v0_5_dx: true,
            warn_compat: false,
            strict_types: false,
        });
        let errors = checker.check(&program);
        assert!(
            errors.iter().any(|e| e.message.contains("E2504")),
            "should emit E2504 for unknown binding 'bogus', got: {:?}",
            errors
        );
    }

    #[test]
    fn e2501_ambiguous_ref_state_machine_and_sum_type() {
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Dual".to_string(),
                    type_params: vec![],
                    variants: vec![VariantDefNode {
                        name: "Active".to_string(),
                        annotations: vec![],
                        fields: vec![("level".to_string(), Expr::IntLit(0, span(1)))],
                    }],
                }),
                Decl::State(StateDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "Dual".to_string(),
                    states: vec![StateDef {
                        id: nid(),
                        span: span(2),
                        name: "Active".to_string(),
                        transitions: vec![],
                    }],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(3),
                    names: vec!["x".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::StateRef("Dual".to_string(), "Active".to_string(), span(3)),
                })),
            ],
        };
        let mut checker = Checker::new_with_options(CheckerOptions {
            features: vec![],
            compat_v0_5_dx: true,
            warn_compat: true,
            strict_types: false,
        });
        let errors = checker.check(&program);
        assert!(
            errors.iter().any(|e| e.message.contains("E2501")),
            "should emit E2501 for ambiguous Dual::Active, got: {:?}",
            errors
        );
    }

    #[test]
    fn strict_types_requires_let_and_fn_annotations() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["x".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::IntLit(1, span(1)),
                })),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "id".to_string(),
                    type_params: vec![],
                    params: vec!["v".to_string()],
                    param_muts: vec![false],
                    param_types: vec![None],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(2),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(3),
                            value: Some(Expr::Ident("v".to_string(), span(3))),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
            ],
        };
        let mut checker = Checker::new_with_options(CheckerOptions {
            features: vec![],
            compat_v0_5_dx: true,
            warn_compat: true,
            strict_types: true,
        });
        let errors = checker.check(&program);
        assert!(errors.iter().any(|e| e
            .message
            .contains("variable 'x' requires an explicit type annotation")));
        assert!(errors
            .iter()
            .any(|e| e.message.contains("parameter 'v' in function 'id'")));
        assert!(errors.iter().any(|e| e
            .message
            .contains("function 'id' needs an explicit return type")));
    }

    #[test]
    fn strict_types_flag_enforces_types_on_pub_and_private() {
        let program = Program {
            declarations: vec![
                Decl::Fn(FnDecl {
                    is_pub: true,
                    id: nid(),
                    span: span(1),
                    name: "id".to_string(),
                    type_params: vec![],
                    params: vec!["v".to_string()],
                    param_muts: vec![false],
                    param_types: vec![None],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::Ident("v".to_string(), span(2))),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
                Decl::Component(component_decl! {
                    id: nid(),
                    span: span(3),
                    name: "Health".to_string(),
                    is_pub: true,
                    fields: vec![("hp".to_string(), None, Expr::IntLit(100, span(3)))],
                }),
            ],
        };
        let mut checker = Checker::new_with_options(CheckerOptions {
            features: vec![],
            compat_v0_5_dx: true,
            warn_compat: true,
            strict_types: true, // strict types enabled
        });
        let errors = checker.check(&program);
        assert!(errors.iter().any(|e| e.message.contains(
            "Public function 'id' requires explicit type annotations for all parameters"
        )));
        assert!(errors.iter().any(|e| e
            .message
            .contains("Public function 'id' requires an explicit return type")));
        assert!(errors.iter().any(|e| e.message.contains(
            "Public component 'Health' requires explicit type annotations for all fields"
        )));
    }

    #[test]
    fn function_value_forward_reference_resolves_before_declaration() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["inc".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::Ident("later".to_string(), span(1)),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(2),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("inc".to_string(), span(2))),
                        vec![Expr::IntLit(1, span(2))],
                        span(2),
                    ),
                })),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(3),
                    name: "later".to_string(),
                    type_params: vec![],
                    params: vec!["v".to_string()],
                    param_muts: vec![false],
                    param_types: vec![Some(TypeExpr::Named("int".to_string()))],
                    return_type: Some(TypeExpr::Named("int".to_string())),
                    body: Block {
                        id: nid(),
                        span: span(3),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(4),
                            value: Some(Expr::Binary(
                                Box::new(Expr::Ident("v".to_string(), span(4))),
                                BinOp::Add,
                                Box::new(Expr::IntLit(1, span(4))),
                                span(4),
                            )),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "forward function value reference should type-check, got: {:?}",
            errors
        );
    }

    #[test]
    fn match_infers_sum_type_from_variant_names() {
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Weather".to_string(),
                    type_params: vec![],
                    variants: vec![
                        VariantDefNode {
                            name: "Sunny".to_string(),
                            annotations: vec![],
                            fields: vec![],
                        },
                        VariantDefNode {
                            name: "Rainy".to_string(),
                            annotations: vec![],
                            fields: vec![("mm".to_string(), Expr::IntLit(0, span(1)))],
                        },
                    ],
                }),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "describe".to_string(),
                    type_params: vec![],
                    params: vec!["w".to_string()],
                    param_muts: vec![false],
                    param_types: vec![None],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(2),
                        stmts: vec![Stmt::Match(MatchStmt {
                            id: nid(),
                            span: span(3),
                            subject: Expr::Ident("w".to_string(), span(3)),
                            cases: vec![
                                MatchCase {
                                    id: nid(),
                                    span: span(4),
                                    pattern: Pattern::Variant {
                                        path: vec!["Sunny".to_string()],
                                        bindings: vec![],
                                        pattern_bindings: vec![],
                                        has_rest: false,
                                        is_bare_variant: false,
                                    },
                                    guard: None,
                                    body: empty_block(),
                                },
                                MatchCase {
                                    id: nid(),
                                    span: span(5),
                                    pattern: Pattern::Variant {
                                        path: vec!["Rainy".to_string()],
                                        bindings: vec!["mm".to_string()],
                                        pattern_bindings: vec![],
                                        has_rest: false,
                                        is_bare_variant: false,
                                    },
                                    guard: None,
                                    body: Block {
                                        id: nid(),
                                        span: span(5),
                                        stmts: vec![Stmt::Expr(ExprStmt {
                                            id: nid(),
                                            span: span(5),
                                            expr: Expr::Call(
                                                Box::new(Expr::Ident("print".to_string(), span(5))),
                                                vec![Expr::Ident("mm".to_string(), span(5))],
                                                span(5),
                                            ),
                                        })],
                                    },
                                },
                            ],
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "match should infer Weather from variant names, got: {:?}",
            errors
        );
    }

    #[test]
    fn match_destructure_on_unknown_type_reports_actionable_error() {
        let program = Program {
            declarations: vec![Decl::Fn(FnDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "process".to_string(),
                type_params: vec![],
                params: vec!["x".to_string()],
                param_muts: vec![false],
                param_types: vec![None],
                return_type: None,
                body: Block {
                    id: nid(),
                    span: span(1),
                    stmts: vec![Stmt::Match(MatchStmt {
                        id: nid(),
                        span: span(2),
                        subject: Expr::Ident("x".to_string(), span(2)),
                        cases: vec![MatchCase {
                            id: nid(),
                            span: span(3),
                            pattern: Pattern::Variant {
                                path: vec!["Bogus".to_string()],
                                bindings: vec!["field".to_string()],
                                pattern_bindings: vec![],
                                has_rest: false,
                                is_bare_variant: false,
                            },
                            guard: None,
                            body: empty_block(),
                        }],
                    })],
                },
                is_pure: false,
                is_async: false,
                effects: vec![],
            })],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("subject type is unknown")),
            "should report actionable error about unknown subject type, got: {:?}",
            errors
        );
    }

    #[test]
    fn struct_decl_registers_type() {
        let program = Program {
            declarations: vec![Decl::Struct(DataDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "Point".to_string(),
                kind: DataKind::Struct,
                version: 0,
                fields: component_fields(vec![
                    ("x".to_string(), None, Expr::FloatLit(0.0, span(1))),
                    ("y".to_string(), None, Expr::FloatLit(0.0, span(1))),
                ]),
                indexed_fields: vec![],
            })],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "struct decl should not produce errors, got: {:?}",
            errors
        );
        assert!(checker.structs.contains_key("Point"));
        let st = &checker.structs["Point"];
        assert_eq!(st.fields.len(), 2);
        assert_eq!(st.fields[0].0, "x");
        assert_eq!(st.fields[1].0, "y");
    }

    #[test]
    fn struct_system_param_rejected() {
        let program = Program {
            declarations: vec![
                Decl::Struct(DataDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Data".to_string(),
                    kind: DataKind::Struct,
                    version: 0,
                    fields: component_fields(vec![(
                        "v".to_string(),
                        None,
                        Expr::IntLit(0, span(1)),
                    )]),
                    indexed_fields: vec![],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "BadSystem".to_string(),
                    params: vec![("d".to_string(), false, "Data".to_string())],
                    body: empty_block(),
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.iter().any(|e| e.message.contains("struct")
                && e.message.contains("system parameters must be components")),
            "should reject struct as system param, got: {:?}",
            errors
        );
    }

    #[test]
    fn struct_duplicate_field_rejected() {
        let program = Program {
            declarations: vec![Decl::Struct(DataDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "Bad".to_string(),
                kind: DataKind::Struct,
                version: 0,
                fields: component_fields(vec![
                    ("x".to_string(), None, Expr::IntLit(0, span(1))),
                    ("x".to_string(), None, Expr::IntLit(1, span(1))),
                ]),
                indexed_fields: vec![],
            })],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.iter().any(|e| e.message.contains("Duplicate field")),
            "should reject duplicate fields, got: {:?}",
            errors
        );
    }

    #[test]
    fn warns_on_always_true_or_false_conditions() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::If(IfStmt {
                id: nid(),
                span: span(1),
                condition: Expr::BoolLit(true, span(1)),
                then_block: empty_block(),
                else_block: None,
            }))],
        };
        let mut checker = Checker::new();
        let _ = checker.check(&program);
        let warnings = checker.warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("Condition is always `true`")),
            "should warn on always true condition, got: {:?}",
            warnings
        );

        let program2 = Program {
            declarations: vec![Decl::Stmt(Stmt::While(WhileStmt {
                id: nid(),
                span: span(1),
                condition: Expr::BoolLit(false, span(1)),
                body: empty_block(),
            }))],
        };
        let mut checker2 = Checker::new();
        let _ = checker2.check(&program2);
        let warnings2 = checker2.warnings();
        assert!(
            warnings2
                .iter()
                .any(|w| w.message.contains("Condition is always `false`")),
            "should warn on always false condition, got: {:?}",
            warnings2
        );
    }

    #[test]
    fn field_access_on_primitive_reports_error() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["x".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::IntLit(5, span(1)),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(2),
                    expr: Expr::Field(
                        Box::new(Expr::Ident("x".to_string(), span(2))),
                        "foo".to_string(),
                        span(2),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.iter().any(|e| e.message.contains("int")
                && e.message.contains("has no field")
                && e.message.contains("foo")),
            "Expected error about int having no field 'foo', got: {:?}",
            errors
        );
    }

    /// if-expressions: branches unify, the condition must be bool, and
    /// mismatched branch types are reported (not silently Any'd).
    #[test]
    fn if_expr_types_unify_and_diagnose() {
        let errors = check_src("fn f(x: int) -> str { return if x > 0 { \"p\" } else { \"n\" } }");
        assert!(errors.is_empty(), "got: {:?}", errors);

        let errors = check_src(
            "fn f(x: int) -> int { return if x > 0 { 1 } else if x < 0 { -1 } else { 0 } }",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);

        let errors = check_src("fn f() -> int { return if 1 { 2 } else { 3 } }");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("condition must be bool")),
            "got: {:?}",
            errors
        );

        let errors = check_src("fn f() -> int { return if true { 2 } else { \"x\" } }");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("incompatible types")),
            "got: {:?}",
            errors
        );
    }

    /// Effect inference must not depend on declaration order: a function
    /// calling helpers declared LATER still infers pure/readonly (the
    /// fixpoint refinement pass).
    #[test]
    fn effect_inference_is_order_independent() {
        // caller first, pure helper after: caller must infer pure
        let errors = check_src(
            "pure fn outer(v: int) -> int { return helper(v) * 2 }\npure fn helper(v: int) -> int { return v + 1 }",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);

        // unannotated forward chain into a readonly builtin: the head
        // must infer readonly, so a system calling it simulates
        let errors = check_src(
            "component C { n: int = 0 }\nsystem s(c: C) {\n    let _ = head(self)\n}\nfn head(e: entity) -> int { return mid(e) }\nfn mid(e: entity) -> int { return require(e, C).n }\nfn main() -> nil {\n    let _ = entity { C {} }\n    let _ = simulate(fork(), [system::s], 1)\n}",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);
    }

    /// `get_entity` is honestly `entity | nil` (guard-narrowed);
    /// `require_entity` is the fail-fast dual returning bare `entity`.
    #[test]
    fn entity_lookup_typing() {
        // unguarded use where an entity is required: error
        let errors = check_src("fn f() -> entity {\n    return get_entity(\"x\")\n}");
        assert!(
            errors.iter().any(|e| e.message.contains("entity | nil")),
            "got: {:?}",
            errors
        );

        // guard narrows to bare entity
        let errors = check_src(
            "component C { n: int = 0 }\nfn f() -> int {\n    let e = get_entity(\"x\")\n    if e == nil { return 0 }\n    return require(e, C).n\n}",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);

        // require_entity needs no guard
        let errors = check_src(
            "component C { n: int = 0 }\nfn f() -> int {\n    return require(require_entity(\"x\"), C).n\n}",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);
    }

    /// `emit` is legal in simulated systems when every transitively
    /// reachable handler is simulation-safe; an IO-bearing handler is
    /// reported with the handler named, including through emit chains.
    #[test]
    fn simulate_allows_safe_emits_rejects_io_handlers() {
        let errors = check_src(
            "component C { n: int = 0 }\nevent Ping { v: int }\nsystem s(c: mut C) {\n    emit Ping { v: c.n }\n}\non Ping(e) {\n    let _ = e.v\n}\nfn main() -> nil {\n    let _ = entity { C {} }\n    let _ = simulate(fork(), [system::s], 1)\n}",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);

        let errors = check_src(
            "component C { n: int = 0 }\nevent Ping { v: int }\nevent Pong { v: int }\nsystem s(c: mut C) {\n    emit Ping { v: c.n }\n}\non Ping(e) {\n    emit Pong { v: e.v }\n}\non Pong(e) {\n    print(e.v)\n}\nfn main() -> nil {\n    let _ = entity { C {} }\n    let _ = simulate(fork(), [system::s], 1)\n}",
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("handler `on Pong`")
                    && e.message.contains("IO builtin 'print'")),
            "got: {:?}",
            errors
        );
    }

    /// Parenthesized for-loop bindings over a list destructure tuples
    /// of any arity with element types; map two-binding iteration is
    /// untouched.
    #[test]
    fn for_loop_tuple_destructure_typing() {
        let errors = check_src(
            "fn f(rows: list<(int, str, bool)>) -> int {\n    let mut n = 0\n    for (a, s, ok) in rows {\n        if ok and s != \"\" { n = n + a }\n    }\n    return n\n}",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);

        // maps keep key/value semantics for two bindings
        let errors = check_src(
            "fn f(m: map<str, int>) -> int {\n    let mut n = 0\n    for (k, v) in m {\n        if k != \"\" { n = n + v }\n    }\n    return n\n}",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);
    }

    /// `transient resource` parses and checks like a normal resource.
    #[test]
    fn transient_resource_checks() {
        let errors = check_src(
            "transient resource Tape { orders: list = [] }\nfn main() -> nil {\n    update(Tape) { orders = push(res(Tape).orders, 1) }\n}",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);
    }

    /// Tuple±scalar broadcast types on all four ops where the tuple is
    /// on the left; scalar-left only for the commutative ones.
    #[test]
    fn tuple_scalar_broadcast_typing() {
        let errors = check_src("fn f(p: (float, float)) -> (float, float) { return p - 1.0 }");
        assert!(errors.is_empty(), "got: {:?}", errors);
        let errors = check_src("fn f(p: (float, float)) -> (float, float) { return 2.0 + p }");
        assert!(errors.is_empty(), "got: {:?}", errors);
        // scalar-left subtraction stays rejected (not commutative)
        let errors = check_src("fn f(p: (float, float)) -> (float, float) { return 1.0 - p }");
        assert!(!errors.is_empty(), "expected an error for scalar - tuple");
    }

    /// group_by keys keep the key fn's type: tuple keys flow into the
    /// result map type, invalid key types are rejected.
    #[test]
    fn group_by_key_typing() {
        let errors = check_src(
            "fn f(xs: list<int>) -> map { return xs |> group_by(fn(v) { return (v % 2, 0) }) }",
        );
        assert!(errors.is_empty(), "got: {:?}", errors);
        let errors = check_src(
            "fn f(xs: list<int>) -> map { return xs |> group_by(fn(v) { return float(v) }) }",
        );
        assert!(
            errors.iter().any(|e| e.message.contains("valid map key")),
            "got: {:?}",
            errors
        );
    }

    /// Tuple map keys: tuples of valid key types pass, floats inside
    /// tuples are still rejected.
    #[test]
    fn tuple_map_keys_typecheck() {
        let errors = check_src("let m = { (1, 2): \"a\", (3, 4): \"b\" }");
        assert!(errors.is_empty(), "got: {:?}", errors);

        let errors = check_src("let m = { (1, (\"x\", true)): 5 }");
        assert!(errors.is_empty(), "got: {:?}", errors);

        let errors = check_src("let m = { (1.0, 2): \"a\" }");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("cannot be used as a map key")),
            "got: {:?}",
            errors
        );
    }

    fn check_src(src: &str) -> Vec<TypeError> {
        use crate::lexer::Lexer;
        use crate::parser::Parser;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        let mut checker = Checker::new();
        checker.check(&program)
    }

    #[test]
    fn accum_param_must_be_a_resource() {
        // dogfood seq 83 IDEA 02: `accum` is a resource-fold declaration;
        // a per-entity component cannot be folded.
        let errors = check_src("component Pos { x: 0 }\nsystem S(p: accum Pos) { p.x = p.x + 1 }");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("only valid on resource parameters")),
            "expected accum-on-component rejection, got {:?}",
            errors
        );
    }

    #[test]
    fn accum_resource_fields_must_be_numeric() {
        // Folding is defined per int/float field; a str field cannot fold.
        let errors = check_src(
            "resource Log { label: \"x\", n: 0 }\nsystem T(l: accum Log) { l.n = l.n + 1 }",
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("non-numeric field")),
            "expected non-numeric accum rejection, got {:?}",
            errors
        );
    }

    fn check_src_warnings(src: &str) -> Vec<TypeWarning> {
        use crate::lexer::Lexer;
        use crate::parser::Parser;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        let mut checker = Checker::new();
        checker.check(&program);
        checker.warnings()
    }

    #[test]
    fn top_level_const_system_list_is_accepted_by_simulate() {
        // dogfood feature seq 22: a top-level immutable `let SET = [system::…]`
        // const-folds to a static schedule, so it is accepted where an inline
        // literal is — no more copy-pasting the schedule at every call site.
        let errors = check_src(
            r#"
            component P { v: 0 }
            system Step(p: mut P) { p = P { v: p.v + 1 } }
            let ROLLOUT = [system::Step]
            fn go() -> int {
                let f = fork()
                let after = simulate(f, ROLLOUT, 3)
                let g = simulate_par(f, ROLLOUT, 3, 2, 42)
                return 0
            }
            "#,
        );
        assert!(
            errors.is_empty(),
            "const system list should be accepted by simulate/simulate_par, got: {:?}",
            errors
        );
    }
    #[test]
    fn for_where_accepts_what_if_accepts() {
        // `where` desugars to a body-wrapping `if`, so it inherits if's
        // truthy condition semantics exactly — no separate rule to learn.
        let errors = check_src(
            r#"
            fn main() -> nil {
              for x in [1, 2, 3] where x > 1 {
                print(x)
              }
              for y in [1, 2, 3] where y + 1 {
                print(y)
              }
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "where mirrors if conditions, got: {:?}",
            errors
        );
    }
