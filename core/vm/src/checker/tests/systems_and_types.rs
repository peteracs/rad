
    #[test]
    fn system_cycle_detection() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Pos".to_string(),
                    fields: vec![("x".to_string(), None, Expr::FloatLit(0.0, span(1)))],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "A".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    accum_params: vec![],
                    body: empty_block(),
                    after: vec!["B".to_string()],
                    before: vec![],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(3),
                    name: "B".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    accum_params: vec![],
                    body: empty_block(),
                    after: vec!["A".to_string()],
                    before: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let cycle_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Circular"))
            .collect();
        assert!(
            !cycle_errors.is_empty(),
            "should detect circular dependency A <-> B"
        );
    }

    #[test]
    fn systems_warn_when_never_run_anywhere() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Pos".to_string(),
                    fields: vec![("x".to_string(), None, Expr::FloatLit(0.0, span(1)))],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "Tick".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    body: empty_block(),
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let _ = checker.check(&program);
        let warnings = checker.warnings();
        assert!(warnings.iter().any(|w| {
            w.message.contains("is declared but never run") && w.message.contains("Tick")
        }));
    }

    #[test]
    fn systems_do_not_warn_when_run_inside_function_blocks() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Pos".to_string(),
                    fields: vec![("x".to_string(), None, Expr::FloatLit(0.0, span(1)))],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "Tick".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    body: empty_block(),
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(3),
                    name: "bootstrap".to_string(),
                    type_params: vec![],
                    params: vec![],
                    param_muts: vec![],
                    param_types: vec![],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(3),
                        stmts: vec![Stmt::If(IfStmt {
                            id: nid(),
                            span: span(4),
                            condition: Expr::BoolLit(true, span(4)),
                            then_block: Block {
                                id: nid(),
                                span: span(4),
                                stmts: vec![Stmt::Schedule(ScheduleStmt {
                                    id: nid(),
                                    span: span(5),
                                    systems: vec!["Tick".to_string()],
                                    serial: false,
                                })],
                            },
                            else_block: None,
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let _ = checker.check(&program);
        let warnings = checker.warnings();
        assert!(!warnings
            .iter()
            .any(|w| w.message.contains("is declared but never run")));
    }

    #[test]
    fn systems_do_not_warn_when_only_invoked_via_simulate_system_refs() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Pos".to_string(),
                    fields: vec![("x".to_string(), None, Expr::FloatLit(0.0, span(1)))],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "Tick".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    body: empty_block(),
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(3),
                    name: "main".to_string(),
                    type_params: vec![],
                    params: vec![],
                    param_muts: vec![],
                    param_types: vec![],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(3),
                        stmts: vec![Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(4),
                            expr: Expr::Call(
                                Box::new(Expr::Ident(
                                    Builtin::Simulate.name().to_string(),
                                    span(4),
                                )),
                                vec![
                                    Expr::Call(
                                        Box::new(Expr::Ident("fork".to_string(), span(4))),
                                        vec![],
                                        span(4),
                                    ),
                                    Expr::ListLit(
                                        vec![Expr::SystemRef(vec!["Tick".to_string()], span(4))],
                                        span(4),
                                    ),
                                    Expr::IntLit(1, span(4)),
                                ],
                                span(4),
                            ),
                        })],
                    },
                    is_pure: false,
                    is_async: false,
                    effects: vec![],
                }),
            ],
        };
        let mut checker = Checker::new();
        let _ = checker.check(&program);
        let warnings = checker.warnings();
        assert!(!warnings
            .iter()
            .any(|w| w.message.contains("is declared but never run")));
    }

    #[test]
    fn simulate_unknown_system_is_compile_error() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "X".to_string(),
                    fields: vec![("v".to_string(), None, Expr::IntLit(0, span(1)))],
                }),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "main".to_string(),
                    type_params: vec![],
                    params: vec![],
                    param_muts: vec![],
                    param_types: vec![],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(2),
                        stmts: vec![Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(3),
                            expr: Expr::Call(
                                Box::new(Expr::Ident(
                                    Builtin::Simulate.name().to_string(),
                                    span(3),
                                )),
                                vec![
                                    Expr::Call(
                                        Box::new(Expr::Ident("fork".to_string(), span(3))),
                                        vec![],
                                        span(3),
                                    ),
                                    Expr::ListLit(
                                        vec![Expr::SystemRef(
                                            vec!["NoSuchSystem".to_string()],
                                            span(3),
                                        )],
                                        span(3),
                                    ),
                                    Expr::IntLit(1, span(3)),
                                ],
                                span(3),
                            ),
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
            errors
                .iter()
                .any(|e| e.message.contains("Unknown system 'NoSuchSystem'")),
            "{errors:?}"
        );
    }

    #[test]
    fn simulate_rejects_string_literal_schedule() {
        let program = Program {
            declarations: vec![Decl::Fn(FnDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "main".to_string(),
                type_params: vec![],
                params: vec![],
                param_muts: vec![],
                param_types: vec![],
                return_type: None,
                body: Block {
                    id: nid(),
                    span: span(1),
                    stmts: vec![Stmt::Expr(ExprStmt {
                        id: nid(),
                        span: span(2),
                        expr: Expr::Call(
                            Box::new(Expr::Ident(Builtin::Simulate.name().to_string(), span(2))),
                            vec![
                                Expr::Call(
                                    Box::new(Expr::Ident("fork".to_string(), span(2))),
                                    vec![],
                                    span(2),
                                ),
                                Expr::ListLit(
                                    vec![Expr::StrLit("S".to_string(), span(2))],
                                    span(2),
                                ),
                                Expr::IntLit(1, span(2)),
                            ],
                            span(2),
                        ),
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
            errors.iter().any(|e| {
                e.message
                    .contains("must be a list of `system::…` references, not string literals")
            }),
            "{errors:?}"
        );
    }

    #[test]
    fn simulate_impure_system_errors_use_each_system_ref_span_line_and_column() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Pos".to_string(),
                    fields: vec![("x".to_string(), None, Expr::FloatLit(0.0, span(1)))],
                }),
                Decl::Event(EventDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "Ev".to_string(),
                    fields: vec![],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(3),
                    name: "PureSys".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    body: empty_block(),
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(4),
                    name: "EmitSys".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    body: Block {
                        id: nid(),
                        span: span(4),
                        stmts: vec![Stmt::Emit(EmitStmt {
                            id: nid(),
                            span: span(4),
                            event_name: "Ev".to_string(),
                            fields: vec![],
                            delay: None,
                        })],
                    },
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
                // `emit` alone is simulation-legal now; EmitSys breaches
                // TRANSITIVELY because Ev's handler performs IO
                Decl::OnHandler(OnHandler {
                    id: nid(),
                    span: span(4),
                    event_name: "Ev".to_string(),
                    param_name: "e".to_string(),
                    body: Block {
                        id: nid(),
                        span: span(4),
                        stmts: vec![Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(4),
                            expr: Expr::Call(
                                Box::new(Expr::Ident("print".to_string(), span(4))),
                                vec![Expr::StrLit("ev".to_string(), span(4))],
                                span(4),
                            ),
                        })],
                    },
                    once: false,
                    is_async: false,
                    has_guard: false,
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(4),
                    name: "PrintSys".to_string(),
                    params: vec![("p".to_string(), false, "Pos".to_string())],
                    body: Block {
                        id: nid(),
                        span: span(4),
                        stmts: vec![Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(4),
                            expr: Expr::Call(
                                Box::new(Expr::Ident("print".to_string(), span(4))),
                                vec![Expr::StrLit("x".to_string(), span(4))],
                                span(4),
                            ),
                        })],
                    },
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                }),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(5),
                    name: "main".to_string(),
                    type_params: vec![],
                    params: vec![],
                    param_muts: vec![],
                    param_types: vec![],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(5),
                        stmts: vec![Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(6),
                            expr: Expr::Call(
                                Box::new(Expr::Ident(
                                    Builtin::Simulate.name().to_string(),
                                    span(6),
                                )),
                                vec![
                                    Expr::Call(
                                        Box::new(Expr::Ident("fork".to_string(), span(6))),
                                        vec![],
                                        span(6),
                                    ),
                                    Expr::ListLit(
                                        vec![
                                            Expr::SystemRef(
                                                vec!["PureSys".to_string()],
                                                span_lc(10, 5),
                                            ),
                                            Expr::SystemRef(
                                                vec!["EmitSys".to_string()],
                                                span_lc(20, 7),
                                            ),
                                            Expr::SystemRef(
                                                vec!["PrintSys".to_string()],
                                                span_lc(30, 11),
                                            ),
                                        ],
                                        span(6),
                                    ),
                                    Expr::IntLit(1, span(6)),
                                ],
                                span(6),
                            ),
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
        let mut breach_spans: Vec<(u32, u32)> = errors
            .iter()
            .filter(|e| e.message.contains("cannot be used in simulate"))
            .map(|e| (e.line, e.col))
            .collect();
        breach_spans.sort();
        assert_eq!(
            breach_spans,
            vec![(20, 7), (30, 11)],
            "expected emit + IO breach on distinct spans; errors={errors:?}"
        );
        assert!(
            errors.iter().any(|e| {
                e.line == 20
                    && e.message.contains("EmitSys")
                    && e.message.contains("handler `on Ev`")
            }),
            "expected handler-based breach on line 20: {errors:?}"
        );
        assert!(
            errors.iter().any(|e| {
                e.line == 30
                    && e.message.contains("PrintSys")
                    && e.message.contains("calls IO builtin")
            }),
            "expected IO breach on line 30: {errors:?}"
        );
    }

    #[test]
    fn simulate_static_schedule_unknown_system_one_error_per_ref_no_builtin_duplicate() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "X".to_string(),
                    fields: vec![("v".to_string(), None, Expr::IntLit(0, span(1)))],
                }),
                Decl::Fn(FnDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(2),
                    name: "main".to_string(),
                    type_params: vec![],
                    params: vec![],
                    param_muts: vec![],
                    param_types: vec![],
                    return_type: None,
                    body: Block {
                        id: nid(),
                        span: span(2),
                        stmts: vec![Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(3),
                            expr: Expr::Call(
                                Box::new(Expr::Ident(
                                    Builtin::Simulate.name().to_string(),
                                    span(3),
                                )),
                                vec![
                                    Expr::Call(
                                        Box::new(Expr::Ident("fork".to_string(), span(3))),
                                        vec![],
                                        span(3),
                                    ),
                                    Expr::ListLit(
                                        vec![
                                            Expr::SystemRef(
                                                vec!["MissingA".to_string()],
                                                span_lc(40, 3),
                                            ),
                                            Expr::SystemRef(
                                                vec!["MissingB".to_string()],
                                                span_lc(50, 9),
                                            ),
                                        ],
                                        span(3),
                                    ),
                                    Expr::IntLit(1, span(3)),
                                ],
                                span(3),
                            ),
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
        let unknown: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Unknown system"))
            .collect();
        assert_eq!(unknown.len(), 2, "{errors:?}");
        let mut positions: Vec<(u32, u32)> = unknown.iter().map(|e| (e.line, e.col)).collect();
        positions.sort();
        assert_eq!(positions, vec![(40, 3), (50, 9)]);
        assert!(
            !errors.iter().any(|e| {
                e.message.contains("simulate()") && e.message.contains("Unknown system")
            }),
            "unknown names should use system-ref diagnostics only, not an extra simulate() summary: {errors:?}"
        );
    }

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
