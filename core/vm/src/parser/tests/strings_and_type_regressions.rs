

#[test]
fn parse_triple_fstring_bare_braces_are_literal() {
    let src = "let s = f\"\"\"\nif (x) { y = 1; }\n\"\"\"";
    let prog = parse_source(src);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    FStringPart::Lit(s) => assert_eq!(s, "\nif (x) { y = 1; }\n"),
                    other => panic!("Expected literal part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_triple_fstring_dollar_interpolation() {
    let src = "let s = f\"\"\"\nhello ${name}\n\"\"\"";
    let prog = parse_source(src);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 3);
                match &parts[0] {
                    FStringPart::Lit(s) => assert_eq!(s, "\nhello "),
                    other => panic!("Expected literal part, got {:?}", other),
                }
                assert!(matches!(&parts[1], FStringPart::Expr(_, _)));
                match &parts[2] {
                    FStringPart::Lit(s) => assert_eq!(s, "\n"),
                    other => panic!("Expected trailing literal part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_triple_fstring_mixed_braces_and_interpolation() {
    let src = "let s = f\"\"\"\nif (x) { return ${val}; }\n\"\"\"";
    let prog = parse_source(src);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 3);
                match &parts[0] {
                    FStringPart::Lit(s) => assert_eq!(s, "\nif (x) { return "),
                    other => panic!("Expected literal part, got {:?}", other),
                }
                assert!(matches!(&parts[1], FStringPart::Expr(_, _)));
                match &parts[2] {
                    FStringPart::Lit(s) => assert_eq!(s, "; }\n"),
                    other => panic!("Expected literal part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_triple_fstring_escaped_braces_produce_literal() {
    let src = "let s = f\"\"\"\\{not interp\\}\"\"\"";
    let prog = parse_source(src);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    FStringPart::Lit(s) => assert_eq!(s, "{not interp}"),
                    other => panic!("Expected literal part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_triple_fstring_unterminated_interpolation_errors() {
    let src = "let s = f\"\"\"${unclosed\"\"\"";
    let mut lex = Lexer::new(src);
    let (_tokens, errors) = lex.tokenize();
    assert!(
        !errors.is_empty(),
        "Expected lexer error for unterminated interpolation"
    );
}

#[test]
fn parse_triple_fstring_escaped_dollar_brace_is_literal() {
    let src = "let s = f\"\"\"\\${not_interp}\"\"\"";
    let prog = parse_source(src);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    FStringPart::Lit(s) => assert_eq!(s, "${not_interp}"),
                    other => panic!("Expected literal part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn reserved_keyword_as_identifier_reports_actionable_error() {
    let err = parse_source_err("system S(l: Load) { let match = l.qps }");
    assert!(err.message.contains("reserved keyword 'match'"));
    assert!(err.message.contains("Try: case_value"));
}

#[test]
fn contextual_entity_keyword_is_allowed_as_identifier() {
    let prog = parse_source("fn main() -> nil { let entity = 1 return entity }");
    match &prog.declarations[0] {
        Decl::Fn(f) => {
            assert_eq!(f.name, "main");
            assert!(matches!(f.body.stmts[0], Stmt::Let(_)));
        }
        other => panic!("Expected function declaration, got {:?}", other),
    }
}

#[test]
fn contextual_before_and_after_are_allowed_as_identifiers() {
    let prog = parse_source("system S(l: Load) { let before = 1\nlet after = 2 }");
    match &prog.declarations[0] {
        Decl::System(s) => {
            assert_eq!(s.name, "S");
            assert_eq!(s.body.stmts.len(), 2);
            assert!(matches!(s.body.stmts[0], Stmt::Let(_)));
            assert!(matches!(s.body.stmts[1], Stmt::Let(_)));
        }
        other => panic!("Expected system declaration, got {:?}", other),
    }
}

#[test]
fn event_field_named_entity_parses() {
    let prog = parse_source("event LevelUp { entity, new_level }");
    match &prog.declarations[0] {
        Decl::Event(e) => {
            assert_eq!(e.name, "LevelUp");
            assert_eq!(
                e.fields,
                vec![
                    ("entity".to_string(), None),
                    ("new_level".to_string(), None),
                ]
            );
        }
        other => panic!("Expected event declaration, got {:?}", other),
    }
}

#[test]
fn event_field_with_type_parses() {
    let prog = parse_source("event ChatMsg { sender: int, text: str }");
    match &prog.declarations[0] {
        Decl::Event(e) => {
            assert_eq!(e.name, "ChatMsg");
            assert_eq!(e.fields.len(), 2);
            assert_eq!(e.fields[0].0, "sender");
            assert!(e.fields[0].1.is_some());
            assert_eq!(e.fields[1].0, "text");
            assert!(e.fields[1].1.is_some());
        }
        other => panic!("Expected event declaration, got {:?}", other),
    }
}

#[test]
fn parse_caps_error_count() {
    let mut src = String::new();
    for _ in 0..400 {
        src.push_str("let =\n");
    }
    let mut lexer = Lexer::new(&src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let _program = parser.parse();
    let errors = parser.errors();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("Stopping parse recovery after")),
        "expected capped-recovery diagnostic, got: {:?}",
        errors
    );
}

// === Stress tests for reconstructed parser/recovery.rs ===

#[test]
fn parse_returns_partial_program() {
    let src = "fn foo() { 42 }\nlet =\nfn bar() { 99 }";
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    let errors = parser.errors();
    assert!(
        program.declarations.len() >= 2,
        "expected at least 2 recovered declarations, got {}",
        program.declarations.len()
    );
    assert!(!errors.is_empty(), "expected at least 1 error from 'let ='");
}

#[test]
fn parse_does_not_infinite_loop_on_garbage() {
    let src = "= = = = = = = = = =";
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let _program = parser.parse();
    let errors = parser.errors();
    assert!(!errors.is_empty(), "garbage input should produce errors");
}

#[test]
fn parse_empty_program() {
    let program = parse_source("");
    assert_eq!(program.declarations.len(), 0);
}

#[test]
fn parse_single_expression() {
    let program = parse_source("42");
    assert_eq!(program.declarations.len(), 1);
}

#[test]
fn parse_on_valid_code_produces_no_errors() {
    let src = "let x = 1\nlet y = 2\nlet z = 3";
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    let errors = parser.errors();
    assert_eq!(errors.len(), 0, "valid code should produce no errors");
    assert_eq!(program.declarations.len(), 3);
}

#[test]
fn parse_struct_decl_basic() {
    let program = parse_source("struct Point { x: 0.0, y: 0.0 }");
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Struct(s) => {
            assert_eq!(s.name, "Point");
            assert_eq!(s.fields.len(), 2);
            assert_eq!(s.fields[0].name, "x");
            assert_eq!(s.fields[1].name, "y");
        }
        other => panic!("Expected Decl::Struct, got {:?}", other),
    }
}

#[test]
fn parse_struct_with_type_annotations() {
    let program = parse_source("struct User { name: str = \"\", age: int = 0 }");
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Struct(s) => {
            assert_eq!(s.name, "User");
            assert_eq!(s.fields.len(), 2);
            assert!(s.fields[0].type_annotation.is_some());
            assert!(s.fields[1].type_annotation.is_some());
        }
        other => panic!("Expected Decl::Struct, got {:?}", other),
    }
}

#[test]
fn parse_struct_literal_as_component_expr() {
    let program = parse_source("struct Point { x: 0.0, y: 0.0 }\nlet p = Point { x: 1.0, y: 2.0 }");
    assert_eq!(program.declarations.len(), 2);
    match &program.declarations[0] {
        Decl::Struct(_) => {}
        other => panic!("Expected Decl::Struct, got {:?}", other),
    }
}

#[test]
fn parse_use_with_alias() {
    let program = parse_source(r#"use "math.rad" as math"#);
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Use(u) => {
            assert_eq!(u.path, "math.rad");
            assert_eq!(u.alias, Some("math".to_string()));
        }
        other => panic!("Expected Decl::Use, got {:?}", other),
    }
}

#[test]
fn parse_use_without_alias_still_works() {
    let program = parse_source(r#"use "utils.rad""#);
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Use(u) => {
            assert_eq!(u.path, "utils.rad");
            assert_eq!(u.alias, None);
        }
        other => panic!("Expected Decl::Use, got {:?}", other),
    }
}

#[test]
fn parse_aliased_component_expr() {
    let program = parse_source(r#"let h = ns.Health { hp: 100 }"#);
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::ComponentExpr(name, fields, _, _) => {
                assert_eq!(name, "ns.Health");
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "hp");
            }
            other => panic!("Expected ComponentExpr, got {:?}", other),
        },
        other => panic!("Expected Decl::Stmt(Let), got {:?}", other),
    }
}

#[test]
fn parse_aliased_variant_expr() {
    let program = parse_source(r#"let x = ns.Color::Red { intensity: 1 }"#);
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::VariantExpr(type_name, variant, fields, _) => {
                assert_eq!(type_name, "ns.Color");
                assert_eq!(variant, "Red");
                assert_eq!(fields.len(), 1);
            }
            other => panic!("Expected VariantExpr, got {:?}", other),
        },
        other => panic!("Expected Decl::Stmt(Let), got {:?}", other),
    }
}

#[test]
fn parse_aliased_state_ref() {
    let program = parse_source(r#"let s = ns.Door::Open"#);
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::StateRef(machine, state, _) => {
                assert_eq!(machine, "ns.Door");
                assert_eq!(state, "Open");
            }
            other => panic!("Expected StateRef, got {:?}", other),
        },
        other => panic!("Expected Decl::Stmt(Let), got {:?}", other),
    }
}

#[test]
fn parse_aliased_type_annotation() {
    let program = parse_source(r#"let x: ns.MyType = 1"#);
    assert_eq!(program.declarations.len(), 1);
    match &program.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => {
            assert!(l.type_annotation.is_some());
            match l.type_annotation.as_ref().unwrap() {
                TypeExpr::Named(name) => assert_eq!(name, "ns.MyType"),
                other => panic!("Expected Named type, got {:?}", other),
            }
        }
        other => panic!("Expected Decl::Stmt(Let), got {:?}", other),
    }
}

#[test]
fn parse_fstring_with_format_spec() {
    let prog = parse_source(r#"let s = f"{x:.2f}""#);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    FStringPart::Expr(_, spec) => {
                        assert_eq!(spec.as_deref(), Some(".2f"));
                    }
                    other => panic!("Expected Expr part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_fstring_without_format_spec() {
    let prog = parse_source(r#"let s = f"{x}""#);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    FStringPart::Expr(_, spec) => {
                        assert!(spec.is_none());
                    }
                    other => panic!("Expected Expr part, got {:?}", other),
                }
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn parse_fstring_mixed_parts_with_spec() {
    let prog = parse_source(r#"let s = f"val={x:>10}!""#);
    match &prog.declarations[0] {
        Decl::Stmt(Stmt::Let(l)) => match &l.value {
            Expr::FStringExpr(parts, _) => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(&parts[0], FStringPart::Lit(s) if s == "val="));
                match &parts[1] {
                    FStringPart::Expr(_, spec) => {
                        assert_eq!(spec.as_deref(), Some(">10"));
                    }
                    other => panic!("Expected Expr part, got {:?}", other),
                }
                assert!(matches!(&parts[2], FStringPart::Lit(s) if s == "!"));
            }
            other => panic!("Expected FStringExpr, got {:?}", other),
        },
        other => panic!("Expected Let, got {:?}", other),
    }
}

#[test]
fn anonymous_entity_literal_parses() {
    let prog = parse_source(
        "component Health { hp: int = 0 }\nfn main() -> nil { let x = entity { Health { hp: 50 } } return x }",
    );
    match &prog.declarations[1] {
        Decl::Fn(f) => {
            assert_eq!(f.name, "main");
            match &f.body.stmts[0] {
                Stmt::Let(l) => match &l.value {
                    Expr::EntityLiteral(name, components, _) => {
                        assert!(name.is_none());
                        assert_eq!(components.len(), 1);
                        let ci = components[0].as_init().unwrap();
                        assert_eq!(ci.comp_name, "Health");
                        assert_eq!(ci.fields.len(), 1);
                        assert_eq!(ci.fields[0].0, "hp");
                    }
                    other => panic!("Expected EntityLiteral, got {:?}", other),
                },
                other => panic!("Expected Let, got {:?}", other),
            }
        }
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn anonymous_entity_literal_multiple_components() {
    let prog = parse_source(
        "component A { x: int = 0 }\ncomponent B { y: int = 0 }\nfn main() -> nil { let e = entity { A { x: 1 }, B { y: 2 } } return e }",
    );
    match &prog.declarations[2] {
        Decl::Fn(f) => match &f.body.stmts[0] {
            Stmt::Let(l) => match &l.value {
                Expr::EntityLiteral(name, components, _) => {
                    assert!(name.is_none());
                    assert_eq!(components.len(), 2);
                    assert_eq!(components[0].as_init().unwrap().comp_name, "A");
                    assert_eq!(components[1].as_init().unwrap().comp_name, "B");
                }
                other => panic!("Expected EntityLiteral, got {:?}", other),
            },
            other => panic!("Expected Let, got {:?}", other),
        },
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn named_entity_still_parses_as_declaration() {
    let prog =
        parse_source("component Health { hp: int = 0 }\nentity player { Health { hp: 100 } }");
    match &prog.declarations[1] {
        Decl::Entity(e) => {
            assert_eq!(e.name, "player");
            assert_eq!(e.components.len(), 1);
            assert_eq!(e.components[0].as_init().unwrap().comp_name, "Health");
        }
        other => panic!("Expected Entity declaration, got {:?}", other),
    }
}

#[test]
fn named_entity_literal_with_string() {
    let prog = parse_source(
        "component Health { hp: int = 0 }\nfn main() -> nil { let x = entity \"player\" { Health { hp: 50 } } return x }",
    );
    match &prog.declarations[1] {
        Decl::Fn(f) => {
            assert_eq!(f.name, "main");
            match &f.body.stmts[0] {
                Stmt::Let(l) => match &l.value {
                    Expr::EntityLiteral(name, components, _) => {
                        assert!(name.is_some());
                        match name.as_ref().unwrap().as_ref() {
                            Expr::StrLit(s, _) => assert_eq!(s, "player"),
                            other => panic!("Expected StrLit name, got {:?}", other),
                        }
                        assert_eq!(components.len(), 1);
                        assert_eq!(components[0].as_init().unwrap().comp_name, "Health");
                    }
                    other => panic!("Expected EntityLiteral, got {:?}", other),
                },
                other => panic!("Expected Let, got {:?}", other),
            }
        }
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn named_entity_literal_with_variable() {
    let prog = parse_source(
        "component Health { hp: int = 0 }\nfn make(name: str) -> entity { return entity name { Health { hp: 100 } } }",
    );
    match &prog.declarations[1] {
        Decl::Fn(f) => {
            assert_eq!(f.name, "make");
            match &f.body.stmts[0] {
                Stmt::Return(r) => match r.value.as_ref().unwrap() {
                    Expr::EntityLiteral(name, components, _) => {
                        assert!(name.is_some());
                        match name.as_ref().unwrap().as_ref() {
                            Expr::Ident(n, _) => assert_eq!(n, "name"),
                            other => panic!("Expected Ident name, got {:?}", other),
                        }
                        assert_eq!(components.len(), 1);
                        assert_eq!(components[0].as_init().unwrap().comp_name, "Health");
                    }
                    other => panic!("Expected EntityLiteral, got {:?}", other),
                },
                other => panic!("Expected Return, got {:?}", other),
            }
        }
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn named_entity_literal_variable_empty_body() {
    let prog = parse_source("fn main() -> nil { let name = \"x\"\nlet e = entity name {} }");
    match &prog.declarations[0] {
        Decl::Fn(f) => match &f.body.stmts[1] {
            Stmt::Let(l) => match &l.value {
                Expr::EntityLiteral(name, components, _) => {
                    assert!(name.is_some());
                    match name.as_ref().unwrap().as_ref() {
                        Expr::Ident(n, _) => assert_eq!(n, "name"),
                        other => panic!("Expected Ident name, got {:?}", other),
                    }
                    assert_eq!(components.len(), 0);
                }
                other => panic!("Expected EntityLiteral, got {:?}", other),
            },
            other => panic!("Expected Let, got {:?}", other),
        },
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn entity_literal_with_variable_component_entry() {
    let prog = parse_source(
        "component Pos { x: int = 0 }\nfn main() -> nil { let p = Pos { x: 1 }\nlet e = entity { p } }",
    );
    match &prog.declarations[1] {
        Decl::Fn(f) => match &f.body.stmts[1] {
            Stmt::Let(l) => match &l.value {
                Expr::EntityLiteral(name, components, _) => {
                    assert!(name.is_none());
                    assert_eq!(components.len(), 1);
                    match components[0].as_expr() {
                        Some(Expr::Ident(n, _)) => assert_eq!(n, "p"),
                        other => panic!("Expected Ident expr entry, got {:?}", other),
                    }
                }
                other => panic!("Expected EntityLiteral, got {:?}", other),
            },
            other => panic!("Expected Let, got {:?}", other),
        },
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn entity_literal_with_call_component_entry() {
    let prog = parse_source(
        "component Pos { x: int = 0 }\nfn make_pos() -> Pos { return Pos { x: 5 } }\nfn main() -> nil { let e = entity { make_pos() } }",
    );
    match &prog.declarations[2] {
        Decl::Fn(f) => match &f.body.stmts[0] {
            Stmt::Let(l) => match &l.value {
                Expr::EntityLiteral(name, components, _) => {
                    assert!(name.is_none());
                    assert_eq!(components.len(), 1);
                    assert!(components[0].as_expr().is_some());
                    match components[0].as_expr().unwrap() {
                        Expr::Call(callee, args, _) => {
                            match callee.as_ref() {
                                Expr::Ident(n, _) => assert_eq!(n, "make_pos"),
                                other => panic!("Expected Ident callee, got {:?}", other),
                            }
                            assert_eq!(args.len(), 0);
                        }
                        other => panic!("Expected Call expr entry, got {:?}", other),
                    }
                }
                other => panic!("Expected EntityLiteral, got {:?}", other),
            },
            other => panic!("Expected Let, got {:?}", other),
        },
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn entity_literal_mixed_init_and_expr_entries() {
    let prog = parse_source(
        "component A { x: int = 0 }\ncomponent B { y: int = 0 }\nfn main() -> nil { let b = B { y: 2 }\nlet e = entity { A { x: 1 }, b } }",
    );
    match &prog.declarations[2] {
        Decl::Fn(f) => match &f.body.stmts[1] {
            Stmt::Let(l) => match &l.value {
                Expr::EntityLiteral(name, components, _) => {
                    assert!(name.is_none());
                    assert_eq!(components.len(), 2);
                    assert_eq!(components[0].as_init().unwrap().comp_name, "A");
                    match components[1].as_expr() {
                        Some(Expr::Ident(n, _)) => assert_eq!(n, "b"),
                        other => panic!("Expected Ident expr entry, got {:?}", other),
                    }
                }
                other => panic!("Expected EntityLiteral, got {:?}", other),
            },
            other => panic!("Expected Let, got {:?}", other),
        },
        other => panic!("Expected Fn, got {:?}", other),
    }
}

#[test]
fn recursive_variant_field_with_bare_type_parses() {
    // The canonical recursive-field spelling (dogfood feature seq 56):
    // `left: Expr` — the bare type name in the default slot — must parse
    // cleanly for a self-referential sum type.
    let mut lexer =
        Lexer::new("type Expr {\n Num { value: 0 }\n Add { left: Expr, right: Expr }\n}");
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(parser.errors().is_empty(), "got: {:?}", parser.errors());
    match &program.declarations[0] {
        Decl::Type(t) => {
            assert_eq!(t.name, "Expr");
            assert_eq!(t.variants.len(), 2);
            assert_eq!(t.variants[1].name, "Add");
            assert_eq!(t.variants[1].fields.len(), 2);
        }
        other => panic!("Expected Type decl, got {:?}", other),
    }
}

#[test]
fn variant_field_type_eq_default_gives_targeted_diagnostic() {
    // spec 2.4 promises a targeted diagnostic for the component/struct form
    // `field: Type = default` written inside a variant, instead of the bare
    // "Expected identifier, got Assign" the parser used to emit. It must
    // name the field and point at the working `field: Type` spelling.
    let err = parse_source_err("type Expr {\n Add { left: Expr = nil }\n}");
    assert!(
        err.message.contains("variant field 'left'")
            && err.message.contains("`left: Expr`")
            && err.message.contains("recursive or self-referential"),
        "got: {}",
        err.message
    );
}

#[test]
fn variant_field_primitive_type_eq_default_also_diagnosed() {
    // The spec's own example: `radius: float = 0.0` inside a variant.
    let err = parse_source_err("type Shape {\n Circle { radius: float = 0.0 }\n}");
    assert!(
        err.message.contains("variant field 'radius'")
            && err.message.contains("component/struct/resource"),
        "got: {}",
        err.message
    );
}
