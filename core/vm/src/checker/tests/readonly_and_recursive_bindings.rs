

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