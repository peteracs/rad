

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

    /// Laws, resolvers, and constraints are executable causal roots rather
    /// than ordinary functions. Helpers used only from those bodies are live.
    #[test]
    fn causal_declarations_seed_helper_reachability() {
        let warnings = check_src_warnings(
            r#"
            component Score { value: 0 }
            intent AddScore { key target: entity  amount: int }

            pure fn law_helper(value: int) -> int { return value + 1 }
            pure fn resolver_helper(value: int) -> int { return value * 2 }
            pure fn constraint_helper(value: int) -> bool { return value >= 0 }

            law Add(target: entity) {
                propose AddScore { target: target, amount: law_helper(2) }
            }

            resolver ResolveScore for AddScore(target, proposals) {
                next(target, Score { value: resolver_helper(proposals[0].amount) })
            }

            constraint ValidScore for Score(subject, proposed) {
                require constraint_helper(proposed.value) else "score.invalid"
            }

            entity score { Score {} }
            fn main() -> nil {
                settle { Add(score) }
            }
            "#,
        );
        let unused = warnings
            .iter()
            .filter(|warning| warning.message.contains("Unused function"))
            .collect::<Vec<_>>();
        assert!(
            unused.is_empty(),
            "causal helper calls must be reachable, got: {:?}",
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
            errors.iter().any(|error| error
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
