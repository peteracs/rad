#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
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
    fn mutable_top_level_system_list_is_still_rejected() {
        // Only an immutable binding const-folds; a `let mut` could be
        // reassigned, so it must not qualify.
        let errors = check_src(
            r#"
            component P { v: 0 }
            system Step(p: mut P) { p = P { v: p.v + 1 } }
            let mut ROLLOUT = [system::Step]
            fn go() -> int {
                let f = fork()
                let after = simulate(f, ROLLOUT, 3)
                return 0
            }
            "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("second argument must be a list literal")),
            "mutable system list must still be rejected, got: {:?}",
            errors
        );
    }

    #[test]
    fn simulate_par_diagnostic_names_the_actual_builtin() {
        // The MINOR ask in seq 22: the diagnostic used to say "simulate()"
        // even when the offending call was simulate_par().
        let errors = check_src(
            r#"
            component P { v: 0 }
            system Step(p: mut P) { p = P { v: p.v + 1 } }
            fn go() -> int {
                let f = fork()
                let sched = get_dynamic_list()
                let g = simulate_par(f, sched, 3, 2, 42)
                return 0
            }
            fn get_dynamic_list() -> list<system> { return [system::Step] }
            "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.starts_with("simulate_par()")),
            "diagnostic should name simulate_par(), got: {:?}",
            errors
        );
    }

    /// A library module has no `main`, so its public API must seed
    /// reachability. Otherwise every private helper reached only through a
    /// `pub fn` is reported unused — and since `pub` fns are themselves
    /// exempt from the report, the warning lands on the helper with the
    /// advice "consider removing it", which would break the module.
    #[test]
    fn pub_fn_is_a_reachability_root() {
        let warnings = check_src_warnings(
            "fn helper(x: int) -> int { return x * 2 }\n\
             pub fn exported(x: int) -> int { return helper(x) + 1 }\n",
        );
        let unused: Vec<_> = warnings
            .iter()
            .filter(|w| w.message.contains("Unused function"))
            .collect();
        assert!(
            unused.is_empty(),
            "helper is called by a pub fn and must not be reported unused, got: {:?}",
            warnings
        );
    }

    /// Control for the above: a helper reached from nothing at all is still
    /// dead code and must still be reported.
    #[test]
    fn unreached_private_fn_is_still_unused() {
        let warnings = check_src_warnings(
            "fn orphan(x: int) -> int { return x * 2 }\n\
             pub fn exported(x: int) -> int { return x + 1 }\n",
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("Unused function 'orphan'")),
            "an unreachable private fn should still be reported, got: {:?}",
            warnings
        );
    }

    /// The "does not bind all fields" hint must not recommend `..` when `..`
    /// is rejected in the current mode; it should spell out the discard form
    /// that actually compiles.
    #[test]
    fn partial_variant_binding_hint_is_usable_without_compat() {
        let errors = check_src(
            "type Expr { ENum { num: 0.0 }  EBin { op: \"\", lhs: 0.0, rhs: 0.0 } }\n\
             fn kind(e: Expr) -> str {\n\
                 match e {\n\
                     ENum { num: _n } => { return \"num\" }\n\
                     EBin { op } => { return op }\n\
                 }\n\
             }\n",
        );
        let partial: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("does not bind all fields"))
            .collect();
        assert_eq!(
            partial.len(),
            1,
            "expected one partial-binding error, got: {:?}",
            errors
        );
        let hint = partial[0].hint.clone().unwrap_or_default();
        assert!(
            !hint.contains("Use `..` to ignore"),
            "hint must not recommend `..` outside compat mode, got: {}",
            hint
        );
        assert!(
            hint.contains("lhs: _lhs") && hint.contains("rhs: _rhs") && hint.contains("op"),
            "hint should spell out the working discard form, got: {}",
            hint
        );
    }

    /// Arity errors must print the signature — counting arguments without
    /// saying what they are sends people to the docs for information the
    /// checker already had. Covers: curated builtin names, generated
    /// fallback, and user functions with declared parameter names.
    #[test]
    fn arity_errors_print_signatures() {
        // curated builtin: parameter NAMES, not just types
        let errors = check_src("let f = simulate(fork())");
        let e = errors
            .iter()
            .find(|e| e.message.contains("'simulate' expects 3"))
            .expect("arity error");
        let hint = e.hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("simulate(fork, systems, ticks)"),
            "got hint: {}",
            hint
        );

        // un-curated builtin: signature generated from the type scheme
        let errors = check_src("let j = json_stringify()");
        let e = errors
            .iter()
            .find(|e| e.message.contains("'json_stringify' expects 1"))
            .expect("arity error");
        let hint = e.hint.as_deref().unwrap_or("");
        assert!(hint.contains("json_stringify("), "got hint: {}", hint);

        // user function: declared names and types in the hint
        let errors = check_src(
            "fn heal(target: int, amount: int) -> int { return target + amount }\nlet x = heal(1)",
        );
        let e = errors
            .iter()
            .find(|e| e.message.contains("'heal' expects 2"))
            .expect("arity error");
        let hint = e.hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("heal(target: int, amount: int)"),
            "got hint: {}",
            hint
        );
    }

    /// `"text " + 3` is the most common newcomer trip; the error must say
    /// what to write instead. Subtraction gets no such hint.
    #[test]
    fn str_plus_number_hints_fstring() {
        let errors = check_src("let a = \"count: \" + 3");
        let e = errors
            .iter()
            .find(|e| {
                e.message
                    .contains("Operator Add not defined for str and int")
            })
            .expect("add error");
        let hint = e.hint.as_deref().unwrap_or("");
        assert!(hint.contains("f\""), "got hint: {}", hint);
        assert!(hint.contains("str(x)"), "got hint: {}", hint);

        // flipped operands hint too
        let errors = check_src("let b = 3 + \"count\"");
        let e = errors
            .iter()
            .find(|e| {
                e.message
                    .contains("Operator Add not defined for int and str")
            })
            .expect("add error");
        assert!(e.hint.is_some());

        // minus stays hint-free: there is no f-string fix for subtraction
        let errors = check_src("let c = \"x\" - 1");
        let e = errors
            .iter()
            .find(|e| e.message.contains("Operator Sub not defined"))
            .expect("sub error");
        assert!(e.hint.is_none(), "got hint: {:?}", e.hint);
    }

    #[test]
    fn test_query_readonly_unpack() {
        let src = "
            component Health { hp: 100 }
            component Armor { def: 0 }
            fn main() -> nil {
                for id, h, a in query { Health, Armor } {
                    print(h.hp + a.def)
                }
            }
        ";
        let errors = check_src(src);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_query_single_binding_iterates_entity_ids() {
        // One binding + read-only query: iterable is list<entity_id>, not component unpack.
        let src = "
            component Health { hp: 100 }
            fn main() -> nil {
                for id in query { Health } {
                    print(id)
                }
            }
        ";
        let errors = check_src(src);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_query_unpack_rejects_unknown_component() {
        let src = "
            component Health { hp: 100 }
            fn main() -> nil {
                for id, h, a in query { Health, Unknown } {
                    print(h.hp)
                }
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Unknown component")),
            "Expected error about unknown component, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_shadowing_global_does_not_warn() {
        let src = "
            entity goblin {}
            fn foo() {
                let goblin = spawn()
            }
        ";
        let warnings = check_src_warnings(src);
        assert!(!warnings
            .iter()
            .any(|w| w.message.contains("shadows an existing variable")));
    }

    #[test]
    fn test_shadowing_local_warns() {
        let src = "
            fn foo() {
                let x = 1
                if true {
                    let x = 2
                }
            }
        ";
        let warnings = check_src_warnings(src);
        assert!(warnings
            .iter()
            .any(|w| w.message.contains("shadows an existing variable")));
    }

    #[test]
    fn test_shadowing_same_scope_global_warns() {
        let src = "
            let x = 1
            let x = 1
        ";
        let warnings = check_src_warnings(src);
        assert!(warnings
            .iter()
            .any(|w| w.message.contains("shadows an existing variable")));
    }

    #[test]
    fn test_match_unreachable_case() {
        let src = "
            fn foo() {
                match 1 {
                    _ => {}
                    2 => {}
                }
            }
        ";
        let errors = check_src(src);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Unreachable match case")
                || e.message.contains("unreachable")));
    }

    #[test]
    fn test_match_expr_unify() {
        let src = "
            fn foo() {
                let x: int = match 1 {
                    1 => { 10 }
                    _ => { \"hello\" }
                }
            }
        ";
        let errors = check_src(src);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Mismatched types") || e.message.contains("expected int")));
    }

    #[test]
    fn test_match_partial_destructure() {
        let src = "
            type MySum { A { x: 0, y: 0 } B { } }
            fn foo(m: MySum) {
                match m {
                    A { x } => {}
                    B { } => {}
                }
            }
        ";
        let errors = check_src(src);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Pattern does not bind all fields")
                || e.message.contains("missing fields")));
    }

    #[test]
    fn test_match_guard_scope() {
        let src = "
            type MySum { A { x: 0 } B { } }
            fn foo(m: MySum) {
                match m {
                    A { x } if x > 0 => {}
                    _ => {}
                }
            }
        ";
        let errors = check_src(src);
        assert!(errors.is_empty(), "Errors: {:?}", errors);
    }

    #[test]
    fn test_match_type_narrowing() {
        let src = "
            type MySum { A { x: 0 } B { } }
            fn foo(m: any) {
                match m {
                    A { x } => {
                        let y: MySum = m
                    }
                    B { } => {}
                }
            }
        ";
        let errors = check_src(src);
        assert!(errors.is_empty(), "Errors: {:?}", errors);
    }
    #[test]
    fn test_missing_fields_variant() {
        let src = "
            type Shape { Circle { radius: 0.0, x: 0.0 } }
            fn foo() {
                let c = Shape::Circle { radius: 5.0 }
            }
        ";
        let errors = check_src(src);
        println!("{:?}", errors);
        assert!(errors.iter().any(|e| e.message.contains("Missing field")));
    }

    #[test]
    fn test_missing_fields_component() {
        let src = "
            component Position { x: float, y: float }
            fn foo() {
                let p = Position { x: 5.0 }
            }
        ";
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse();
        println!("{:#?}", program);
        let errors = check_src(src);
        println!("{:?}", errors);
        assert!(errors.iter().any(|e| e.message.contains("Missing field")));
    }
    #[test]
    fn test_duplicate_fields_component() {
        let src = "
            component Position { x: float = 0.0, y: float = 0.0 }
            fn foo() {
                let p = Position { x: 5.0, y: 5.0, x: 6.0 }
            }
        ";
        let errors = check_src(src);
        assert!(
            errors.iter().any(|e| e.message.contains("Duplicate field")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_duplicate_fields_variant() {
        let src = "
            type Shape { Circle { radius: 0.0, x: 0.0 } }
            fn foo() {
                let c = Shape::Circle { radius: 5.0, x: 5.0, x: 6.0 }
            }
        ";
        let errors = check_src(src);
        assert!(
            errors.iter().any(|e| e.message.contains("Duplicate field")),
            "Errors: {:?}",
            errors
        );
    }
    #[test]
    fn test_duplicate_match_bindings() {
        let src = "
            type MySum { A { x: int, y: int } }
            fn foo(m: MySum) {
                match m {
                    A { x, x } => {}
                }
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate binding")),
            "Errors: {:?}",
            errors
        );
    }
    #[test]
    fn test_duplicate_fn_params() {
        let src = "
            fn foo(x: int, x: int) {}
        ";
        let errors = check_src(src);
        println!("{:?}", errors);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("Duplicate parameter") || e.message.contains("duplicate")));
    }

    #[test]
    fn test_duplicate_anon_fn_params() {
        let src = "
            let f = fn(x: int, x: int) -> int { 1 }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate parameter")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_list_concat_incompatible_element_types() {
        let src = "
            fn foo() {
                let a = [1, 2]
                let b = [\"x\"]
                let c = a + b
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("incompatible element types")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_and_or_require_bool_operands() {
        let src = "
            fn foo() {
                let x = 5 and true
            }
        ";
        let errors = check_src(src);
        assert!(
            errors.iter().any(|e| e.message.contains("must be bool")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_unary_neg_requires_numeric() {
        let src = "
            fn foo() {
                let x = -\"hello\"
            }
        ";
        let errors = check_src(src);
        assert!(
            errors.iter().any(|e| e.message.contains("numeric")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_unary_not_requires_bool() {
        let src = "
            fn foo() {
                let x = !42
            }
        ";
        let errors = check_src(src);
        assert!(
            errors.iter().any(|e| e.message.contains("bool")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_list_index_must_be_int() {
        let src = "
            fn foo() {
                let a = [1, 2, 3]
                let x = a[\"hello\"]
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("List index must be int")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_string_index_must_be_int() {
        let src = "
            fn foo() {
                let s = \"hello\"
                let x = s[true]
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("String index must be int")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_duplicate_variant_in_type_decl() {
        let src = "
            type Shape {
                Circle { radius: float }
                Circle { x: int }
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate variant")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_duplicate_field_in_variant_decl() {
        let src = "
            type Shape {
                Circle { radius: float, radius: int }
            }
        ";
        let errors = check_src(src);
        assert!(
            errors.iter().any(|e| e.message.contains("Duplicate field")),
            "Errors: {:?}",
            errors
        );
    }

    #[test]
    fn pipeline_purity_diagnostic_names_impure_builtin() {
        let src = r#"
            component Health { hp: 100 }
            fn use_set(e) {
                set(e, Health { hp: 50 })
            }
            let result = [1, 2, 3] |> map(use_set)
        "#;
        let errors = check_src(src);
        let purity_err = errors.iter().find(|e| e.message.contains("side-effecting"));
        assert!(
            purity_err.is_some(),
            "Expected purity error, got: {:?}",
            errors
        );
        let hint = purity_err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("calls impure builtin 'set'"),
            "Hint should trace to impure builtin 'set', got: {}",
            hint
        );
    }

    #[test]
    fn resource_name_is_usable_as_fn_annotation() {
        // A resource is "structurally identical to a component" (spec
        // §3.1.1) and already spellable in system-parameter position; it
        // must also resolve as a fn parameter/return annotation, or resource
        // values silently degrade to `any` across every function boundary.
        let src = r#"
            resource Bank { gold: int = 10 }
            fn topup(b: Bank) -> Bank {
                return Bank { gold: b.gold + 5 }
            }
        "#;
        let errors = check_src(src);
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("Unknown type annotation")),
            "a resource name must resolve as a fn annotation, got: {:?}",
            errors
        );
    }

    #[test]
    fn query_where_impure_predicate_diagnostic_names_the_call() {
        // A structurally-correct but side-effecting predicate used to render
        // as "expects pure fn(entity) -> bool, got fn(entity) -> bool" — two
        // identical-looking types with no reason given. The diagnostic now
        // says the predicate has side effects and names the offending call.
        let src = r#"
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let picked = query_where(C, fn(id: entity) -> bool {
                print("side effect")
                return true
            })
        "#;
        let errors = check_src(src);
        let err = errors
            .iter()
            .find(|e| e.message.contains("must be a read-only predicate"));
        assert!(
            err.is_some(),
            "expected the read-only-predicate diagnostic, got: {:?}",
            errors
        );
        let hint = err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("impure builtin 'print'"),
            "hint should name the offending call, got: {}",
            hint
        );
    }

    #[test]
    fn query_where_accepts_a_readonly_predicate() {
        // Filtering by component values is query_where's whole purpose:
        // a predicate that only READS the world (`get` + `unwrap`) is
        // accepted. The entity list is snapshotted before the predicate
        // runs, so reads observe a stable world.
        let src = r#"
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let picked = query_where(C, fn(id: entity) -> bool {
                let c = get(id, C) |> unwrap
                return c.v >= 1
            })
        "#;
        let errors = check_src(src);
        assert!(
            errors.is_empty(),
            "a read-only predicate must be accepted, got: {:?}",
            errors
        );
    }

    #[test]
    fn query_where_accepts_a_named_readonly_fn() {
        // A declared `readonly fn` passed by name is exactly as safe as the
        // inline closure — its effect row already proves it cannot write.
        let src = r#"
            component C { v: int = 0 }
            readonly fn rich(id: entity) -> bool {
                let c = get(id, C) |> unwrap
                return c.v >= 10
            }
            let e = spawn("e", C { v: 12 })
            let picked = query_where(C, rich)
        "#;
        let errors = check_src(src);
        assert!(
            errors.is_empty(),
            "a named readonly fn must be accepted, got: {:?}",
            errors
        );
    }

    #[test]
    fn query_map_accepts_a_readonly_mapper() {
        // query_map gets the same read-only contract as query_where: a
        // mapper that reads components is the builtin's whole purpose.
        let src = r#"
            component C { v: int = 0 }
            let e = spawn("e", C { v: 7 })
            let values = query_map(C, fn(id: entity) -> int {
                let c = get(id, C) |> unwrap
                return c.v * 2
            })
        "#;
        let errors = check_src(src);
        assert!(
            errors.is_empty(),
            "a read-only mapper must be accepted, got: {:?}",
            errors
        );
    }

    #[test]
    fn query_map_rejects_a_writing_mapper() {
        // query_map previously enforced NOTHING on the mapper — a `set`
        // inside it sailed through the checker. It now shares query_where's
        // read-only contract.
        let src = r#"
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let values = query_map(C, fn(id: entity) -> int {
                set(id, C { v: 99 })
                return 0
            })
        "#;
        let errors = check_src(src);
        let err = errors
            .iter()
            .find(|e| e.message.contains("must be a read-only mapper"));
        assert!(
            err.is_some(),
            "a writing mapper must be rejected, got: {:?}",
            errors
        );
        let hint = err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("impure builtin 'set'"),
            "hint should name the write, got: {}",
            hint
        );
    }

    #[test]
    fn query_map_rejects_a_resource_writing_mapper() {
        // The escape adversarial verification found: `set_resource` was
        // missing from every purity/effect table (no BuiltinSig, not in
        // is_impure_builtin, not in the effect arms), so a checked mapper
        // could mutate a resource at runtime. Now closed.
        let src = r#"
            resource R { x: int = 0 }
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let values = query_map(C, fn(id: entity) -> int {
                set_resource(R, R { x: 42 })
                return 0
            })
        "#;
        let errors = check_src(src);
        let err = errors
            .iter()
            .find(|e| e.message.contains("must be a read-only mapper"));
        assert!(
            err.is_some(),
            "a set_resource mapper must be rejected, got: {:?}",
            errors
        );
        let hint = err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("impure builtin 'set_resource'"),
            "hint should name set_resource, got: {}",
            hint
        );
    }

    #[test]
    fn query_where_rejects_a_resource_writing_predicate() {
        let src = r#"
            resource R { x: int = 0 }
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let picked = query_where(C, fn(id: entity) -> bool {
                set_resource(R, R { x: 42 })
                return true
            })
        "#;
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("must be a read-only predicate")),
            "a set_resource predicate must be rejected, got: {:?}",
            errors
        );
    }

    #[test]
    fn readonly_fn_cannot_hide_set_resource() {
        // The laundering path: a `readonly fn` whose body writes a resource
        // must be rejected at its declaration (effect violation), so the
        // annotation-trusting consumers (query callbacks, pipelines) never
        // see a lying effect row.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            readonly fn sneaky(id: entity) -> bool {
                set_resource(R, R { x: 9 })
                return true
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("requires ecs effect")),
            "readonly fn writing a resource must be an effect violation, got: {:?}",
            errors
        );
    }

    #[test]
    fn readonly_fn_cannot_hide_load_world() {
        // Same table, heavier write: load_world REPLACES the world. It was
        // missing from builtin_required_effects (along with the whole
        // speculation/persistence write family), so a `readonly fn` could
        // hide it and be trusted by annotation-consuming checks.
        let errors = check_src(
            r#"
            readonly fn evil(id: entity) -> bool {
                let n = load_world("{}")
                return n > 0
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("requires ecs effect")),
            "readonly fn calling load_world must be an effect violation, got: {:?}",
            errors
        );
    }

    #[test]
    fn pure_fn_cannot_call_set_resource() {
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            pure fn f(n: int) -> int {
                set_resource(R, R { x: n })
                return n
            }
        "#,
        );
        assert!(
            !errors.is_empty(),
            "pure fn calling set_resource must be rejected"
        );
    }

    #[test]
    fn pure_fn_callback_param_rejects_impure_argument() {
        // The higher-order laundering hole: `pure fn take(cb: fn() -> any)`
        // accepted an impure named fn and RAN it — a set_resource write
        // escaped through a pure annotation. Fn-typed params of an
        // effect-annotated fn are now promoted to pure fn types, so the
        // impure argument is rejected at the call site.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            fn do_write() -> int {
                set_resource(R, R { x: 77 })
                return 0
            }
            pure fn take(cb: fn() -> int) -> int {
                return cb()
            }
            let n = take(do_write)
        "#,
        );
        assert!(
            errors.iter().any(|e| e.message.contains("expects pure fn")),
            "impure callback into a pure fn's fn-typed param must be rejected, got: {:?}",
            errors
        );
    }

    #[test]
    fn pure_fn_callback_param_accepts_pure_closure() {
        // The promotion must keep higher-order pure fns expressible: a
        // side-effect-free closure satisfies the promoted parameter, and the
        // body may call it (its type is pure, so no effect violation).
        let errors = check_src(
            r#"
            pure fn take(cb: fn(int) -> int) -> int {
                return cb(20)
            }
            let n = take(fn(x: int) -> int { return x + 1 })
        "#,
        );
        assert!(
            errors.is_empty(),
            "pure closure through a pure fn's callback param must check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn readonly_fn_callback_param_rejects_impure_argument() {
        // Same promotion for readonly fns — the other trusted annotation.
        // A readonly row promotes its bare fn params to READONLY callback
        // types (the strongest thing the row can call), so the impure
        // argument is still rejected, now against `readonly fn(...)`.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            fn do_write(v: int) -> int {
                set_resource(R, R { x: v })
                return v
            }
            readonly fn apply(f: fn(int) -> int, v: int) -> int {
                return f(v)
            }
            let n = apply(do_write, 3)
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("expects readonly fn")),
            "impure callback into a readonly fn's fn-typed param must be rejected, got: {:?}",
            errors
        );
    }

    #[test]
    fn pure_fn_cannot_call_unverifiable_any_value() {
        // Belt check: an `any`-typed callee has no effect row to trust, so
        // calling it inside a restricted context is a violation — otherwise
        // `cb: any` would trivially reopen the callback laundering hole.
        let errors = check_src(
            r#"
            pure fn take(cb: any) -> int {
                return cb()
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("cannot be verified")),
            "calling an any-typed value inside a pure fn must be an effect violation, got: {:?}",
            errors
        );
    }

    #[test]
    fn pipeline_direct_stage_rejects_world_write_family() {
        // Direct-stage inconsistency: `saved |> load_world` EXECUTED (world
        // replacement as a pipe stage) while `x |> set` was rejected, because
        // the pipeline gate consults only is_impure_builtin and the
        // speculation/persistence write family was missing from it.
        let errors = check_src(
            r#"
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let saved = save_world()
            let n = saved |> load_world
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("load_world") && e.message.contains("pipeline")),
            "load_world as a direct pipe stage must be rejected, got: {:?}",
            errors
        );
    }

    #[test]
    fn readonly_fn_cannot_hide_sleep_ms() {
        // sleep_ms was IO in builtin_effect but missing from
        // builtin_required_effects, so a `readonly fn` could launder an
        // observable delay into trusted contexts (query callbacks, |>).
        let errors = check_src(
            r#"
            readonly fn nap(id: entity) -> bool {
                sleep_ms(500)
                return true
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("'sleep_ms' requires io effect")),
            "readonly fn calling sleep_ms must be an effect violation, got: {:?}",
            errors
        );
    }

    #[test]
    fn pure_fn_cannot_call_get_resource() {
        // get_resource was absent from every classification table — a world
        // READ that defaulted to pure. It is now ReadECS everywhere, exactly
        // like res(): fine in readonly fns, an effect violation in pure fns.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            pure fn peek_r() -> int {
                let v = get_resource(R)
                return 0
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("'get_resource' requires readonly effect")),
            "pure fn calling get_resource must be an effect violation, got: {:?}",
            errors
        );
        let readonly_ok = check_src(
            r#"
            resource R { x: int = 0 }
            readonly fn peek_r() -> int {
                let v = get_resource(R)
                return 0
            }
        "#,
        );
        assert!(
            readonly_ok.is_empty(),
            "readonly fn calling get_resource must be allowed, got: {:?}",
            readonly_ok
        );
    }

    #[test]
    fn query_where_accepts_a_get_resource_reading_predicate() {
        // get_resource joined every read table INCLUDING the
        // is_readonly_builtin allowlist that the read-tolerant breach finder
        // consults — an inline predicate reading a resource through it is as
        // legal as one using res() (reads are snapshot-safe during
        // iteration). Without the allowlist entry, the new impure BuiltinSig
        // alone would have flipped this from unchecked to over-rejected.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let picked = query_where(C, fn(id: entity) -> bool {
                let r = unwrap(get_resource(R))
                return r.x == 0
            })
        "#,
        );
        assert!(
            errors.is_empty(),
            "a get_resource-reading predicate must be accepted like res(), got: {:?}",
            errors
        );
    }

    #[test]
    fn log_and_metric_are_callable_and_io_classified() {
        // Both had a VM impl, a BuiltinSig, and an IO effect row, but were
        // missing from Builtin::from_name — every call died with "Undefined
        // variable". Now wired, and honest at the annotation boundary (IO).
        let errors = check_src(
            r#"
            log("boot", { "phase": 1 })
            metric("frame", "ms", 16.6, { "scene": "arena" })
        "#,
        );
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("Undefined variable")),
            "log/metric must resolve as builtins, got: {:?}",
            errors
        );
        let laundered = check_src(
            r#"
            readonly fn sneaky_log(id: entity) -> bool {
                log("x", { "k": 1 })
                return true
            }
        "#,
        );
        assert!(
            laundered
                .iter()
                .any(|e| e.message.contains("'log' requires io effect")),
            "readonly fn calling log must be an effect violation, got: {:?}",
            laundered
        );
    }

    #[test]
    fn set_resource_first_class_type_has_real_arity() {
        // The missing BuiltinSig left first-class `set_resource` typed as a
        // 0-arity stub, which unified with `fn() -> any`. It now carries its
        // real 2-parameter shape, so the stub annotation no longer fits.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            let writer: fn() -> nil = set_resource
        "#,
        );
        assert!(
            !errors.is_empty(),
            "a 0-arity fn annotation must no longer fit first-class set_resource"
        );
    }

    #[test]
    fn pure_fn_cannot_hide_world_reads() {
        // save_world/world_digest/why/why_resource/schema_digest/fork_seed
        // were ReadECS in builtin_effect but absent from
        // builtin_required_effects, so a `pure fn` could hide whole-world
        // reads. Now honest: rejected in pure fns, fine in readonly fns.
        let errors = check_src(
            r#"
            pure fn snap() -> str {
                return save_world()
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("'save_world' requires readonly effect")),
            "pure fn calling save_world must be an effect violation, got: {:?}",
            errors
        );
        let readonly_ok = check_src(
            r#"
            readonly fn snap() -> str {
                return save_world()
            }
        "#,
        );
        assert!(
            readonly_ok.is_empty(),
            "readonly fn calling save_world must be allowed, got: {:?}",
            readonly_ok
        );
    }

    #[test]
    fn pipeline_direct_stage_rejects_io_builtins() {
        // `x |> print` executed while `x |> set` was rejected: the
        // direct-stage gate consults only is_impure_builtin, and the IO
        // family was missing from it. Spec §8.4 restricts pipelines to
        // pure/readonly computation — now enforced for IO too.
        let errors = check_src(
            r#"
            let n = 5 |> print
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("impure builtin 'print' inside a pipeline")),
            "print as a direct pipe stage must be rejected, got: {:?}",
            errors
        );
        let arg_errors = check_src(
            r#"
            let xs = [1, 2] |> map(print)
        "#,
        );
        assert!(
            arg_errors
                .iter()
                .any(|e| e.message.contains("side-effecting function 'print'")),
            "bare print as a pipeline callback must be rejected, got: {:?}",
            arg_errors
        );
    }

    #[test]
    fn readonly_fn_type_annotation_accepts_readonly_callback() {
        // `readonly fn(...)` types exist now, so a readonly HO fn can accept
        // a readonly callback — previously inexpressible (fn types carried
        // only a pure bit, and promoted params demanded pure).
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            readonly fn readmy(v: int) -> int {
                return res(R).x + v
            }
            readonly fn apply(f: readonly fn(int) -> int, v: int) -> int {
                return f(v)
            }
            let n = apply(readmy, 3)
        "#,
        );
        assert!(
            errors.is_empty(),
            "readonly callback through a readonly fn(...) param must check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn bare_fn_param_of_readonly_fn_promotes_to_readonly() {
        // Promotion targets the strongest callback type the row can call:
        // a readonly fn's bare `fn(...)` param accepts readonly callbacks.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            readonly fn readmy(v: int) -> int {
                return res(R).x + v
            }
            readonly fn apply(f: fn(int) -> int, v: int) -> int {
                return f(v)
            }
            let n = apply(readmy, 3)
        "#,
        );
        assert!(
            errors.is_empty(),
            "readonly callback into a readonly fn's promoted param must be accepted, got: {:?}",
            errors
        );
    }

    #[test]
    fn readonly_callback_rejected_where_pure_required() {
        // Purity ranks order Pure < Readonly < Impure: a readonly callback
        // must not satisfy a pure fn's promoted (pure) callback param.
        let errors = check_src(
            r#"
            resource R { x: int = 0 }
            readonly fn readmy(v: int) -> int {
                return res(R).x + v
            }
            pure fn take(cb: fn(int) -> int) -> int {
                return cb(1)
            }
            let n = take(readmy)
        "#,
        );
        assert!(
            errors.iter().any(|e| e.message.contains("expects pure fn")),
            "a readonly callback must not satisfy a pure fn's callback param, got: {:?}",
            errors
        );
    }

    #[test]
    fn pure_fn_cannot_call_readonly_typed_callback() {
        // An explicit `readonly fn(...)` param on a PURE fn is expressible,
        // but calling it inside the pure body is an effect violation — the
        // author's contradiction surfaces at the call site.
        let errors = check_src(
            r#"
            pure fn take(cb: readonly fn(int) -> int) -> int {
                return cb(1)
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("readonly function value requires the readonly effect")),
            "calling a readonly callback in a pure body must be an effect violation, got: {:?}",
            errors
        );
    }

    #[test]
    fn readonly_typed_local_flows_into_query_where() {
        // The query gates trust pure/readonly TYPES now, so a readonly
        // callback param can be forwarded straight into query_where.
        let errors = check_src(
            r#"
            component C { v: int = 0 }
            readonly fn scan(pred: readonly fn(entity) -> bool) -> list<entity> {
                return query_where(C, pred)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "a readonly-typed callback param must be usable as a query predicate, got: {:?}",
            errors
        );
    }

    #[test]
    fn query_where_still_rejects_a_writing_predicate() {
        // Reads are in; WRITES stay out — a predicate that mutates world
        // state during iteration is exactly the bug class the rule exists
        // to prevent.
        let src = r#"
            component C { v: int = 0 }
            let e = spawn("e", C { v: 1 })
            let picked = query_where(C, fn(id: entity) -> bool {
                set(id, C { v: 99 })
                return true
            })
        "#;
        let errors = check_src(src);
        let err = errors
            .iter()
            .find(|e| e.message.contains("must be a read-only predicate"));
        assert!(
            err.is_some(),
            "a writing predicate must still be rejected, got: {:?}",
            errors
        );
        let hint = err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("impure builtin 'set'"),
            "hint should name the write, got: {}",
            hint
        );
    }

    #[test]
    fn pipeline_purity_diagnostic_traces_call_chain() {
        let src = r#"
            fn inner(x) {
                print(x)
                return x
            }
            fn middle(x) {
                return inner(x)
            }
            fn outer(x) {
                return middle(x)
            }
            let result = [1, 2, 3] |> map(outer)
        "#;
        let errors = check_src(src);
        let purity_err = errors
            .iter()
            .find(|e| e.message.contains("side-effecting") && e.message.contains("outer"));
        assert!(
            purity_err.is_some(),
            "Expected purity error for 'outer', got: {:?}",
            errors
        );
        let hint = purity_err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("middle") && hint.contains("inner") && hint.contains("print"),
            "Hint should trace through the full chain (middle -> inner -> print), got: {}",
            hint
        );
    }

    #[test]
    fn pipeline_purity_diagnostic_suggests_pure_fn_annotation() {
        let src = r#"
            fn helper(x) {
                print(x)
                return x
            }
            let result = [1, 2, 3] |> map(helper)
        "#;
        let errors = check_src(src);
        let purity_err = errors.iter().find(|e| e.message.contains("side-effecting"));
        assert!(
            purity_err.is_some(),
            "Expected purity error, got: {:?}",
            errors
        );
        let hint = purity_err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("pure fn helper"),
            "Hint should suggest `pure fn helper`, got: {}",
            hint
        );
    }

    #[test]
    fn pipeline_purity_diagnostic_inline_closure_emit() {
        let src = r#"
            event Ping { msg }
            let result = [1, 2, 3] |> map(fn(x) {
                emit Ping { msg: "hello" }
                return x
            })
        "#;
        let errors = check_src(src);
        let callback_err = errors.iter().find(|e| {
            e.message.contains("side-effecting function") && e.message.contains("argument")
        });
        assert!(
            callback_err.is_some(),
            "Expected pipeline callback purity error, got: {:?}",
            errors
        );
        let hint = callback_err.unwrap().hint.as_deref().unwrap_or("");
        assert!(
            hint.contains("emits an event"),
            "Hint should explain the closure emits an event, got: {}",
            hint
        );
    }

    #[test]
    fn unique_binding_rejects_let_alias() {
        let src = r#"
            let unique xs = [1, 2, 3]
            let ys = xs
        "#;
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Cannot alias unique binding")),
            "Expected unique alias error, got: {:?}",
            errors
        );
    }

    #[test]
    fn unique_binding_rejects_passing_as_argument() {
        let src = r#"
            fn keep(v) { return v }
            let unique xs = [1, 2, 3]
            let ys = keep(xs)
        "#;
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Cannot alias unique binding")),
            "Expected unique argument alias error, got: {:?}",
            errors
        );
    }

    #[test]
    fn unique_binding_rejects_closure_capture() {
        let src = r#"
            let unique xs = [1, 2, 3]
            let f = fn() {
                return len(xs)
            }
        "#;
        let errors = check_src(src);
        assert!(
            errors.iter().any(|e| {
                e.message.contains("Cannot alias unique binding") && e.message.contains("capturing")
            }),
            "Expected unique closure-capture error, got: {:?}",
            errors
        );
    }

    #[test]
    fn unique_binding_allows_reassignment_to_same_name() {
        let src = r#"
            let unique mut xs = [1, 2, 3]
            xs = push(xs, 4)
        "#;
        let errors = check_src(src);
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("Cannot alias unique binding")),
            "Did not expect unique alias error, got: {:?}",
            errors
        );
    }

    #[test]
    fn system_cycle_error_has_real_span() {
        let src = "
            component Pos { x: 0 }
            system A(p: Pos) after B {}
            system B(p: Pos) after A {}
        ";
        let errors = check_src(src);
        let cycle_err = errors
            .iter()
            .find(|e| e.message.contains("Circular system dependency"));
        assert!(
            cycle_err.is_some(),
            "Expected cycle error, got: {:?}",
            errors
        );
        let err = cycle_err.unwrap();
        assert!(
            err.line > 0,
            "Cycle error should have a real line number, got line={}",
            err.line
        );
    }

    #[test]
    fn is_operator_validates_state_machine_states() {
        let src = "
            state DoorState {
                Locked { on unlock -> Open }
                Open { on lock -> Locked }
            }
            fn foo(d: DoorState) {
                let x = d is Nonexistent
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Unknown state 'Nonexistent'")),
            "Expected unknown state error, got: {:?}",
            errors
        );
    }

    #[test]
    fn is_operator_accepts_valid_state_machine_state() {
        let src = "
            state DoorState {
                Locked { on unlock -> Open }
                Open { on lock -> Locked }
            }
            fn foo(d: DoorState) {
                let x = d is Locked
            }
        ";
        let errors = check_src(src);
        let is_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Unknown state"))
            .collect();
        assert!(
            is_errors.is_empty(),
            "Should not error on valid state, got: {:?}",
            is_errors
        );
    }

    #[test]
    fn event_handler_param_validates_field_access() {
        let src = "
            event Click { x, y }
            on Click(e) {
                print(e.x)
                print(e.y)
            }
        ";
        let errors = check_src(src);
        let field_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("No field"))
            .collect();
        assert!(
            field_errors.is_empty(),
            "Valid event fields should not error, got: {:?}",
            field_errors
        );
    }

    #[test]
    fn event_handler_param_rejects_unknown_field() {
        let src = "
            event Click { x, y }
            on Click(e) {
                print(e.z)
            }
        ";
        let errors = check_src(src);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("No field 'z' on event 'Click'")),
            "Expected unknown field error, got: {:?}",
            errors
        );
    }

    #[test]
    fn aliased_function_call_resolves() {
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        let src = r#"
            print(math.square(5))
        "#;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();

        let alias_src = "pub fn square(x: int) -> int { return x * x }";
        let mut alias_lexer = Lexer::new(alias_src);
        let alias_tokens = alias_lexer.tokenize().0;
        let alias_program = Parser::new(alias_tokens).parse();

        let mut aliases = std::collections::HashMap::new();
        aliases.insert("math".to_string(), alias_program.declarations);

        let mut checker = Checker::new();
        checker.set_aliases(aliases);
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn aliased_function_field_access_resolves_type() {
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        let src = r#"
            let val = m.helper(10)
        "#;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();

        let alias_src = "pub fn helper(x: int) -> int { return x + 1 }";
        let mut alias_lexer = Lexer::new(alias_src);
        let alias_tokens = alias_lexer.tokenize().0;
        let alias_program = Parser::new(alias_tokens).parse();

        let mut aliases = std::collections::HashMap::new();
        aliases.insert("m".to_string(), alias_program.declarations);

        let mut checker = Checker::new();
        checker.set_aliases(aliases);
        let errors = checker.check(&program);
        assert!(
            errors.is_empty(),
            "Expected no errors for aliased call, got: {:?}",
            errors
        );
    }

    #[test]
    fn query_select_not_in_components_is_error() {
        let errors = check_src(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            fn main() -> nil {
                let x = query { Health } select Armor
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("not in the query set")),
            "Expected 'not in the query set' error, got: {:?}",
            errors
        );
    }

    #[test]
    fn tuple_literal_typechecks() {
        let errors = check_src(
            r#"
            fn main() -> nil {
                let t = (1, "hello", true)
                print(t)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "Expected no errors for tuple literal, got: {:?}",
            errors
        );
    }

    #[test]
    fn let_rec_allows_recursive_closure() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["fact".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: true,
                type_annotation: None,
                value: Expr::FnExpr(
                    vec!["n".to_string()],
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
                            value: Some(Expr::Call(
                                Box::new(Expr::Ident("fact".to_string(), span(2))),
                                vec![Expr::IntLit(1, span(2))],
                                span(2),
                            )),
                        })],
                    },
                    span(1),
                ),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        let undefined_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Undefined variable 'fact'"))
            .collect();
        assert!(
            undefined_errors.is_empty(),
            "let rec should allow self-reference, but got: {:?}",
            undefined_errors
        );
    }

    #[test]
    fn let_rec_rejects_non_closure_rhs() {
        let program = Program {
            declarations: vec![Decl::Stmt(Stmt::Let(let_stmt! {
                id: nid(),
                span: span(1),
                names: vec!["x".to_string()],
                tuple_destructure: false,
                mutable: false,
                recursive: true,
                type_annotation: None,
                value: Expr::IntLit(42, span(1)),
            }))],
        };
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        assert!(
            errors.iter().any(|e| e.message.contains("let rec")),
            "expected `let rec` to reject non-closure RHS, got: {:?}",
            errors
        );
    }

    #[test]
    fn closure_destructure_uses_expected_pipeline_param_types() {
        let errors = check_src(
            r#"
            fn main() -> nil {
                let rows = [(1, 2), (3, 4)]
                let out = rows |> map(fn([a, b]) { return a + b })
                print(out)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "expected closure destructure to typecheck with inferred tuple element type, got: {:?}",
            errors
        );
    }

    #[test]
    fn closure_destructure_reports_tuple_arity_mismatch() {
        let errors = check_src(
            r#"
            fn takes_pair(f: fn((int, int)) -> int) -> int {
                return f((1, 2))
            }
            fn main() -> nil {
                let _ = takes_pair(fn([a, b, c]: (int, int)) { return a })
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("tuple value has 2 elements but 3 bindings")),
            "expected tuple arity mismatch error, got: {:?}",
            errors
        );
    }

    #[test]
    fn closure_cross_destructure_duplicate_name_detected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
                let rows = [[(1, 2), (3, 4)]]
                let _ = rows |> map(fn([a, b], [a, c]) { return a })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate parameter")),
            "expected duplicate parameter error for cross-destructure, got: {:?}",
            errors
        );
    }

    #[test]
    fn closure_destructure_name_conflicts_with_plain_param() {
        let errors = check_src(
            r#"
            fn main() -> nil {
                let rows = [[(1, 2)]]
                let _ = rows |> reduce(0, fn(a, [a, b]) { return a })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate parameter")),
            "expected duplicate parameter error for destructure vs plain param, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_spawn_rejected() {
        let errors = check_src(
            r#"
            resource ClusterPool {
              free_workers: 2
            }
            fn main() -> nil {
              spawn(ClusterPool { free_workers: 1 })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("spawn() cannot add resource")),
            "expected spawn-resource rejection, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_component_name_collision() {
        let errors = check_src(
            r#"
            component Pool { size: 0 }
            resource Pool { size: 0 }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("conflicts with an existing component")),
            "expected name collision error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_update_with_entity_rejected() {
        let errors = check_src(
            r#"
            resource Settings { volume: 50 }
            fn main() -> nil {
              let e = spawn()
              update(e, Settings) { volume = 80 }
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("is a resource; use update(Settings)")),
            "expected update(entity, resource) rejection, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_entities_query_rejected() {
        let errors = check_src(
            r#"
            resource Config { debug: false }
            fn main() -> nil {
              let _ = entities(Config)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("entities() cannot query resource")),
            "expected entities-resource rejection, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_duplicate_declaration_rejected() {
        let errors = check_src(
            r#"
            resource Dup { x: 1 }
            resource Dup { x: 999 }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate resource declaration")),
            "expected duplicate resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_update_inside_system_with_mut_param_rejected() {
        let errors = check_src(
            r#"
            component Tag { label: "" }
            resource Counter { n: 0 }
            system Bad(t: Tag, c: mut Counter) {
              update(Counter) { n = 999 }
            }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("conflicts with mutable system parameter")),
            "expected writeback conflict error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_set_resource_inside_system_with_mut_param_rejected() {
        let errors = check_src(
            r#"
            resource Counter { n: 0 }
            system Bad(c: mut Counter) {
              set_resource(Counter, Counter { n: 500 })
            }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("set_resource(Counter, ...) conflicts with mutable system parameter")),
            "expected set_resource conflict error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_update_with_readonly_param_allowed() {
        let errors = check_src(
            r#"
            resource Config { value: 0 }
            system Reader(c: Config) {
              let _ = c.value
            }
            fn main() -> nil {}
        "#,
        );
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("conflicts with mutable")),
            "readonly resource param should not trigger conflict, got: {:?}",
            errors
        );
    }

    #[test]
    fn get_resource_on_component_rejected() {
        let errors = check_src(
            r#"
            component Foo { x: 1 }
            fn main() -> nil {
              let _ = get_resource(Foo)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("is a component, not a resource")),
            "expected component-not-resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_resource_on_component_rejected() {
        let errors = check_src(
            r#"
            component Foo { x: 1 }
            fn main() -> nil {
              set_resource(Foo, Foo { x: 99 })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("is a component, not a resource")),
            "expected component-not-resource error, got: {:?}",
            errors
        );
    }

    // ---- shift operators (`<<` / `>>` as int expressions) ----

    #[test]
    fn shift_ops_type_as_int() {
        let errors = check_src(
            r#"
            fn f(a: int, b: int) -> int {
              return a << 2 | b >> 3
            }
            fn main() -> nil { print(f(8, 64)) }
        "#,
        );
        assert!(
            errors.is_empty(),
            "int shifts should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn shift_on_float_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let x = 1.5 << 2
              print(x)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("requires int operands")),
            "expected int-operand error for float shift, got: {:?}",
            errors
        );
    }

    #[test]
    fn shift_on_list_expression_points_at_push() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let xs = [1, 2]
              let ys = xs << 3
              print(ys)
            }
        "#,
        );
        let e = errors
            .iter()
            .find(|e| e.message.contains("int left shift"))
            .expect("expected list-shift expression error");
        assert!(
            e.hint.as_deref().is_some_and(|h| h.contains("push(xs, v)")),
            "hint should point at push(), got: {:?}",
            e.hint
        );
    }

    #[test]
    fn statement_level_list_append_still_checks_clean() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let mut xs = [1]
              xs << 2
              xs << 3 << 4
              print(xs)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "list append statements must stay legal, got: {:?}",
            errors
        );
    }

    // ---- builtin shadowing (bindings shadow builtins, like the runtime) ----

    #[test]
    fn calling_shadowed_builtin_is_compile_error_with_rename_hint() {
        let errors = check_src(
            r#"
            fn walk(range: int) -> int {
              let mut acc = 0
              for i in range(0, range) {
                acc = acc + i
              }
              return acc
            }
            fn main() -> nil { print(walk(5)) }
        "#,
        );
        let e = errors
            .iter()
            .find(|e| e.message.contains("Cannot call non-function 'range'"))
            .expect("expected non-callable error for shadowed builtin");
        assert!(
            e.hint
                .as_deref()
                .is_some_and(|h| h.contains("shadows the builtin")),
            "hint should explain the builtin shadow, got: {:?}",
            e.hint
        );
    }

    #[test]
    fn defining_builtin_named_binding_warns() {
        let warnings = check_src_warnings(
            r#"
            fn f(range: int) -> int { return range }
            fn main() -> nil { print(f(1)) }
        "#,
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("shadows the builtin function 'range()'")),
            "expected builtin-shadow warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn fn_typed_binding_named_like_builtin_does_not_warn() {
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let lookup = fn(x: int) -> int { return x + 1 }
              print(lookup(1))
            }
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("shadows the builtin")),
            "fn-typed bindings are callable, no shadow warning expected, got: {:?}",
            warnings
        );
    }

    // ---- pub let ----

    #[test]
    fn pub_let_unused_is_not_warned() {
        let warnings = check_src_warnings(
            r#"
            pub let EXPORTED = 42
            fn main() -> nil {}
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("Unused variable 'EXPORTED'")),
            "pub let is a module export, never 'unused', got: {:?}",
            warnings
        );
    }

    #[test]
    fn private_top_level_let_unused_still_warns() {
        let warnings = check_src_warnings(
            r#"
            let PRIVATE = 42
            fn main() -> nil {}
        "#,
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("Unused variable 'PRIVATE'")),
            "private unused top-level lets must keep warning, got: {:?}",
            warnings
        );
    }

    // ---- indexed update blocks ----

    #[test]
    fn update_indexed_on_non_list_field_rejected() {
        let errors = check_src(
            r#"
            component C { tag: int = 0 }
            fn main() -> nil {
              let e = entity "e" { C {} }
              update(e, C) { tag[0] = 1 }
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("indexed assignment needs an indexable field")),
            "expected non-indexable indexed-update error, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_index_must_be_int() {
        let errors = check_src(
            r#"
            component C { vals: list = [0] }
            fn main() -> nil {
              let e = entity "e" { C {} }
              update(e, C) { vals["x"] = 1 }
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("index for list field 'vals' must be int")),
            "expected int-index error, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_on_list_field_checks_clean() {
        let errors = check_src(
            r#"
            component C { vals: list = [0, 0] }
            fn main() -> nil {
              let e = entity "e" { C {} }
              update(e, C) { vals[1] = 9 }
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "indexed list update should check clean, got: {:?}",
            errors
        );
    }

    // ---- mixed map values widen like mixed lists ----

    #[test]
    fn mixed_map_values_warn_and_widen() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let mixed = {"scores": [1, 2, 3], "owner_count": 3}
              print(mixed["owner_count"])
            }
        "#,
        );
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("Heterogeneous map")),
            "mixed map values should warn, not error, got: {:?}",
            errors
        );
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let mixed = {"scores": [1, 2, 3], "owner_count": 3}
              print(mixed["owner_count"])
            }
        "#,
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("mixed value types")),
            "expected mixed-map warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn map_any_annotation_silences_mixed_value_warning() {
        // The warning's own hint says "annotate the binding as `map<K, any>`
        // to silence this warning" — applied literally, it used to warn
        // identically (dogfood bug seq 58-6d: only the list<any> half of the
        // suppression predicate existed). The annotated binding must be
        // silent; the unannotated case (covered above) must still warn.
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let annotated: map<str, any> = { "n": 1, "s": "two" }
              print(annotated["n"])
            }
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("mixed value types")),
            "map<K, any> annotation must silence the mixed-value warning, got: {:?}",
            warnings
        );

        // Re-assignment to a binding already typed map<K, any> is the same
        // accepted-mixed contract.
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let mut m: map<str, any> = {}
              m = { "n": 1, "s": "two" }
              print(m["n"])
            }
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("mixed value types")),
            "assignment to a map<K, any> binding must not warn, got: {:?}",
            warnings
        );
    }

    #[test]
    fn update_indexed_on_map_field_checks_clean() {
        let errors = check_src(
            r#"
            component Inv { items: map<str, int> = {} }
            fn main() -> nil {
              let e = entity "e" { Inv {} }
              update(e, Inv) { items["sword"] = 1 }
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "map-field keyed update should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_map_key_type_mismatch_rejected() {
        let errors = check_src(
            r#"
            component Inv { items: map<str, int> = {} }
            fn main() -> nil {
              let e = entity "e" { Inv {} }
              update(e, Inv) { items[3] = 1 }
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("key for map field 'items' expects str")),
            "expected map key type error, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_map_value_type_mismatch_rejected() {
        let errors = check_src(
            r#"
            component Inv { items: map<str, int> = {} }
            fn main() -> nil {
              let e = entity "e" { Inv {} }
              update(e, Inv) { items["sword"] = "two" }
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("map field 'items' holds int values")),
            "expected map value type error, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_at_returns_collection_type() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let xs: list<int> = [1, 2]
              let ys: list<int> = set_at(xs, 0, 9)
              let m: map<str, int> = {"a": 1}
              let m2: map<str, int> = set_at(m, "b", 2)
              print(ys)
              print(m2)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "set_at should preserve collection types, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_at_on_map_with_wrong_key_type_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let m: map<str, int> = {"a": 1}
              print(set_at(m, 3, 2))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("set_at() map key expects str")),
            "expected map key error, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_at_on_int_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              print(set_at(5, 0, 1))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("set_at() expects a list or map")),
            "expected non-collection error, got: {:?}",
            errors
        );
    }

    // ---- res(): direct resource access ----

    #[test]
    fn res_reads_resource_with_precise_field_types() {
        let errors = check_src(
            r#"
            resource Rng { s: int = 12345 }
            fn next() -> int {
              return res(Rng).s * 3
            }
            fn main() -> nil { print(next()) }
        "#,
        );
        assert!(
            errors.is_empty(),
            "res(Rng).s should check as int, got: {:?}",
            errors
        );
    }

    #[test]
    fn res_field_typo_is_caught() {
        let errors = check_src(
            r#"
            resource Rng { s: int = 12345 }
            fn main() -> nil {
              print(res(Rng).seed)
            }
        "#,
        );
        let e = errors
            .iter()
            .find(|e| e.message.contains("No field 'seed' on resource 'Rng'"))
            .expect("expected unknown-field error on resource");
        assert!(
            e.hint
                .as_deref()
                .is_some_and(|h| h.contains("Available fields: s")),
            "hint should list resource fields, got: {:?}",
            e.hint
        );
    }

    #[test]
    fn res_on_component_rejected() {
        let errors = check_src(
            r#"
            component Foo { x: int = 1 }
            fn main() -> nil {
              print(res(Foo))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("'Foo' is a component, not a resource")),
            "expected component-not-resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn res_on_unknown_name_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              print(res(Nope))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("unknown resource 'Nope'")),
            "expected unknown-resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn get_in_unannotated_fn_stays_allowed() {
        let errors = check_src(
            r#"
            component C { x: int = 5 }
            fn read_x(e: entity) -> int {
              return unwrap(get(e, C)).x
            }
            fn main() -> nil {
              let e = entity "e" { C {} }
              print(read_x(e))
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "ECS reads in unannotated fns are allowed (inference must not over-restrict), got: {:?}",
            errors
        );
    }

    #[test]
    fn res_rejected_in_pure_fn() {
        let errors = check_src(
            r#"
            resource Cfg { n: int = 1 }
            pure fn bad() -> int {
              return res(Cfg).n
            }
            fn main() -> nil { print(bad()) }
        "#,
        );
        assert!(
            !errors.is_empty(),
            "res() reads world state and must not pass inside `pure fn`"
        );
    }

    // ---- buffcore round: ~, for-where, .field, sum/product, system self ----

    #[test]
    fn bitnot_requires_int() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let x = ~1.5
              print(x)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Bitwise '~' requires int")),
            "expected int-operand error for ~float, got: {:?}",
            errors
        );
    }

    #[test]
    fn bitnot_on_int_checks_clean() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let all = 7
              let revoked = 2
              print(all & ~revoked)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "~int should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn system_self_is_entity_typed() {
        let errors = check_src(
            r#"
            component Tag {}
            component Score { points: int = 0 }
            system bump(Tag, s: mut Score) {
              s.points = s.points + require(self, Score).points
              remove(self, Tag)
            }
            fn main() -> nil {
              let _e = entity "e" { Tag {}, Score {} }
              schedule [bump]
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "self in systems should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn self_undefined_outside_systems() {
        let errors = check_src(
            r#"
            component Score { points: int = 0 }
            fn bad() -> int {
              return require(self, Score).points
            }
            fn main() -> nil { print(bad()) }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Undefined variable 'self'")),
            "self must not leak outside system bodies, got: {:?}",
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

    // ---- spellwork round: emit-after, Result in pub APIs ----

    #[test]
    fn emit_after_requires_int_delay() {
        let errors = check_src(
            r#"
            event Ping { tag: str }
            fn main() -> nil {
              emit Ping { tag: "x" } after "soon"
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("emit ... after expects an int tick count")),
            "expected int-delay error, got: {:?}",
            errors
        );
    }

    #[test]
    fn emit_after_with_int_checks_clean() {
        let errors = check_src(
            r#"
            event Ping { tag: str }
            fn main() -> nil {
              let d = 5
              emit Ping { tag: "x" } after d * 2
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "int delays should check clean, got: {:?}",
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
            !errors.iter().any(|e| e.message.contains("Public API leak")),
            "Result is language vocabulary, not a private type, got: {:?}",
            errors
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
                .any(|e| e.message.contains("all keys must share one type")),
            "mixed map KEYS must stay an error, got: {:?}",
            errors
        );
    }
}
