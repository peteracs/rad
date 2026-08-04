
    #[test]
    fn anonymous_fn_infers_return_type() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["f".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: None,
                value: Expr::FnExpr(
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    None,
                    Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::IntLit(42, span(2))),
                        })],
                    },
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "unexpected type errors: {:?}", errors);
        let binding = checker.lookup("f").expect("binding for f");
        match binding.ty {
            Ty::Fn { ret, .. } => assert_eq!(*ret, Ty::Int),
            other => panic!("expected function type, got {}", other),
        }
    }
    #[test]
    fn named_function_allows_polymorphic_calls() {
        let program = Program {
            declarations: vec![
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "identity".to_string(),
                    type_params: vec![],
                    params: vec!["x".to_string()],
                    param_muts: vec![false],
                    param_types: vec![None],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::Ident("x".to_string(), span(2))),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("identity".to_string(), span(3))),
                        vec![Expr::IntLit(1, span(3))],
                        span(3),
                    ),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(4),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("identity".to_string(), span(4))),
                        vec![Expr::StrLit("s".to_string(), span(4))],
                        span(4),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "unexpected type errors: {:?}", errors);
    }
    #[test]
    fn eq_rejects_incompatible_types() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Expr(ExprStmt {
                id: nid(),
                span: span(1),
                expr: Expr::Binary(
                    Box::new(Expr::IntLit(1, span(1))),
                    BinOp::Eq,
                    Box::new(Expr::StrLit("x".to_string(), span(1))),
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Cannot compare int and str")));
    }
    #[test]
    fn annotated_param_type_is_checked() {
        let program = Program {
            declarations: vec![
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "id".to_string(),
                    type_params: vec![],
                    params: vec!["x".to_string()],
                    param_muts: vec![false],
                    param_types: vec![Some(TypeExpr::Named("int".to_string()))],
                    return_type: Some(TypeExpr::Named("int".to_string())),
                    body: Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::Ident("x".to_string(), span(2))),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("id".to_string(), span(3))),
                        vec![Expr::StrLit("oops".to_string(), span(3))],
                        span(3),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("expects int, got str")));
    }
    #[test]
    fn annotated_return_type_is_checked() {
        let program = Program {
            declarations: vec![Decl::Fn(FnDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "bad".to_string(),
                type_params: vec![],
                params: vec![],
                param_muts: vec![],
                param_types: vec![],
                return_type: Some(TypeExpr::Named("int".to_string())),
                body: Block {
                    id: nid(),
                    span: span(1),
                    stmts: vec![Stmt::Return(ReturnStmt {
                        id: nid(),
                        span: span(2),
                        value: Some(Expr::StrLit("x".to_string(), span(2))),
                    })],
                },
                is_pure: false,
                is_async: false,
                effects: vec![],
            })],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("declares return type int")));
    }
    #[test]
    fn anonymous_fn_variable_allows_polymorphic_calls() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["f".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::FnExpr(
                        vec!["x".to_string()],
                        vec![false],
                        vec![None],
                        vec![],
                        None,
                        Block {
                            id: nid(),
                            span: span(1),
                            stmts: vec![Stmt::Return(ReturnStmt {
                                id: nid(),
                                span: span(2),
                                value: Some(Expr::Ident("x".to_string(), span(2))),
                            })],
                        },
                        span(1),
                    ),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("f".to_string(), span(3))),
                        vec![Expr::IntLit(5, span(3))],
                        span(3),
                    ),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(4),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("f".to_string(), span(4))),
                        vec![Expr::StrLit("oops".to_string(), span(4))],
                        span(4),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "unexpected type errors: {:?}", errors);
    }
    #[test]
    fn let_annotation_valid() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["x".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: Some(TypeExpr::Named("int".to_string())),
                value: Expr::IntLit(42, span(1)),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "valid annotation should not error: {:?}",
            errors
        );
    }
    #[test]
    fn let_annotation_mismatch() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["x".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: Some(TypeExpr::Named("int".to_string())),
                value: Expr::StrLit("hello".to_string(), span(1)),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            !errors.is_empty(),
            "annotation mismatch should produce error"
        );
        assert!(errors[0].message.contains("Type mismatch"));
    }
    #[test]
    fn let_annotation_gradual_any() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["x".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: None,
                value: Expr::IntLit(10, span(1)),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "unannotated let should infer type without error: {:?}",
            errors
        );
    }
    #[test]
    fn index_assignment_checks_index_and_value_types() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["xs".to_string()],
                    tuple_destructure: false,
                    mutable: true,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::ListLit(vec![Expr::IntLit(1, span(1))], span(1)),
                })),
                Decl::Stmt(Stmt::Assign(AssignStmt {
                    id: nid(),
                    span: span(2),
                    target: Expr::Index(
                        Box::new(Expr::Ident("xs".to_string(), span(2))),
                        Box::new(Expr::StrLit("0".to_string(), span(2))),
                        span(2),
                    ),
                    value: Expr::IntLit(5, span(2)),
                })),
                Decl::Stmt(Stmt::Assign(AssignStmt {
                    id: nid(),
                    span: span(3),
                    target: Expr::Index(
                        Box::new(Expr::Ident("xs".to_string(), span(3))),
                        Box::new(Expr::IntLit(0, span(3))),
                        span(3),
                    ),
                    value: Expr::StrLit("bad".to_string(), span(3)),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("List index must be int")));
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Type error in list assignment")));
    }
    #[test]
    fn field_assignment_checks_local_component_type() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Health".to_string(),
                    fields: vec![("hp".to_string(), None, Expr::IntLit(100, span(1)))],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(2),
                    names: vec!["h".to_string()],
                    tuple_destructure: false,
                    mutable: true,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::ComponentExpr(
                        "Health".to_string(),
                        vec![("hp".to_string(), Expr::IntLit(1, span(2)))],
                        None,
                        span(2),
                    ),
                })),
                Decl::Stmt(Stmt::Assign(AssignStmt {
                    id: nid(),
                    span: span(3),
                    target: Expr::Field(
                        Box::new(Expr::Ident("h".to_string(), span(3))),
                        "hp".to_string(),
                        span(3),
                    ),
                    value: Expr::StrLit("oops".to_string(), span(3)),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Type error in 'Health.hp'")));
    }
    #[test]
    fn calling_non_function_reports_error() {
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
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(2),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("x".to_string(), span(2))),
                        vec![],
                        span(2),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Cannot call non-function 'x'")));
    }
    #[test]
    fn pipe_checks_input_against_function_param_type() {
        let program = Program {
            declarations: vec![
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "expects_str".to_string(),
                    type_params: vec![],
                    params: vec!["s".to_string()],
                    param_muts: vec![false],
                    param_types: vec![Some(TypeExpr::Named("str".to_string()))],
                    return_type: Some(TypeExpr::Named("int".to_string())),
                    body: Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::Call(
                                Box::new(Expr::Ident("len".to_string(), span(2))),
                                vec![Expr::Ident("s".to_string(), span(2))],
                                span(2),
                            )),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::Pipe(
                        Box::new(Expr::IntLit(1, span(3))),
                        Box::new(Expr::Ident("expects_str".to_string(), span(3))),
                        span(3),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("expects str, got int")));
    }
    #[test]
    fn pipeline_nested_scope_allows_local_mutation() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Expr(ExprStmt {
                id: nid(),
                span: span(1),
                expr: Expr::Pipe(
                    Box::new(Expr::IntLit(1, span(1))),
                    Box::new(Expr::FnExpr(
                        vec!["x".to_string()],
                        vec![false],
                        vec![None],
                        vec![],
                        None,
                        Block {
                            id: nid(),
                            span: span(1),
                            stmts: vec![
                                Stmt::Let(let_stmt! {
                                    id: nid(),
                                    span: span(2),
                                    names: vec!["y".to_string()],
                                    tuple_destructure: false,
                                    mutable: true,
                                    recursive: false,
                                    type_annotation: None,
                                    value: Expr::IntLit(0, span(2)),
                                }),
                                Stmt::If(IfStmt {
                                    id: nid(),
                                    span: span(3),
                                    condition: Expr::BoolLit(true, span(3)),
                                    then_block: Block {
                                        id: nid(),
                                        span: span(3),
                                        stmts: vec![Stmt::Assign(AssignStmt {
                                            id: nid(),
                                            span: span(4),
                                            target: Expr::Ident("y".to_string(), span(4)),
                                            value: Expr::IntLit(2, span(4)),
                                        })],
                                    },
                                    else_block: None,
                                }),
                                Stmt::Return(ReturnStmt {
                                    id: nid(),
                                    span: span(5),
                                    value: Some(Expr::Ident("y".to_string(), span(5))),
                                }),
                            ],
                        },
                        span(1),
                    )),
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("Cannot assign to outer variable")),
            "unexpected pipeline mutation error: {:?}",
            errors
        );
    }
    #[test]
    fn system_param_shadowing_does_not_apply_component_field_rules() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Position".to_string(),
                    fields: vec![("x".to_string(), None, Expr::FloatLit(0.0, span(1)))],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "S".to_string(),
                    params: vec![("pos".to_string(), true, "Position".to_string())],
                    body: Block {
                        id: nid(),
                        span: span(2),
                        stmts: vec![
                            Stmt::Let(let_stmt! {
                                id: nid(),
                                span: span(3),
                                names: vec!["pos".to_string()],
                                tuple_destructure: false,
                                mutable: true,
                                recursive: false,
                                type_annotation: None,
                                value: Expr::IntLit(1, span(3)),
                            }),
                            Stmt::Assign(AssignStmt {
                                id: nid(),
                                span: span(4),
                                target: Expr::Field(
                                    Box::new(Expr::Ident("pos".to_string(), span(4))),
                                    "x".to_string(),
                                    span(4),
                                ),
                                value: Expr::FloatLit(1.0, span(4)),
                            }),
                        ],
                    },
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("on value of type int")));
    }
    #[test]
    fn errors_when_push_result_is_ignored() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["xs".to_string()],
                    tuple_destructure: false,
                    mutable: true,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::ListLit(vec![Expr::IntLit(1, span(1))], span(1)),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(2),
                    expr: Expr::Call(
                        Box::new(Expr::Ident("push".to_string(), span(2))),
                        vec![
                            Expr::Ident("xs".to_string(), span(2)),
                            Expr::IntLit(2, span(2)),
                        ],
                        span(2),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Ignored result from 'push'")));
    }
    #[test]
    fn does_not_error_when_push_result_is_rebound() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["xs".to_string()],
                    tuple_destructure: false,
                    mutable: true,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::ListLit(vec![Expr::IntLit(1, span(1))], span(1)),
                })),
                Decl::Stmt(Stmt::Assign(AssignStmt {
                    id: nid(),
                    span: span(2),
                    target: Expr::Ident("xs".to_string(), span(2)),
                    value: Expr::Call(
                        Box::new(Expr::Ident("push".to_string(), span(2))),
                        vec![
                            Expr::Ident("xs".to_string(), span(2)),
                            Expr::IntLit(2, span(2)),
                        ],
                        span(2),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let _errors = checker.check(&program);
        let warnings = checker.warnings();
        assert!(warnings.is_empty());
    }

    #[test]
    fn mixed_list_warning_is_suppressed_by_list_any_annotation() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["row".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: Some(TypeExpr::Generic(
                    "list".to_string(),
                    vec![TypeExpr::Named("any".to_string())],
                )),
                value: Expr::ListLit(
                    vec![
                        Expr::IntLit(1, span(1)),
                        Expr::BoolLit(true, span(1)),
                        Expr::StrLit("x".to_string(), span(1)),
                    ],
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let warnings = checker.warnings();
        assert!(errors.is_empty());
        assert!(!warnings
            .iter()
            .any(|w| w.message.contains("List contains mixed types")));
    }

    #[test]
    fn mixed_list_warning_still_emits_without_annotation() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["row".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: None,
                value: Expr::ListLit(
                    vec![Expr::IntLit(1, span(1)), Expr::BoolLit(true, span(1))],
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let warnings = checker.warnings();
        assert!(errors.is_empty());
        assert!(warnings
            .iter()
            .any(|w| w.message.contains("List contains mixed types")));
    }

    #[test]
    fn mixed_list_warning_is_suppressed_on_assignment_to_list_any() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["row".to_string()],
                    tuple_destructure: false,
                    mutable: true,
                    recursive: false,
                    type_annotation: Some(TypeExpr::Generic(
                        "list".to_string(),
                        vec![TypeExpr::Named("any".to_string())],
                    )),
                    value: Expr::ListLit(vec![Expr::IntLit(1, span(1))], span(1)),
                })),
                Decl::Stmt(Stmt::Assign(AssignStmt {
                    id: nid(),
                    span: span(2),
                    target: Expr::Ident("row".to_string(), span(2)),
                    value: Expr::ListLit(
                        vec![Expr::IntLit(1, span(2)), Expr::BoolLit(true, span(2))],
                        span(2),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let warnings = checker.warnings();
        assert!(errors.is_empty());
        assert!(!warnings
            .iter()
            .any(|w| w.message.contains("List contains mixed types")));
    }

    #[test]
    fn malformed_fn_decl_param_alignment_reports_invariant_error() {
        let program = Program {
            declarations: vec![Decl::Fn(FnDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "bad".to_string(),
                type_params: vec![],
                params: vec!["a".to_string(), "b".to_string()],
                param_muts: vec![false, false],
                param_types: vec![Some(TypeExpr::Named("int".to_string()))],
                return_type: Some(TypeExpr::Named("int".to_string())),
                body: Block {
                    id: nid(),
                    span: span(1),
                    stmts: vec![Stmt::Return(ReturnStmt {
                        id: nid(),
                        span: span(2),
                        value: Some(Expr::IntLit(1, span(2))),
                    })],
                },
                is_pure: false,
                is_async: false,
                effects: vec![],
            })],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Internal AST invariant violated")));
    }
    #[test]
    fn malformed_fn_expr_param_alignment_reports_invariant_error() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["f".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: None,
                value: Expr::FnExpr(
                    vec!["x".to_string(), "y".to_string()],
                    vec![false, false],
                    vec![Some(TypeExpr::Named("int".to_string()))],
                    vec![],
                    Some(TypeExpr::Named("int".to_string())),
                    Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::IntLit(1, span(2))),
                        })],
                    },
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Internal AST invariant violated")));
    }

    #[test]
    fn compat_shorthand_state_ref_resolves_to_zero_field_sum_variant() {
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
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(3),
                    subject: Expr::Ident("sig".to_string(), span(3)),
                    cases: vec![MatchCase {
                        id: nid(),
                        span: span(3),
                        pattern: Pattern::Variant {
                            path: vec!["MfaDisabled".to_string()],
                            bindings: vec![],
                            pattern_bindings: vec![],
                            has_rest: false,
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
            warn_compat: true,
            strict_types: false,
        });
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "compat shorthand should type-check, got: {:?}",
            errors
        );
    }
    #[test]
    fn result_in_pub_api_is_not_a_leak() {
        let errors = check_src(
            r#"
            pub fn try_thing(x: int) -> Result<int, str> {
              if x < 0 { return Err("negative") }
              return Ok(x)
            }
            fn main() -> nil {
              print(try_thing(3) |> unwrap_or(0))
            }
        "#,
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("Public API leak")),
            "Result is language vocabulary, not a private type, got: {:?}",
            errors
        );
    }
