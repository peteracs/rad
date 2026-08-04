


    use crate::checker::*;
    use crate::value::Builtin;
    fn span(line: u32) -> Span {
        Span {
            line,
            col: 0,
            file: None,
        }
    }
    fn span_lc(line: u32, col: u32) -> Span {
        Span {
            line,
            col,
            file: None,
        }
    }
    fn nid() -> NodeId {
        NodeId(0)
    }
    fn empty_block() -> Block {
        Block {
            id: nid(),
            span: span(0),
            stmts: vec![],
        }
    }

    fn component_fields(raw: Vec<(String, Option<TypeExpr>, Expr)>) -> Vec<FieldDef> {
        raw.into_iter()
            .map(|(name, type_annotation, default_value)| FieldDef {
                required: false,
                name,
                type_annotation,
                default_value,
                is_indexed: false,
            })
            .collect()
    }

    macro_rules! let_stmt {
        ($($field:tt)*) => {
            LetStmt {
                $($field)*
                is_unique: false,
                is_pub: false,
            }
        };
    }

    macro_rules! component_decl {
        (
            is_pub: $is_pub:expr,
            id: $id:expr,
            span: $span:expr,
            name: $name:expr,
            fields: $fields:expr $(,)?
        ) => {
            DataDecl {
                is_pub: $is_pub,
                id: $id,
                span: $span,
                name: $name,
                kind: DataKind::Component,
                version: 0,
                fields: component_fields($fields),
                indexed_fields: vec![],
            }
        };
        (
            id: $id:expr,
            span: $span:expr,
            name: $name:expr,
            is_pub: $is_pub:expr,
            fields: $fields:expr $(,)?
        ) => {
            DataDecl {
                id: $id,
                span: $span,
                name: $name,
                is_pub: $is_pub,
                kind: DataKind::Component,
                version: 0,
                fields: component_fields($fields),
                indexed_fields: vec![],
            }
        };
    }
    #[test]
    fn immutable_let_reassignment() {
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
                    value: Expr::IntLit(10, span(1)),
                })),
                Decl::Stmt(Stmt::Assign(AssignStmt {
                    id: nid(),
                    span: span(2),
                    target: Expr::Ident("x".to_string(), span(2)),
                    value: Expr::IntLit(20, span(2)),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .message
            .contains("Cannot assign to immutable variable"));
    }
    #[test]
    fn mutable_let_reassignment_ok() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["x".to_string()],
                    tuple_destructure: false,
                    mutable: true,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::IntLit(10, span(1)),
                })),
                Decl::Stmt(Stmt::Assign(AssignStmt {
                    id: nid(),
                    span: span(2),
                    target: Expr::Ident("x".to_string(), span(2)),
                    value: Expr::IntLit(20, span(2)),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn unreachable_code_after_return() {
        let program = Program {
            declarations: vec![Decl::Fn(FnDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "foo".to_string(),
                type_params: vec![],
                params: vec![],
                param_muts: vec![],
                param_types: vec![],
                return_type: Some(TypeExpr::Named("int".to_string())),
                body: Block {
                    id: nid(),
                    span: span(1),
                    stmts: vec![
                        Stmt::Return(ReturnStmt {
                            id: nid(),
                            span: span(2),
                            value: Some(Expr::IntLit(10, span(2))),
                        }),
                        Stmt::Expr(ExprStmt {
                            id: nid(),
                            span: span(3),
                            expr: Expr::IntLit(20, span(3)),
                        }),
                    ],
                },
                is_pure: false,
                is_async: false,
                effects: vec![],
            })],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Unreachable code"));
    }
    #[test]
    fn component_type_mismatch() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Health".to_string(),
                    fields: vec![
                        ("hp".to_string(), None, Expr::IntLit(100, span(1))),
                        ("max".to_string(), None, Expr::IntLit(100, span(1))),
                    ],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::ComponentExpr(
                        "Health".to_string(),
                        vec![
                            (
                                "hp".to_string(),
                                Expr::StrLit("banana".to_string(), span(3)),
                            ),
                            ("max".to_string(), Expr::IntLit(100, span(3))),
                        ],
                        None,
                        span(3),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("expected int, got str"));
    }
    #[test]
    fn exhaustive_match() {
        let program = Program {
            declarations: vec![
                Decl::State(StateDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Door".to_string(),
                    states: vec![
                        StateDef {
                            id: nid(),
                            span: span(2),
                            name: "Locked".to_string(),
                            transitions: vec![("unlock".to_string(), "Closed".to_string(), None)],
                        },
                        StateDef {
                            id: nid(),
                            span: span(3),
                            name: "Closed".to_string(),
                            transitions: vec![("open".to_string(), "Open".to_string(), None)],
                        },
                        StateDef {
                            id: nid(),
                            span: span(4),
                            name: "Open".to_string(),
                            transitions: vec![("close".to_string(), "Closed".to_string(), None)],
                        },
                    ],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(5),
                    names: vec!["door".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::StateRef("Door".to_string(), "Locked".to_string(), span(5)),
                })),
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(6),
                    subject: Expr::Ident("door".to_string(), span(6)),
                    cases: vec![MatchCase {
                        id: nid(),
                        span: span(6),
                        pattern: Pattern::Variant {
                            path: vec!["Locked".to_string()],
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
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let match_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Non-exhaustive"))
            .collect();
        assert_eq!(match_errors.len(), 2);
    }
    #[test]
    fn system_mut_enforcement() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Position".to_string(),
                    fields: vec![
                        ("x".to_string(), None, Expr::FloatLit(0.0, span(1))),
                        ("y".to_string(), None, Expr::FloatLit(0.0, span(1))),
                    ],
                }),
                Decl::System(SystemDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(3),
                    name: "BadSystem".to_string(),
                    params: vec![("pos".to_string(), false, "Position".to_string())],
                    accum_params: vec![],
                    after: vec![],
                    before: vec![],
                    body: Block {
                        id: nid(),
                        span: span(3),
                        stmts: vec![Stmt::Assign(AssignStmt {
                            id: nid(),
                            span: span(4),
                            target: Expr::Field(
                                Box::new(Expr::Ident("pos".to_string(), span(4))),
                                "x".to_string(),
                                span(4),
                            ),
                            value: Expr::FloatLit(10.0, span(4)),
                        })],
                    },
                }),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Cannot mutate field"));
    }
    #[test]
    fn pipeline_purity() {
        let program = Program {
            declarations: vec![
                Decl::Event(EventDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Click".to_string(),
                    fields: vec![("x".to_string(), None), ("y".to_string(), None)],
                }),
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
                                stmts: vec![Stmt::Emit(EmitStmt {
                                    delay: None,
                                    id: nid(),
                                    span: span(4),
                                    event_name: "Click".to_string(),
                                    fields: vec![
                                        ("x".to_string(), Expr::IntLit(1, span(4))),
                                        ("y".to_string(), Expr::IntLit(2, span(4))),
                                    ],
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
        assert!(
            !errors.is_empty(),
            "pipeline with emit should produce errors"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("emit") && e.message.contains("pipeline")),
            "should contain emit-in-pipeline error"
        );
    }
    #[test]
    fn int_to_float_promotion() {
        let program = Program {
            declarations: vec![
                Decl::Component(component_decl! {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Position".to_string(),
                    fields: vec![
                        ("x".to_string(), None, Expr::FloatLit(0.0, span(1))),
                        ("y".to_string(), None, Expr::FloatLit(0.0, span(1))),
                    ],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::ComponentExpr(
                        "Position".to_string(),
                        vec![
                            ("x".to_string(), Expr::IntLit(5, span(3))),
                            ("y".to_string(), Expr::IntLit(10, span(3))),
                        ],
                        None,
                        span(3),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert_eq!(errors.len(), 0, "int should be promotable to float");
    }
    #[test]
    fn arithmetic_rejects_int_plus_str() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Expr(ExprStmt {
                id: nid(),
                span: span(1),
                expr: Expr::Binary(
                    Box::new(Expr::IntLit(1, span(1))),
                    BinOp::Add,
                    Box::new(Expr::StrLit("a".to_string(), span(1))),
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Operator Add not defined")),
            "expected int + str to be rejected, got: {:?}",
            errors
        );
    }
    #[test]
    fn sum_type_exhaustive_match() {
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Shape".to_string(),
                    type_params: vec![],
                    variants: vec![
                        VariantDefNode {
                            name: "Circle".to_string(),
                            annotations: vec![],
                            fields: vec![("radius".to_string(), Expr::FloatLit(0.0, span(1)))],
                        },
                        VariantDefNode {
                            name: "Rect".to_string(),
                            annotations: vec![],
                            fields: vec![
                                ("w".to_string(), Expr::FloatLit(0.0, span(1))),
                                ("h".to_string(), Expr::FloatLit(0.0, span(1))),
                            ],
                        },
                        VariantDefNode {
                            name: "Point".to_string(),
                            annotations: vec![],
                            fields: vec![],
                        },
                    ],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(5),
                    names: vec!["s".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::VariantExpr(
                        "Shape".to_string(),
                        "Circle".to_string(),
                        vec![("radius".to_string(), Expr::FloatLit(5.0, span(5)))],
                        span(5),
                    ),
                })),
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(6),
                    subject: Expr::Ident("s".to_string(), span(6)),
                    cases: vec![MatchCase {
                        id: nid(),
                        span: span(6),
                        pattern: Pattern::Variant {
                            path: vec!["Circle".to_string()],
                            bindings: vec!["radius".to_string()],
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
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let missing: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Non-exhaustive"))
            .collect();
        assert_eq!(missing.len(), 2, "should report Rect and Point as missing");
    }
    #[test]
    fn sum_type_guarded_arms_are_not_exhaustive() {
        // Every variant has an arm, but every arm is guarded. When the
        // guards are false no arm runs and the match evaluates to nil, so
        // this must NOT count as exhaustive.
        let guarded_case = |variant: &str| MatchCase {
            id: nid(),
            span: span(6),
            pattern: Pattern::Variant {
                path: vec![variant.to_string()],
                bindings: vec![],
                pattern_bindings: vec![],
                has_rest: false,
                is_bare_variant: true,
            },
            guard: Some(Expr::Ident("open".to_string(), span(6))),
            body: empty_block(),
        };
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Light".to_string(),
                    type_params: vec![],
                    variants: vec![
                        VariantDefNode {
                            name: "Red".to_string(),
                            annotations: vec![],
                            fields: vec![],
                        },
                        VariantDefNode {
                            name: "Green".to_string(),
                            annotations: vec![],
                            fields: vec![],
                        },
                    ],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(4),
                    names: vec!["open".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::BoolLit(true, span(4)),
                })),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(5),
                    names: vec!["s".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::VariantExpr(
                        "Light".to_string(),
                        "Red".to_string(),
                        vec![],
                        span(5),
                    ),
                })),
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(6),
                    subject: Expr::Ident("s".to_string(), span(6)),
                    cases: vec![guarded_case("Red"), guarded_case("Green")],
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let guarded: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("only covered by guarded arms"))
            .collect();
        assert_eq!(
            guarded.len(),
            2,
            "both guarded variants should be reported, got: {:?}",
            errors
        );
    }
    #[test]
    fn sum_type_field_type_mismatch() {
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Result".to_string(),
                    type_params: vec![],
                    variants: vec![
                        VariantDefNode {
                            name: "Ok".to_string(),
                            annotations: vec![],
                            fields: vec![("value".to_string(), Expr::IntLit(0, span(1)))],
                        },
                        VariantDefNode {
                            name: "Err".to_string(),
                            annotations: vec![],
                            fields: vec![(
                                "message".to_string(),
                                Expr::StrLit("".to_string(), span(1)),
                            )],
                        },
                    ],
                }),
                Decl::Stmt(Stmt::Expr(ExprStmt {
                    id: nid(),
                    span: span(3),
                    expr: Expr::VariantExpr(
                        "Result".to_string(),
                        "Err".to_string(),
                        vec![("message".to_string(), Expr::IntLit(42, span(3)))],
                        span(3),
                    ),
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert_eq!(
            errors.len(),
            1,
            "should detect type mismatch in Err.message"
        );
        assert!(errors[0].message.contains("expected str, got int"));
    }
    #[test]
    fn result_option_exhaustive() {
        let program = Program {
            declarations: vec![
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(1),
                    names: vec!["opt".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::VariantExpr(
                        "Option".to_string(),
                        "Some".to_string(),
                        vec![("value".to_string(), Expr::IntLit(42, span(1)))],
                        span(1),
                    ),
                })),
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(2),
                    subject: Expr::Ident("opt".to_string(), span(2)),
                    cases: vec![MatchCase {
                        id: nid(),
                        span: span(2),
                        pattern: Pattern::Variant {
                            path: vec!["Some".to_string()],
                            bindings: vec!["value".to_string()],
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
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let missing: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Non-exhaustive"))
            .collect();
        assert_eq!(
            missing.len(),
            1,
            "should require None case for Option match"
        );
    }

    #[test]
    fn transition_err_message_is_typed_as_str_in_match_binding() {
        let program = Program {
            declarations: vec![
                Decl::State(StateDecl {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Gate".to_string(),
                    states: vec![
                        StateDef {
                            id: nid(),
                            span: span(2),
                            name: "Closed".to_string(),
                            transitions: vec![("open".to_string(), "Open".to_string(), None)],
                        },
                        StateDef {
                            id: nid(),
                            span: span(3),
                            name: "Open".to_string(),
                            transitions: vec![],
                        },
                    ],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(5),
                    names: vec!["g".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::StateRef("Gate".to_string(), "Closed".to_string(), span(5)),
                })),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(6),
                    names: vec!["opened".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::Pipe(
                        Box::new(Expr::Call(
                            Box::new(Expr::Ident("transition".to_string(), span(6))),
                            vec![
                                Expr::Ident("g".to_string(), span(6)),
                                Expr::StrLit("open".to_string(), span(6)),
                            ],
                            span(6),
                        )),
                        Box::new(Expr::Ident("unwrap".to_string(), span(6))),
                        span(6),
                    ),
                })),
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(7),
                    subject: Expr::Call(
                        Box::new(Expr::Ident("transition".to_string(), span(7))),
                        vec![
                            Expr::Ident("opened".to_string(), span(7)),
                            Expr::StrLit("nope".to_string(), span(7)),
                        ],
                        span(7),
                    ),
                    cases: vec![
                        MatchCase {
                            id: nid(),
                            span: span(8),
                            pattern: Pattern::Variant {
                                path: vec!["Ok".to_string()],
                                bindings: vec!["value".to_string()],
                                pattern_bindings: vec![],
                                has_rest: false,
                                is_bare_variant: false,
                            },
                            guard: None,
                            body: empty_block(),
                        },
                        MatchCase {
                            id: nid(),
                            span: span(9),
                            pattern: Pattern::Variant {
                                path: vec!["Err".to_string()],
                                bindings: vec!["message".to_string()],
                                pattern_bindings: vec![],
                                has_rest: false,
                                is_bare_variant: false,
                            },
                            guard: None,
                            body: Block {
                                id: nid(),
                                span: span(9),
                                stmts: vec![Stmt::Let(let_stmt! {
                                    id: nid(),
                                    span: span(10),
                                    names: vec!["full".to_string()],
                                    tuple_destructure: false,
                                    mutable: false,
                                    recursive: false,
                                    type_annotation: None,
                                    value: Expr::Binary(
                                        Box::new(Expr::StrLit("invalid: ".to_string(), span(10))),
                                        BinOp::Add,
                                        Box::new(Expr::Ident("message".to_string(), span(10))),
                                        span(10),
                                    ),
                                })],
                            },
                        },
                    ],
                })),
            ],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("Operator Add not defined for str")),
            "Err.message should be str for transition Result; errors: {:?}",
            errors
        );
    }
    #[test]
    fn unknown_match_variant_reports_case_span_line() {
        let program = Program {
            declarations: vec![
                Decl::Type(TypeDeclNode {
                    is_pub: false,
                    id: nid(),
                    span: span(1),
                    name: "Option".to_string(),
                    type_params: vec![],
                    variants: vec![
                        VariantDefNode {
                            name: "Some".to_string(),
                            annotations: vec![],
                            fields: vec![("value".to_string(), Expr::IntLit(0, span(1)))],
                        },
                        VariantDefNode {
                            name: "None".to_string(),
                            annotations: vec![],
                            fields: vec![],
                        },
                    ],
                }),
                Decl::Stmt(Stmt::Let(let_stmt! {
                    id: nid(),
                    span: span(2),
                    names: vec!["v".to_string()],
                    tuple_destructure: false,
                    mutable: false,
                    recursive: false,
                    type_annotation: None,
                    value: Expr::VariantExpr(
                        "Option".to_string(),
                        "Some".to_string(),
                        vec![("value".to_string(), Expr::IntLit(1, span(2)))],
                        span(2),
                    ),
                })),
                Decl::Stmt(Stmt::Match(MatchStmt {
                    id: nid(),
                    span: span(10),
                    subject: Expr::Ident("v".to_string(), span(10)),
                    cases: vec![MatchCase {
                        id: nid(),
                        span: span(20),
                        pattern: Pattern::Variant {
                            path: vec!["Bogus".to_string()],
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
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let variant_error = errors
            .iter()
            .find(|e| e.message.contains("Unknown variant 'Bogus'"))
            .expect("expected unknown variant error");
        assert_eq!(variant_error.line, 20);
    }
    #[test]
    fn unknown_state_transition_reports_state_def_span_line() {
        let program = Program {
            declarations: vec![Decl::State(StateDecl {
                is_pub: false,
                id: nid(),
                span: span(1),
                name: "Door".to_string(),
                states: vec![StateDef {
                    id: nid(),
                    span: span(7),
                    name: "Closed".to_string(),
                    transitions: vec![("open".to_string(), "Open".to_string(), None)],
                }],
            })],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let transition_error = errors
            .iter()
            .find(|e| e.message.contains("transitions to unknown state"))
            .expect("expected unknown transition target error");
        assert_eq!(transition_error.line, 7);
    }