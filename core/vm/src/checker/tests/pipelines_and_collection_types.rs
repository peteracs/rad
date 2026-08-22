    #[test]
    fn pipeline_rejects_set() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Expr(ExprStmt {
                id: nid(),
                span: span(1),
                expr: Expr::Pipe(
                    Box::new(Expr::ListLit(vec![Expr::IntLit(1, span(1))], span(1))),
                    Box::new(Expr::FnExpr(
                        vec!["x".to_string()],
                        vec![false],
                        vec![None],
                        vec![],
                        None,
                        Block {
                            id: nid(),
                            span: span(1),
                            stmts: vec![Stmt::Expr(ExprStmt {
                                id: nid(),
                                span: span(2),
                                expr: Expr::Call(
                                    Box::new(Expr::Ident("set".to_string(), span(2))),
                                    vec![Expr::IntLit(0, span(2)), Expr::IntLit(0, span(2))],
                                    span(2),
                                ),
                            })],
                        },
                        span(1),
                    )),
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let pipe_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("impure") || e.message.contains("non-pure"))
            .collect();
        assert!(!pipe_errors.is_empty(), "should reject set() in pipeline");
    }

    #[test]
    fn pipeline_rejects_impure_fn() {
        let program = Program {
            declarations: vec![
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "impure_fn".to_string(),
                    type_params: vec![],
                    params: vec!["x".to_string()],
                    param_muts: vec![false],
                    param_types: vec![None],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(2),
                            expr: Expr::Call(
                                Box::new(Expr::Ident("set".to_string(), span(2))),
                                vec![Expr::IntLit(0, span(2)), Expr::IntLit(0, span(2))],
                                span(2),
                            ),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                    ownership_writes: vec![],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(5),
                    expr: Expr::Pipe(
                        Box::new(Expr::ListLit(vec![Expr::IntLit(1, span(5))], span(5))),
                        Box::new(Expr::Call(
                            Box::new(Expr::Ident("impure_fn".to_string(), span(5))),
                            vec![],
                            span(5),
                        )),
                        span(5),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let pipe_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("side-effecting"))
            .collect();
        assert!(
            !pipe_errors.is_empty(),
            "should reject side-effecting function call in pipeline"
        );
    }

    #[test]
    fn pipeline_allows_fn_inferred_pure_when_only_calling_merge() {
        let program = Program {
            declarations: vec![
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "add_flag".to_string(),
                    type_params: vec![],
                    params: vec!["m".to_string()],
                    param_muts: vec![false],
                    param_types: vec![None],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(1),
                        stmts: vec![Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::Call(
                                Box::new(Expr::Ident("merge".to_string(), span(2))),
                                vec![
                                    Expr::Ident("m".to_string(), span(2)),
                                    Expr::MapLit(
                                        vec![(
                                            Expr::StrLit("ok".to_string(), span(2)),
                                            Expr::BoolLit(true, span(2)),
                                        )],
                                        span(2),
                                    ),
                                ],
                                span(2),
                            )),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                    ownership_writes: vec![],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(4),
                    expr: Expr::Pipe(
                        Box::new(Expr::ListLit(
                            vec![Expr::MapLit(
                                vec![(
                                    Expr::StrLit("name".to_string(), span(4)),
                                    Expr::StrLit("A".to_string(), span(4)),
                                )],
                                span(4),
                            )],
                            span(4),
                        )),
                        Box::new(Expr::Call(
                            Box::new(Expr::Ident("map".to_string(), span(4))),
                            vec![Expr::Ident("add_flag".to_string(), span(4))],
                            span(4),
                        )),
                        span(4),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            !errors.iter().any(|e| e
                .message
                .contains("Cannot call non-pure function 'add_flag'")),
            "inferred-pure helper should be accepted in pipeline; errors: {:?}",
            errors
        );
    }

    #[test]
    fn pipeline_rejects_outer_assign() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["counter".to_string()],
                    tuple_destructure: false,
                    mutable: true,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::IntLit(0, span(1)),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::Pipe(
                        Box::new(Expr::ListLit(vec![Expr::IntLit(1, span(3))], span(3))),
                        Box::new(Expr::FnExpr(
                            vec!["x".to_string()],
                            vec![false],
                            vec![None],
                            vec![],
                            None,
                            Block {
                                id: nid(),
                                span: span(3),
                                stmts: vec![Stmt::Assign(AssignStmt {
                                    id: nid(),
                                    span: span(4),
                                    target: Expr::Ident("counter".to_string(), span(4)),
                                    value: Expr::IntLit(1, span(4)),
                                })],
                            },
                            span(3),
                        )),
                        span(3),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let pipe_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("outer variable"))
            .collect();
        assert!(
            !pipe_errors.is_empty(),
            "should reject assigning to outer variable in pipeline"
        );
    }

    #[test]
    fn map_keys_reject_invalid_types() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["m".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: None,
                value: Expr::MapLit(
                    vec![(
                        Expr::FloatLit(1.0, span(1)),
                        Expr::StrLit("a".to_string(), span(1)),
                    )],
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("cannot be used as a map key")));
    }

    #[test]
    fn map_allows_int_keys() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["m".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: false,
                type_annotation: None,
                value: Expr::MapLit(
                    vec![(
                        Expr::IntLit(1, span(1)),
                        Expr::StrLit("a".to_string(), span(1)),
                    )],
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "int keys should be allowed in maps, got: {:?}",
            errors
        );
    }

    #[test]
    fn map_index_type_mismatch() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["m".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::MapLit(
                        vec![(
                            Expr::StrLit("a".to_string(), span(1)),
                            Expr::IntLit(1, span(1)),
                        )],
                        span(1),
                    ),
                })),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(2),
                    expr: Expr::Index(
                        Box::new(Expr::Ident("m".to_string(), span(2))),
                        Box::new(Expr::IntLit(0, span(2))),
                        span(2),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Map key type is str, got int")));
    }

    #[test]
    fn accessor_shorthand_checks_clean_in_pipelines() {
        let errors = check_src(
            r#"
            struct Mod { flat: float = 0.0 }
            fn main() -> nil {
              let mods = [Mod { flat: 1.0 }, Mod { flat: 2.0 }]
              print(mods |> map(.flat) |> sum)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "accessor shorthand should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn sum_requires_list() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              print(sum(5))
            }
        "#,
        );
        assert!(
            !errors.is_empty(),
            "sum(int) must be rejected (expects a list)"
        );
    }

    #[test]
    fn heterogeneous_map_keys_still_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let bad = {"a": 1, 2: 3}
              print(bad)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("all keys must share one type")),
            "mixed map KEYS must stay an error, got: {:?}",
            errors
        );
    }
