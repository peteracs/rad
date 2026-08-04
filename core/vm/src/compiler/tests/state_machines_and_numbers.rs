

    #[test]
    fn compile_state_machine() {
        let output = run_source(
            r#"
            state Door {
                Locked {
                    on unlock -> Closed
                }
                Closed {
                    on open -> Open
                }
                Open {
                    on close -> Closed
                }
            }
            let mut d = Door::Locked
            d = transition(d, "unlock") |> unwrap
            print(d)
            d = transition(d, "open") |> unwrap
            print(d)
        "#,
        );
        assert_eq!(output, vec!["Door::Closed", "Door::Open"]);
    }

    #[test]
    fn compile_match_state() {
        let output = run_source(
            r#"
            state Light {
                On {
                    on toggle -> Off
                }
                Off {
                    on toggle -> On
                }
            }
            let l = Light::On
            match l {
                On => {
                    print("light is on")
                }
                Off => {
                    print("light is off")
                }
            }
        "#,
        );
        assert_eq!(output, vec!["light is on"]);
    }

    #[test]
    fn compile_match_variant() {
        let output = run_source(
            r#"
            type Shape {
                Circle { radius: 0.0 }
                Rect { w: 0.0, h: 0.0 }
            }
            let s = Shape::Circle { radius: 5.0 }
            match s {
                Circle => {
                    print("circle")
                }
                Rect => {
                    print("rect")
                }
            }
        "#,
        );
        assert_eq!(output, vec!["circle"]);
    }

    #[test]
    fn compile_match_expression_in_let_binding() {
        let output = run_source(
            r#"
            type Shape {
                Circle { radius: 0.0 }
                Rect { w: 0.0, h: 0.0 }
            }
            let s = Shape::Rect { w: 2.0, h: 3.0 }
            let label = match s {
                Circle => { "circle" }
                Rect => { "rect" }
            }
            print(label)
        "#,
        );
        assert_eq!(output, vec!["rect"]);
    }

    #[test]
    fn compile_plain_string_dollar_interpolation() {
        let output = run_source(
            r#"
            let city = "Neo Arcadia"
            let pop = 1200
            print("city=${city}, pop=${pop}")
        "#,
        );
        assert_eq!(output, vec!["city=Neo Arcadia, pop=1200"]);
    }

    #[test]
    fn compile_match_variant_bindings_and_locals() {
        let output = run_source(
            r#"
            type Payload {
                Data { first: 0, second: 0 }
            }
            let p = Payload::Data { first: 10, second: 20 }
            match p {
                Data { first, second } => {
                    print(first)
                    print(second)
                    let total = first + second
                    print(total)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["10", "20", "30"]);
    }

    #[test]
    fn compile_match_nested_destructuring_and_guard() {
        let output = run_source_result_with_compat(
            r#"
            type Meta {
                Info { code: 0 }
            }
            type Ev {
                Alarm { meta: Meta::Info { code: 9 }, level: 0 }
            }
            let ev = Ev::Alarm { meta: Meta::Info { code: 9 }, level: 3 }
            match ev {
                Alarm { meta: { code }, level: sev } when sev > 2 => {
                    print(code)
                }
                Alarm { .. } => {
                    print("fallback")
                }
            }
        "#,
        )
        .expect("expected program to run successfully");
        assert_eq!(output, vec!["9"]);
    }

    #[test]
    fn compile_unwrap_option() {
        let output = run_source(
            r#"
            component Score {
                value: 0
            }
            entity p {
                Score { value: 42 }
            }
            let s = get(p, Score) |> unwrap
            print(s.value)
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn compile_for_loop_global_scope() {
        let output = run_source(
            r#"
            for x in [1, 2, 3] {
                print(x)
            }
        "#,
        );
        assert_eq!(output, vec!["1", "2", "3"]);
    }

    #[test]
    fn compile_state_machine_transition_chain() {
        let output = run_source(
            r#"
            state Door {
                Locked {
                    on unlock -> Closed
                }
                Closed {
                    on open -> Open
                }
                Open {
                    on close -> Closed
                }
            }
            let mut d = Door::Locked
            d = transition(d, "unlock") |> unwrap
            d = transition(d, "open") |> unwrap
            d = transition(d, "close") |> unwrap
            print(d)
        "#,
        );
        assert_eq!(output, vec!["Door::Closed"]);
    }

    #[test]
    fn compile_and_or_logic() {
        let output = run_source(
            r#"
            print(true and false)
            print(true or false)
            print(not true)
        "#,
        );
        assert_eq!(output, vec!["false", "true", "false"]);
    }

    #[test]
    fn compile_nested_break() {
        let output = run_source(
            r#"
            for x in [1, 2, 3] {
                if x == 2 {
                    let y = 99
                    break
                }
                print(x)
            }
            print("done")
        "#,
        );
        assert_eq!(output, vec!["1", "done"]);
    }

    fn compile_err(src: &str) -> String {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();
        let compiler = Compiler::new();
        compiler.compile(&program).unwrap_err().message
    }

    #[test]
    fn immutable_local_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn main() -> nil {
                let x = 1
                x = 2
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn mutable_local_reassign_allowed() {
        let output = run_source(
            r#"
            fn test() {
                let mut x = 1
                x = 2
                print(x)
            }
            test()
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn immutable_global_reassign_rejected() {
        let msg = compile_err("let x = 1\nx = 2");
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn mutable_global_reassign_allowed() {
        let output = run_source("let mut x = 1\nx = 2\nprint(x)");
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn fn_param_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn f(a) {
                a = 10
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'a'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_index_assign_rejected() {
        let msg = compile_err(
            r#"
            fn main() -> nil {
                let xs = [1, 2, 3]
                xs[0] = 9
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'xs'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_field_assign_rejected() {
        let msg = compile_err(
            r#"
            component Health { hp: 0 }
            entity p { Health { hp: 1 } }
            fn main() -> nil {
                let h = get(p, Health) |> unwrap
                h.hp = 9
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'h'"),
            "got: {msg}"
        );
    }

    #[test]
    fn for_var_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn test() {
                for x in [1, 2, 3] {
                    x = 99
                }
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn compile_for_loop_map_iterates_keys() {
        let output = run_source(
            r#"
            let m = {"a": 1, "b": 2, "c": 3}
            for k in m {
                print(k)
            }
        "#,
        );
        assert_eq!(output, vec!["a", "b", "c"]);
    }

    #[test]
    fn compile_recursion_reports_stack_overflow() {
        let err = run_source_result(
            r#"
            fn recurse(n) {
                return recurse(n + 1)
            }
            print(recurse(0))
        "#,
        )
        .expect_err("expected recursion to fail");
        assert!(err.contains("Stack overflow"), "unexpected error: {}", err);
    }

    #[test]
    fn compile_and_or_are_boolean() {
        let output = run_source(
            r#"
            let x = true and 42
            print(x)
            print(typeof(x))
            let y = false or 7
            print(y)
            print(typeof(y))
        "#,
        );
        assert_eq!(output, vec!["true", "bool", "true", "bool"]);
    }

    #[test]
    fn compile_emit_preserves_registration_order() {
        let output = run_source(
            r#"
            event Hit { target }
            on Hit(e) { print("A") }
            on Hit(e) { print("B") }
            on Hit(e) { print("C") }
            fn main() -> nil {
                emit Hit { target: 1 }
                flush_events()
            }
        "#,
        );
        assert_eq!(output, vec!["A", "B", "C"]);
    }

    #[test]
    fn compile_fstring_in_component_default_and_event_payload() {
        let output = run_source(
            r#"
            component Audit {
                msg: f"code {7}"
            }
            event Alert { message }
            on Alert(e) {
                print(e.message)
            }
            fn main() -> nil {
                let a = Audit {}
                emit Alert { message: f"got {a.msg}" }
                flush_events()
            }
        "#,
        );
        assert_eq!(output, vec!["got code 7"]);
    }

    #[test]
    fn compile_numeric_cross_type_equality() {
        let output = run_source(
            r#"
            print(1 == 1.0)
            print(1 != 1.0)
        "#,
        );
        assert_eq!(output, vec!["true", "false"]);
    }

    #[test]
    fn compile_match_fallthrough_keeps_subject_for_bindings() {
        let output = run_source(
            r#"
            type Payload {
                Other { n: 0 }
                Data { first: 0, second: 0 }
            }
            let p = Payload::Data { first: 10, second: 20 }
            match p {
                Other { n } => {
                    print(n)
                }
                Data { first, second } => {
                    print(first)
                    print(second)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["10", "20"]);
    }

    #[test]
    fn immutable_function_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn f() { return 1 }
            f = 42
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'f'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_component_reassign_rejected() {
        let msg = compile_err(
            r#"
            component Health { hp: 100 }
            Health = 42
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'Health'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_entity_reassign_rejected() {
        let msg = compile_err(
            r#"
            component Health { hp: 100 }
            entity player { Health { hp: 50 } }
            player = 42
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'player'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_captured_variable_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn f() {
                let x = 0
                let g = fn() {
                    x = 1
                }
                g()
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_global() {
        let output = run_source(
            r#"
            let x = 10
            fn outer() {
                let mut x = 5
                let inner = fn() {
                    x = 99
                    return x
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_fn_global() {
        let output = run_source(
            r#"
            fn x() { return 0 }
            fn outer() {
                let mut x = 5
                let inner = fn() {
                    x = 42
                    return x
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_entity_global() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity player { Health { hp: 50 } }
            fn outer() {
                let mut player = "shadow"
                let inner = fn() {
                    player = "reassigned"
                    return player
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["reassigned"]);
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_component_global() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            fn outer() {
                let mut Health = "shadow"
                let inner = fn() {
                    Health = "ok"
                    return Health
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["ok"]);
    }

    #[test]
    fn immutable_upvalue_shadows_mutable_global() {
        let msg = compile_err(
            r#"
            let mut x = 10
            fn outer() {
                let x = 5
                let inner = fn() {
                    x = 99
                }
                inner()
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn deeply_nested_mutable_upvalue_shadows_immutable_global() {
        let output = run_source(
            r#"
            let x = 0
            fn a() {
                let mut x = 1
                let b = fn() {
                    let c = fn() {
                        x = 777
                        return x
                    }
                    return c()
                }
                return b()
            }
            print(a())
        "#,
        );
        assert_eq!(output, vec!["777"]);
    }

    #[test]
    fn triple_nested_immutable_upvalue_rejected() {
        let msg = compile_err(
            r#"
            fn a() {
                let x = 1
                let b = fn() {
                    let c = fn() {
                        x = 2
                    }
                    c()
                }
                b()
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn field_assign_on_mutable_upvalue_shadowing_immutable_global() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity player { Health { hp: 50 } }
            fn outer() {
                let mut h = get(player, Health) |> unwrap
                let inner = fn() {
                    h.hp = 999
                }
                inner()
                return h.hp
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["999"]);
    }

    #[test]
    fn index_assign_on_mutable_upvalue_shadowing_immutable_global() {
        let output = run_source(
            r#"
            let xs = [0]
            fn outer() {
                let mut xs = [10, 20, 30]
                let inner = fn() {
                    xs[1] = 99
                }
                inner()
                return xs[1]
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn no_upvalue_no_global_passes_mutability_check() {
        let output = run_source(
            r#"
            let mut x = 1
            x = 2
            print(x)
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn upvalue_mutable_global_absent() {
        let output = run_source(
            r#"
            fn outer() {
                let mut z = 0
                let inner = fn() {
                    z = 42
                    return z
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn mutable_upvalue_two_closures_independent() {
        let output = run_source(
            r#"
            let x = 0
            fn make() {
                let mut x = 10
                let inc = fn() {
                    x = x + 1
                    return x
                }
                let dec = fn() {
                    x = x - 1
                    return x
                }
                return [inc(), dec()]
            }
            print(make())
        "#,
        );
        assert_eq!(output, vec!["[11, 10]"]);
    }

    #[test]
    fn immutable_global_no_upvalue_still_rejected() {
        let msg = compile_err(
            r#"
            let x = 10
            fn f() {
                x = 20
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_fn_global_no_upvalue_still_rejected() {
        let msg = compile_err(
            r#"
            fn greet() { return "hi" }
            fn f() {
                greet = 42
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'greet'"),
            "got: {msg}"
        );
    }

    #[test]
    fn constant_folding_arithmetic() {
        let output = run_source("print(2 + 3 * 4)");
        assert_eq!(output, vec!["14"]);
    }

    #[test]
    fn constant_folding_string_concat() {
        let output = run_source(r#"print("hello" + " " + "world")"#);
        assert_eq!(output, vec!["hello world"]);
    }

    #[test]
    fn constant_folding_comparison() {
        let output = run_source("print(10 > 5)\nprint(3 == 3)");
        assert_eq!(output, vec!["true", "true"]);
    }

    #[test]
    fn constant_folding_negation() {
        let output = run_source("print(-42)\nprint(not true)");
        assert_eq!(output, vec!["-42", "false"]);
    }

    #[test]
    fn dead_code_after_return_is_eliminated() {
        let output = run_source(
            r#"
            fn f() {
                return 1
                print("unreachable")
                return 2
            }
            print(f())
        "#,
        );
        assert_eq!(output, vec!["1"]);
    }

    #[test]
    fn dead_code_after_break_is_eliminated() {
        let output = run_source(
            r#"
            for x in [1, 2, 3] {
                if x == 2 {
                    break
                    print("unreachable")
                }
                print(x)
            }
        "#,
        );
        assert_eq!(output, vec!["1"]);
    }

    // === Integration stress tests for reconstructed lexer/parser ===

    #[test]
    fn compile_string_escape_sequences_run_correctly() {
        let output = run_source(r#"print("hello\tworld\n!")"#);
        assert_eq!(output, vec!["hello\tworld\n!"]);
    }

    #[test]
    fn compile_fstring_interpolation_runs_correctly() {
        let output = run_source(
            r#"
            let name = "rad"
            print(f"hello {name}!")
        "#,
        );
        assert_eq!(output, vec!["hello rad!"]);
    }

    #[test]
    fn compile_triple_fstring_runs_correctly() {
        let output = run_source(
            r#"
            let n = 3
            let code = f"""
if (x) { return ${n}; }
"""
            print(code)
        "#,
        );
        assert_eq!(output, vec!["\nif (x) { return 3; }\n"]);
    }

    #[test]
    fn compile_code_with_comments_runs_correctly() {
        let output = run_source(
            r#"
            // This is a line comment
            let x = 1
            /* This is a block comment */
            let y = 2
            /* Nested /* comments */ work */
            let z = x + y
            print(z)
        "#,
        );
        assert_eq!(output, vec!["3"]);
    }

    #[test]
    fn compile_range_with_dotdot() {
        let output = run_source(
            r#"
            for i in range(1, 4) {
                print(i)
            }
        "#,
        );
        assert_eq!(output, vec!["1", "2", "3"]);
    }

    #[test]
    fn compile_float_operations() {
        let output = run_source(
            r#"
            let a = 0.5
            let b = .5
            let c = 5.
            let d = 1.5e2
            print(a == b)
            print(c)
            print(d)
        "#,
        );
        assert_eq!(output, vec!["true", "5.0", "150.0"]);
    }