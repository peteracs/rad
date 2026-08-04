

fn parse_source(src: &str) -> Program {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    Parser::new(tokens).parse()
}

fn parse_source_with_compat(src: &str) -> Program {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens).with_options(ParserOptions {
        compat_v0_5_dx: true,
    });
    parser.parse()
}

fn parse_source_err(src: &str) -> ParseError {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    parser.parse();
    parser.errors[0].clone()
}

fn parse_source_err_with_no_compat(src: &str) -> ParseError {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens).with_options(ParserOptions {
        compat_v0_5_dx: false,
    });
    parser.parse();
    parser.errors[0].clone()
}

fn parse_source_err_with_compat(src: &str) -> ParseError {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens).with_options(ParserOptions {
        compat_v0_5_dx: true,
    });
    parser.parse();
    parser.errors[0].clone()
}

#[test]
fn parse_system_ref_expr() {
    let prog = parse_source("let x = system::Physics");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert!(matches!(
                &l.value,
                Expr::SystemRef(path, _) if path == &["Physics".to_string()]
            ));
        }
        other => panic!("expected let, got {:?}", other),
    }
}

#[test]
fn parse_system_ref_qualified_path() {
    let prog = parse_source("let x = system::ns::Run");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert!(matches!(
                &l.value,
                Expr::SystemRef(path, _) if path == &["ns".to_string(), "Run".to_string()]
            ));
        }
        other => panic!("expected let, got {:?}", other),
    }
}

#[test]
fn parse_schedule_accepts_system_refs() {
    let prog = parse_source(
        "component C { x: 0 }\nsystem Tick(p: C) {}\nfn main() { schedule [system::Tick] }",
    );
    let decl = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            Decl::Fn(f) if f.name == "main" => Some(f),
            _ => None,
        })
        .expect("main");
    let stmt = decl.body.stmts.first().expect("schedule stmt");
    match stmt {
        Stmt::Schedule(s) => {
            assert_eq!(s.systems, vec!["Tick".to_string()]);
            assert!(!s.serial, "plain schedule is not serial");
        }
        _ => panic!("expected schedule, got {:?}", stmt),
    }
}

#[test]
fn parse_schedule_serial_soft_keyword() {
    // dogfood feature seq 83: `schedule serial [ ... ]`.
    let prog = parse_source("fn main() -> nil { schedule serial [Tick, Tock] }");
    let decl = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            Decl::Fn(f) if f.name == "main" => Some(f),
            _ => None,
        })
        .expect("main");
    match decl.body.stmts.first().expect("schedule stmt") {
        Stmt::Schedule(s) => {
            assert_eq!(s.systems, vec!["Tick".to_string(), "Tock".to_string()]);
            assert!(s.serial);
        }
        other => panic!("expected schedule, got {:?}", other),
    }
}

#[test]
fn parse_serial_phase_decl() {
    // dogfood feature seq 83: `serial phase P [A, B]` — and `serial` stays
    // usable as a plain identifier.
    let prog = parse_source("serial phase Line [Feed, Advance]\nlet serial = 1");
    match &prog.declarations[0] {
        Decl::Phase(p) => {
            assert_eq!(p.name, "Line");
            assert_eq!(p.systems, vec!["Feed".to_string(), "Advance".to_string()]);
            assert!(p.serial);
        }
        other => panic!("expected phase, got {:?}", other),
    }
    match &prog.declarations[1] {
        Decl::Stmt(Stmt::Let(l)) => assert_eq!(l.names, vec!["serial".to_string()]),
        other => panic!("expected let serial = 1, got {:?}", other),
    }
}

#[test]
fn parse_let_stmt() {
    let prog = parse_source("let x = 42");
    assert_eq!(prog.declarations.len(), 1);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert_eq!(l.names, vec!["x".to_string()]);
            assert!(!l.tuple_destructure);
            assert!(!l.mutable);
        }
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_let_stmt_with_type_annotation() {
    let prog = parse_source("let x: int = 42");
    assert_eq!(prog.declarations.len(), 1);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert_eq!(l.names, vec!["x".to_string()]);
            assert!(!l.tuple_destructure);
            assert_eq!(l.type_annotation, Some(TypeExpr::Named("int".to_string())));
        }
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_let_tuple_destructure() {
    let prog = parse_source("let (a, b) = (1, 2)");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert_eq!(l.names, vec!["a".to_string(), "b".to_string()]);
            assert!(l.tuple_destructure);
            assert!(!l.mutable);
        }
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_component_decl() {
    let prog = parse_source("component Position { x: 0.0, y: 0.0 }");
    assert_eq!(prog.declarations.len(), 1);
    match &prog.declarations[0] {
        Decl::Component(c) => {
            assert_eq!(c.name, "Position");
            assert_eq!(c.fields.len(), 2);
        }
        other => panic!("Expected Component, got {:?}", other),
    }
}

#[test]
fn parse_system_with_ordering() {
    let prog = parse_source("system Render(p: Position) after Physics { print(p.x) }");
    match &prog.declarations[0] {
        Decl::System(s) => {
            assert_eq!(s.name, "Render");
            assert_eq!(s.after, vec!["Physics"]);
        }
        other => panic!("Expected System, got {:?}", other),
    }
}

#[test]
fn parse_pipe_expr() {
    let prog = parse_source("let x = [1, 2, 3] |> len");
    assert_eq!(prog.declarations.len(), 1);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::Pipe(lhs, rhs, _) => {
                assert!(matches!(lhs.as_ref(), Expr::ListLit(_, _)));
                assert!(matches!(rhs.as_ref(), Expr::Ident(name, _) if name == "len"));
            }
            other => panic!("Expected Pipe expression, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_pipe_then_division_expr() {
    let prog =
        parse_source("let avg = scores |> reduce(0, fn(a, b) { return a + b }) / len(scores)");
    assert_eq!(prog.declarations.len(), 1);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::Pipe(lhs, rhs, _) => {
                assert!(matches!(lhs.as_ref(), Expr::Ident(name, _) if name == "scores"));
                assert!(matches!(rhs.as_ref(), Expr::Binary(_, BinOp::Div, _, _)));
            }
            other => panic!("Expected pipe expression over division, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_state_identifier_in_let() {
    let prog = parse_source("let state = 1");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert_eq!(l.names, vec!["state".to_string()]);
            assert!(!l.tuple_destructure);
            assert!(matches!(l.value, Expr::IntLit(1, _)));
        }
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_state_transitions_accept_optional_commas() {
    let prog = parse_source(
        r#"
            state RequestState {
                Draft { on submit -> WaitingManager }
                WaitingManager { on manager_approve -> WaitingFinance, on reject -> Rejected when true }
                WaitingFinance {
                    on finance_approve -> Approved,
                    on reject -> Rejected
                }
                Approved {}
                Rejected {}
            }
        "#,
    );

    match &prog.declarations[0] {
        Decl::State(s) => {
            assert_eq!(s.name, "RequestState");
            assert_eq!(s.states.len(), 5);

            let waiting_manager = &s.states[1];
            assert_eq!(waiting_manager.name, "WaitingManager");
            assert_eq!(waiting_manager.transitions.len(), 2);
            assert_eq!(waiting_manager.transitions[0].0, "manager_approve");
            assert_eq!(waiting_manager.transitions[0].1, "WaitingFinance");
            assert!(waiting_manager.transitions[0].2.is_none());
            assert_eq!(waiting_manager.transitions[1].0, "reject");
            assert_eq!(waiting_manager.transitions[1].1, "Rejected");
            assert!(matches!(
                waiting_manager.transitions[1].2,
                Some(Expr::BoolLit(true, _))
            ));
        }
        other => panic!("Expected State, got {:?}", other),
    }
}

#[test]
fn parse_state_transitions_reject_comma_before_first_on() {
    let err = parse_source_err(
        r#"
            state Door {
                Closed { , on open -> Open }
                Open {}
            }
        "#,
    );
    assert!(err.message.contains("Expected On, got Comma"));
}

#[test]
fn parse_state_transitions_reject_double_commas() {
    let err = parse_source_err(
        r#"
            state Door {
                Closed { on open -> Open,, on lock -> Closed }
                Open {}
            }
        "#,
    );
    assert!(err.message.contains("Expected On, got Comma"));
}

#[test]
fn parse_fn_decl_with_type_annotations() {
    let prog = parse_source("fn add(a: int, b: int) -> int { return a + b }");
    match &prog.declarations[0] {
        Decl::Fn(f) => {
            assert_eq!(f.params, vec!["a", "b"]);
            assert_eq!(
                f.param_types,
                vec![
                    Some(TypeExpr::Named("int".to_string())),
                    Some(TypeExpr::Named("int".to_string()))
                ]
            );
            assert_eq!(f.return_type, Some(TypeExpr::Named("int".to_string())));
        }
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn parse_event_effect_fn_decl() {
    let mut lexer = Lexer::new("event fn publish() { emit Done {} }");
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let prog = parser.parse();
    assert!(parser.errors().is_empty(), "{:?}", parser.errors());
    match &prog.declarations[0] {
        Decl::Fn(f) => {
            assert_eq!(f.name, "publish");
            assert_eq!(f.effects, vec!["event".to_string()]);
        }
        other => panic!("Expected effect-annotated Fn, got {:?}", other),
    }
}

#[test]
fn parse_event_decl_still_wins_without_fn() {
    let prog = parse_source("event Done { id: int }");
    match &prog.declarations[0] {
        Decl::Event(e) => assert_eq!(e.name, "Done"),
        other => panic!("Expected Event, got {:?}", other),
    }
}

#[test]
fn parse_combined_effect_fn_decl_with_event() {
    let mut lexer = Lexer::new("io event fn publish() { print(\"x\") }");
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let prog = parser.parse();
    assert!(parser.errors().is_empty(), "{:?}", parser.errors());
    match &prog.declarations[0] {
        Decl::Fn(f) => {
            assert_eq!(f.name, "publish");
            assert_eq!(f.effects, vec!["io".to_string(), "event".to_string()]);
        }
        other => panic!("Expected effect-annotated Fn, got {:?}", other),
    }
}

#[test]
fn parse_generic_fn_decl() {
    let prog = parse_source("fn identity<T>(x: T) -> T { return x }");
    match &prog.declarations[0] {
        Decl::Fn(f) => {
            assert_eq!(f.name, "identity");
            assert_eq!(f.type_params, vec!["T".to_string()]);
            assert_eq!(f.param_types, vec![Some(TypeExpr::Named("T".to_string()))]);
            assert_eq!(f.return_type, Some(TypeExpr::Named("T".to_string())));
        }
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn parse_type_alias_decl() {
    let prog = parse_source("type UserId = int");
    match &prog.declarations[0] {
        Decl::TypeAlias(alias) => {
            assert_eq!(alias.name, "UserId");
            assert!(alias.type_params.is_empty());
            assert_eq!(alias.target, TypeExpr::Named("int".to_string()));
        }
        other => panic!("Expected TypeAlias, got {:?}", other),
    }
}

#[test]
fn parse_fn_expr_with_type_annotations() {
    let prog = parse_source("let f = fn(x: int) -> int { return x }");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FnExpr(params, _, param_types, _, return_type, _, _) => {
                assert_eq!(params, &vec!["x".to_string()]);
                assert_eq!(param_types, &vec![Some(TypeExpr::Named("int".to_string()))]);
                assert_eq!(return_type, &Some(TypeExpr::Named("int".to_string())));
            }
            other => panic!("Expected FnExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_fn_expr_with_destructure_params() {
    let prog = parse_source("let f = fn([name, phase], acc: int) { return phase + acc }");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FnExpr(params, _, param_types, param_destructures, _, _, _) => {
                assert_eq!(params, &vec!["__dp_0".to_string(), "acc".to_string()]);
                assert_eq!(
                    param_destructures,
                    &vec![Some(vec!["name".to_string(), "phase".to_string()]), None]
                );
                assert_eq!(
                    param_types,
                    &vec![None, Some(TypeExpr::Named("int".to_string()))]
                );
            }
            other => panic!("Expected FnExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_fn_expr_destructure_rejects_duplicate_bindings() {
    let err = parse_source_err("let f = fn([x, x]) { return x }");
    assert!(err.message.contains("Duplicate binding"));
}

#[test]
fn parse_fn_type_rejects_bare_fn() {
    let err = parse_source_err("let f: fn = fn() {}");
    assert!(err.message.contains("Bare 'fn' is not a valid type"));

    let err2 = parse_source_err("fn f() -> fn { 1 }");
    assert!(err2.message.contains("Bare 'fn' is not a valid type"));
}

#[test]
fn parse_match_qualified_variant_path() {
    let prog = parse_source(
        r#"
            match k {
                lex.Tok.IntLit { n } => { print(n) }
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Match(m)) => {
            assert_eq!(m.cases.len(), 1);
            match &m.cases[0].pattern {
                Pattern::Variant { path, .. } => {
                    assert_eq!(path, &vec!["lex", "Tok", "IntLit"]);
                }
                other => panic!("Expected Variant pattern, got {:?}", other),
            }
        }
        other => panic!("Expected Match, got {:?}", other),
    }
}

#[test]
fn parse_match_with_bindings() {
    let prog = parse_source(
        r#"
            match x {
                Some { value } => { print(value) }
                None => { print("none") }
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Match(m)) => {
            assert_eq!(m.cases.len(), 2);
            assert!(
                matches!(m.cases[0].pattern, Pattern::Variant { ref path, .. } if path.last().unwrap() == "Some")
            );
            match &m.cases[0].pattern {
                Pattern::Variant { bindings, .. } => {
                    assert_eq!(bindings, &vec!["value".to_string()])
                }
                _ => panic!("Expected Variant pattern"),
            }
        }
        other => panic!("Expected Match, got {:?}", other),
    }
}

#[test]
fn parse_match_expression_in_let() {
    let prog = parse_source(
        r#"
            let x = match v {
                Some { value } => { value }
                None => { 0 }
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::MatchExpr(m, _) => {
                assert_eq!(m.cases.len(), 2);
                assert!(
                    matches!(m.cases[0].pattern, Pattern::Variant { ref path, .. } if path.last().unwrap() == "Some")
                );
            }
            other => panic!("Expected MatchExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_let_else_some() {
    let prog = parse_source(
        r#"
            let Some { value: hp } = opt else {
                print("none")
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::LetElse(le)) => {
            assert_eq!(le.variant_name, "Some");
            assert_eq!(le.primary_binding_name(), Some("hp".to_string()));
            assert!(!le.mutable);
            assert!(le.type_annotation.is_none());
        }
        other => panic!("Expected LetElse, got {:?}", other),
    }
}

#[test]
fn parse_let_else_ok_with_mut_and_annotation() {
    let prog = parse_source(
        r#"
            let mut Ok { value: x }: int = r else {
                print("err")
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::LetElse(le)) => {
            assert_eq!(le.variant_name, "Ok");
            assert!(le.mutable);
            assert_eq!(le.type_annotation, Some(TypeExpr::Named("int".to_string())));
            assert_eq!(le.primary_binding_name(), Some("x".to_string()));
        }
        other => panic!("Expected LetElse, got {:?}", other),
    }
}

#[test]
fn parse_match_with_nested_destructuring_and_guard() {
    let prog = parse_source_with_compat(
        r#"
            match ev {
                Alarm { meta: { code }, level: sev } when sev > 2 => { print(code) }
                Alarm { .. } => { print("low") }
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Match(m)) => {
            assert_eq!(m.cases.len(), 2);
            let case0 = &m.cases[0];
            assert!(case0.guard.is_some());
            match &case0.pattern {
                Pattern::Variant {
                    pattern_bindings, ..
                } => {
                    assert_eq!(pattern_bindings.len(), 2);
                    assert_eq!(pattern_bindings[0].name, "code");
                    assert_eq!(
                        pattern_bindings[0].path,
                        vec!["meta".to_string(), "code".to_string()]
                    );
                    assert_eq!(pattern_bindings[1].name, "sev");
                    assert_eq!(pattern_bindings[1].path, vec!["level".to_string()]);
                }
                _ => panic!("Expected Variant pattern"),
            }
        }
        other => panic!("Expected Match, got {:?}", other),
    }
}

#[test]
fn parse_match_rest_binding_requires_compat_flag() {
    let err = parse_source_err_with_no_compat(
        r#"
            match x {
                Some { value, .. } => { print(value) }
                None => { print("none") }
            }
        "#,
    );
    assert!(err.message.contains("requires --compat-v0.5-dx"));
}

#[test]
fn parse_match_rest_binding_with_compat_flag() {
    let prog = parse_source_with_compat(
        r#"
            match x {
                Some { value, .. } => { print(value) }
                None => { print("none") }
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Match(m)) => {
            assert_eq!(m.cases.len(), 2);
            match &m.cases[0].pattern {
                Pattern::Variant {
                    bindings, has_rest, ..
                } => {
                    assert_eq!(bindings, &vec!["value".to_string()]);
                    assert!(*has_rest);
                }
                _ => panic!("Expected Variant pattern"),
            }
            match &m.cases[1].pattern {
                Pattern::Variant { has_rest, .. } => assert!(!*has_rest),
                _ => panic!("Expected Variant pattern"),
            }
        }
        other => panic!("Expected Match, got {:?}", other),
    }
}

#[test]
fn parse_match_rest_binding_must_be_last() {
    let err = parse_source_err_with_compat(
        r#"
            match x {
                Some { .., value } => { print(value) }
                None => { print("none") }
            }
        "#,
    );
    assert!(err.message.contains("must be the final entry"));
}

#[test]
fn parse_zero_field_variant_shorthand_stays_qualified_ref() {
    let prog = parse_source_with_compat("let s = AccessSignal::MfaDisabled");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert!(matches!(
                &l.value,
                Expr::StateRef(machine, state, _) if machine == "AccessSignal" && state == "MfaDisabled"
            ));
        }
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_component_literal_with_non_ident_key_is_disambiguated() {
    let prog = parse_source(r#"let p = Position { "x": 1 }"#);
    assert_eq!(prog.declarations.len(), 2);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert!(matches!(&l.value, Expr::Ident(name, _) if name == "Position"));
        }
        other => panic!("Expected Let with Ident, got {:?}", other),
    }
    match &prog.declarations[1] {
        Decl::Stmt(Stmt::Expr(e)) => {
            assert!(matches!(&e.expr, Expr::MapLit(..)));
        }
        other => panic!("Expected map literal expression, got {:?}", other),
    }
}

#[test]
fn parse_variant_literal_with_non_ident_key_is_disambiguated() {
    let prog = parse_source(r#"let v = Shape::Circle { "r": 1 }"#);
    assert_eq!(prog.declarations.len(), 2);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert!(matches!(&l.value, Expr::StateRef(m, s, _) if m == "Shape" && s == "Circle"));
        }
        other => panic!("Expected Let with StateRef, got {:?}", other),
    }
    match &prog.declarations[1] {
        Decl::Stmt(Stmt::Expr(e)) => {
            assert!(matches!(&e.expr, Expr::MapLit(..)));
        }
        other => panic!("Expected map literal expression, got {:?}", other),
    }
}

#[test]
fn parse_component_literal_missing_colon_errors() {
    let err = parse_source_err(r#"let p = Position { x 1 }"#);
    assert!(err.message.contains("Expected Colon"));
}

#[test]
fn parse_component_update_with_rest_spread() {
    let prog = parse_source("let next = Stats { hp: 42, ..prev }");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::ComponentExpr(name, fields, rest, _) => {
                assert_eq!(name, "Stats");
                assert_eq!(fields.len(), 1);
                assert!(matches!(
                    fields[0],
                    (ref field_name, Expr::IntLit(42, _)) if field_name == "hp"
                ));
                assert!(matches!(
                    rest.as_deref(),
                    Some(Expr::Ident(base, _)) if base == "prev"
                ));
            }
            other => panic!("Expected ComponentExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_for_loop_does_not_treat_body_as_component_literal() {
    let prog = parse_source(
        r#"
            let items = [1, 2, 3]
            for x in items {
                print(x)
            }
        "#,
    );
    match &prog.declarations[1] {
        Decl::Stmt(Stmt::For(f)) => {
            assert!(matches!(f.iterable, Expr::Ident(ref name, _) if name == "items"));
            assert_eq!(f.body.stmts.len(), 1);
        }
        other => panic!("Expected For, got {:?}", other),
    }
}

#[test]
fn parse_for_loop_with_parentheses_bindings() {
    let prog = parse_source(
        r#"
            for (id, bin_op, span) in query { BinaryOp, Span } {
                print(id)
            }
        "#,
    );
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::For(f)) => {
            assert_eq!(f.bindings, vec!["id", "bin_op", "span"]);
            assert!(matches!(f.iterable, Expr::QueryExpr(..)));
            assert_eq!(f.body.stmts.len(), 1);
        }
        other => panic!("Expected For, got {:?}", other),
    }
}

#[test]
fn parse_for_loop_with_destructure_binding() {
    let prog = parse_source(
        r#"
            let rows = [[1, 2], [3, 4]]
            for [a, b] in rows {
                print(a + b)
            }
        "#,
    );
    match &prog.declarations[1] {
        Decl::Stmt(Stmt::For(f)) => {
            assert_eq!(f.bindings, vec!["__fd_0"]);
            assert_eq!(
                f.destructure_bindings,
                Some(vec!["a".to_string(), "b".to_string()])
            );
        }
        other => panic!("Expected For, got {:?}", other),
    }
}

#[test]
fn parse_fn_expr_empty_destructure_brackets_rejected() {
    let err = parse_source_err("let f = fn([]) { return 1 }");
    assert!(
        err.message.contains("at least one variable name"),
        "expected empty brackets error, got: {}",
        err.message
    );
}

#[test]
fn parse_for_empty_destructure_brackets_rejected() {
    let err = parse_source_err("for [] in rows { print(1) }");
    assert!(
        err.message.contains("at least one variable name"),
        "expected empty brackets error, got: {}",
        err.message
    );
}

#[test]
fn parse_for_destructure_rejects_duplicate_bindings() {
    let err = parse_source_err("for [x, x] in rows { print(x) }");
    assert!(
        err.message.contains("Duplicate binding"),
        "expected duplicate binding error, got: {}",
        err.message
    );
}

#[test]
fn parse_fn_expr_single_element_destructure() {
    let prog = parse_source("let f = fn([x]) { return x }");
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FnExpr(params, _, _, param_destructures, _, _, _) => {
                assert_eq!(params, &vec!["__dp_0".to_string()]);
                assert_eq!(param_destructures, &vec![Some(vec!["x".to_string()])]);
            }
            other => panic!("Expected FnExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_fstring_double_braces_are_literal() {
    let prog = parse_source(r##"let s = f"hello {{world}}""##);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    FStringPart::Lit(s) => assert_eq!(s, "hello {world}"),
                    other => panic!("Expected literal f-string part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_fstring_unescaped_closing_brace_errors() {
    let mut lex = Lexer::new(r#"let s = f"hello } world""#);
    let (_tokens, errors) = lex.tokenize();
    assert!(!errors.is_empty());
    assert!(errors[0].message.contains("Unescaped '}'"));
}

#[test]
fn parse_fstring_unterminated_interpolation_errors() {
    let mut lex = Lexer::new(r#"let s = f"hello {x""#);
    let (_tokens, errors) = lex.tokenize();
    assert!(!errors.is_empty());
}

#[test]
fn parse_fstring_expression_rejects_trailing_tokens() {
    let err = parse_source_err(r#"let s = f"{x y}""#);
    assert!(
        err.message.contains("Unexpected token")
            || err.message.contains("Expected InterpolationEnd"),
        "Unexpected error message: {}",
        err.message
    );
}

#[test]
fn parse_fstring_dollar_brace_interpolation() {
    let prog = parse_source(r#"let s = f"hp=${hp}""#);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], FStringPart::Lit(_)));
                assert!(matches!(parts[1], FStringPart::Expr(_, _)));
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_plain_string_dollar_brace_interpolation() {
    let prog = parse_source(r#"let s = "hp=${hp}""#);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], FStringPart::Lit(_)));
                assert!(matches!(parts[1], FStringPart::Expr(_, _)));
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}