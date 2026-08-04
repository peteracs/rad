

    #[test]
    fn compile_method_call_on_int_literal() {
        let output = run_source(
            r#"
            let x = [1, 2, 3]
            print(len(x))
        "#,
        );
        assert_eq!(output, vec!["3"]);
    }

    #[test]
    fn compile_empty_string() {
        let output = run_source(
            r#"
            let s = ""
            print(len(s))
        "#,
        );
        assert_eq!(output, vec!["0"]);
    }

    #[test]
    fn compile_question_mark_operator_on_result_ok() {
        let output = run_source(
            r#"
            type Result {
                Ok { value: int }
                Err { message: str }
            }
            fn try_get() -> Result {
                return Result::Ok { value: 42 }
            }
            fn use_try() -> Result {
                let v = try_get()?
                return Result::Ok { value: v }
            }
            let r = use_try()
            match r {
                Ok { value } => {
                    print(value)
                }
                Err { message } => {
                    print(message)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn compile_question_mark_operator_on_result_err() {
        let output = run_source(
            r#"
            type Result {
                Ok { value: int }
                Err { message: str }
            }
            fn try_get() -> Result {
                return Result::Err { message: "failed" }
            }
            fn use_try() -> Result {
                let v = try_get()?
                return Result::Ok { value: v }
            }
            let r = use_try()
            match r {
                Ok { value } => {
                    print(value)
                }
                Err { message } => {
                    print(message)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["failed"]);
    }

    #[test]
    fn compile_struct_basic_creation_and_field_access() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            let p = Point { x: 3.0, y: 4.0 }
            print(p.x)
            print(p.y)
        "#,
        );
        assert_eq!(output, vec!["3.0", "4.0"]);
    }

    #[test]
    fn compile_struct_default_fields() {
        let output = run_source(
            r#"
            struct Config { debug: false, level: 1 }
            let c = Config {}
            print(c.debug)
            print(c.level)
        "#,
        );
        assert_eq!(output, vec!["false", "1"]);
    }

    #[test]
    fn compile_struct_spread_syntax() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            fn main() -> nil {
                let p1 = Point { x: 1.0, y: 2.0 }
                let p2 = Point { x: 10.0, ..p1 }
                print(p2.x)
                print(p2.y)
            }
        "#,
        );
        assert_eq!(output, vec!["10.0", "2.0"]);
    }

    #[test]
    fn compile_struct_field_mutation() {
        let output = run_source(
            r#"
            struct Counter { count: 0 }
            let mut c = Counter { count: 5 }
            c.count = c.count + 1
            print(c.count)
        "#,
        );
        assert_eq!(output, vec!["6"]);
    }

    #[test]
    fn compile_struct_pass_to_function() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            fn magnitude(p) {
                return p.x + p.y
            }
            let p = Point { x: 3.0, y: 4.0 }
            print(magnitude(p))
        "#,
        );
        assert_eq!(output, vec!["7.0"]);
    }

    #[test]
    fn compile_struct_return_from_function() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            fn make_point(a, b) {
                return Point { x: a, y: b }
            }
            let p = make_point(5.0, 6.0)
            print(p.x)
            print(p.y)
        "#,
        );
        assert_eq!(output, vec!["5.0", "6.0"]);
    }

    #[test]
    fn compile_struct_coexists_with_component() {
        let output = run_source(
            r#"
            component Position { x: 0.0, y: 0.0 }
            struct Vec2 { x: 0.0, y: 0.0 }
            let v = Vec2 { x: 1.0, y: 2.0 }
            print(v.x)
            print(v.y)
        "#,
        );
        assert_eq!(output, vec!["1.0", "2.0"]);
    }

    #[test]
    fn compile_is_none_false_for_non_option() {
        let output = run_source(
            r#"
            print(is_none(42))
            print(is_none("hello"))
            print(is_none([1, 2]))
            type Option { Some { value: 0 } None { } }
            print(is_none(Option::None {}))
            print(is_none(Option::Some { value: 5 }))
        "#,
        );
        assert_eq!(output, vec!["false", "false", "false", "true", "false"]);
    }

    #[test]
    fn compile_now_unix_s_returns_positive_int() {
        let output = run_source(
            r#"
            let t = now_unix_s()
            print(typeof(t))
            print(t > 0)
        "#,
        );
        assert_eq!(output, vec!["int", "true"]);
    }

    #[test]
    fn compile_now_unix_ms_returns_positive_int() {
        let output = run_source(
            r#"
            let t = now_unix_ms()
            print(typeof(t))
            print(t > 0)
        "#,
        );
        assert_eq!(output, vec!["int", "true"]);
    }

    #[test]
    fn compile_read_file_write_file_roundtrip() {
        let output = run_source(
            r#"
            write_file("__test_roundtrip.txt", "hello rad")
            let content = read_file("__test_roundtrip.txt")
            print(content)
            remove_file("__test_roundtrip.txt")
        "#,
        );
        assert_eq!(output, vec!["hello rad"]);
    }

    #[test]
    fn compile_file_exists_and_remove() {
        let output = run_source(
            r#"
            write_file("__test_exists.txt", "x")
            print(file_exists("__test_exists.txt"))
            remove_file("__test_exists.txt")
            print(file_exists("__test_exists.txt"))
        "#,
        );
        assert_eq!(output, vec!["true", "false"]);
    }

    #[test]
    fn compile_typeof_for_all_primitives() {
        let output = run_source(
            r#"
            print(typeof(42))
            print(typeof(3.14))
            print(typeof("hi"))
            print(typeof(true))
            print(typeof(nil))
            print(typeof([1, 2]))
            print(typeof({"a": 1}))
        "#,
        );
        assert_eq!(
            output,
            vec!["int", "float", "str", "bool", "nil", "list", "map"]
        );
    }

    #[test]
    fn compile_basic_query_no_filter() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity unitA { Health { hp: 80 } }
            entity unitB { Health { hp: 20 } }
            fn main() -> nil {
                let all = query { Health }
                print(len(all))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_filter_actually_filters() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity unitA { Health { hp: 80 } }
            entity unitB { Health { hp: 20 } }
            entity unitC { Health { hp: 60 } }
            fn main() -> nil {
                let strong = query { Health } where Health.hp > 50
                print(len(strong))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_filter_multi_component() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            entity unitC {
                Health { hp: 60 },
                Armor { def: 10 }
            }
            fn main() -> nil {
                let tanky = query { Health, Armor } where Armor.def >= 5
                print(len(tanky))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_readonly_unpack() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                for id, h, a in query { Health, Armor } {
                    print(h.hp + a.def)
                }
            }
        "#,
        );
        // unitA: 80 + 5 = 85
        // unitB: 20 + 2 = 22
        assert_eq!(output, vec!["85", "22"]);
    }

    /// `EcsHas` skip jumps + `continue`: inner scopes must not run on skip; `continue` must pop
    /// entity + component locals and land after both `end_scope`s (see `compile_for_query_unpack`).
    #[test]
    fn compile_query_unpack_continue_skips_and_loop_depth() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                for id, h, a in query { mut Health, mut Armor } {
                    continue
                }
                print("done")
            }
        "#,
        );
        assert_eq!(output, vec!["done"]);
    }

    /// Read-only multi-binding unpack also uses `skip_jumps`; `continue` must still balance scopes.
    #[test]
    fn compile_query_unpack_readonly_continue_two_components() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            fn main() -> nil {
                for id, h, a in query { Health, Armor } {
                    continue
                }
                print("done")
            }
        "#,
        );
        assert_eq!(output, vec!["done"]);
    }

    #[test]
    fn compile_query_unpack_break_mut_two_components() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                for id, h, a in query { mut Health, mut Armor } {
                    print("once")
                    break
                }
                print("after")
            }
        "#,
        );
        assert_eq!(output, vec!["once", "after"]);
    }

    /// Removing a component mid-loop forces later iterations through the `JumpIfFalse` skip path;
    /// `continue` after `remove` must still leave the VM stack consistent across skip vs body.
    #[test]
    fn compile_query_unpack_remove_then_skip_with_continue() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 10 }
            fn main() -> nil {
                let e1 = spawn()
                set(e1, Health { hp: 100 })
                set(e1, Armor { def: 10 })
                let e2 = spawn()
                set(e2, Health { hp: 80 })
                set(e2, Armor { def: 5 })
                for id, h, a in query { mut Health, mut Armor } {
                    if id == e1 {
                        remove(e2, Armor)
                    }
                    continue
                }
                print("done")
            }
        "#,
        );
        assert_eq!(output, vec!["done"]);
    }

    #[test]
    fn compile_query_unpack_remove_then_break() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 10 }
            fn main() -> nil {
                let e1 = spawn()
                set(e1, Health { hp: 100 })
                set(e1, Armor { def: 10 })
                let e2 = spawn()
                set(e2, Health { hp: 80 })
                set(e2, Armor { def: 5 })
                for id, h, a in query { mut Health, mut Armor } {
                    if id == e1 {
                        remove(e2, Armor)
                        h.hp = 999
                        break
                    }
                }
                for id, h in query { Health } {
                    print(h.hp)
                }
            }
        "#,
        );
        // e1 mutated to 999, e2 remains 80.
        assert_eq!(output, vec!["999", "80"]);
    }

    #[test]
    fn compile_query_unpack_return_from_main() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            fn main() -> nil {
                for id, h, a in query { mut Health, mut Armor } {
                    if h.hp == 80 {
                        return
                    }
                }
                print("after")
            }
        "#,
        );
        assert_eq!(output, Vec::<String>::new());
    }

    /// Exercises `return` from inside a nested function's unpack loop, ensuring writebacks
    /// are correctly emitted by `compile_return` even when earlier entities hit the `EcsHas` skip path.
    #[test]
    fn compile_query_unpack_return_writeback_in_nested_fn() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }

            fn process() {
                for id, h, a in query { mut Health, mut Armor } {
                    if h.hp == 80 {
                        h.hp = 999
                        return
                    }
                }
            }

            fn main() -> nil {
                let e1 = spawn()
                set(e1, Health { hp: 50 }) // No Armor, triggers EcsHas skip path in process()

                let e2 = spawn()
                set(e2, Health { hp: 80 })
                set(e2, Armor { def: 5 })  // Matches, mutates, and triggers return

                process()

                for id, h in query { Health } {
                    print(h.hp)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["50", "999"]);
    }

    #[test]
    fn compile_tuple_literal_basic() {
        let output = run_source(
            r#"
            let t = (1, 2, 3)
            print(t)
            print(typeof(t))
            print(len(t))
        "#,
        );
        assert_eq!(output, vec!["(1, 2, 3)", "tuple", "3"]);
    }

    #[test]
    fn compile_let_tuple_destructure() {
        let output = run_source(
            r#"
            fn pair() { return (10, 20) }
            let (a, b) = pair()
            print(a)
            print(b)
            let (x, y) = [1, 2]
            print(x + y)
        "#,
        );
        assert_eq!(output, vec!["10", "20", "3"]);
    }

    #[test]
    fn compile_tuple_indexing() {
        let output = run_source(
            r#"
            let t = ("hello", 42, true)
            print(t[0])
            print(t[1])
            print(t[2])
        "#,
        );
        assert_eq!(output, vec!["hello", "42", "true"]);
    }

    #[test]
    fn compile_tuple_equality() {
        let output = run_source(
            r#"
            let a = (1, 2)
            let b = (1, 2)
            let c = (1, 3)
            print(a == b)
            print(a == c)
        "#,
        );
        assert_eq!(output, vec!["true", "false"]);
    }

    #[test]
    fn compile_tuple_single_element() {
        let output = run_source(
            r#"
            let t = (42,)
            print(t)
            print(typeof(t))
        "#,
        );
        assert_eq!(output, vec!["(42,)", "tuple"]);
    }

    #[test]
    fn compile_tuple_empty() {
        let output = run_source(
            r#"
            let t = ()
            print(typeof(t))
            print(len(t))
        "#,
        );
        assert_eq!(output, vec!["tuple", "0"]);
    }

    #[test]
    fn compile_tuple_nested() {
        let output = run_source(
            r#"
            let t = (1, (2, 3))
            print(t[0])
            print(t[1])
        "#,
        );
        assert_eq!(output, vec!["1", "(2, 3)"]);
    }

    #[test]
    fn compile_query_select_single_component() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity warrior {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity mage {
                Health { hp: 50 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                let healths = query { Health, Armor } select Health
                print(len(healths))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_select_multi_component() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity warrior {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            fn main() -> nil {
                let result = query { Health, Armor } select Health, Armor
                print(len(result))
                let pair = result[0]
                print(typeof(pair))
            }
        "#,
        );
        assert_eq!(output, vec!["1", "tuple"]);
    }

    #[test]
    fn compile_query_select_with_filter() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity a { Health { hp: 80 } }
            entity b { Health { hp: 20 } }
            entity c { Health { hp: 60 } }
            fn main() -> nil {
                let strong = query { Health } select Health where Health.hp > 50
                print(len(strong))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_string_builtins_comprehensive() {
        let output = run_source(
            r#"
            print(to_upper("hello"))
            print(to_lower("WORLD"))
            print(trim("  hi  "))
            print(starts_with("abc", "ab"))
            print(ends_with("abc", "bc"))
            print(contains("hello world", "world"))
            print(replace("aXbXc", "X", "_"))
            print(split("a,b,c", ","))
        "#,
        );
        assert_eq!(
            output,
            vec![
                "HELLO",
                "world",
                "hi",
                "true",
                "true",
                "true",
                "a_b_c",
                "[\"a\", \"b\", \"c\"]"
            ]
        );
    }

    fn compile_with_warnings(src: &str) -> (Vec<String>, Vec<CompileWarning>) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();
        let compiler = Compiler::new();
        let result = compiler.compile(&program).unwrap();
        let warnings = result.warnings;
        let mut vm = VM::new();
        vm.load_compile_result(CompileResult {
            chunks: result.chunks,
            systems: result.systems,
            handlers: result.handlers,
            migrations: result.migrations,
            state_machines: result.state_machines,
            intents: result.intents,
            resolvers: result.resolvers,
            constraints: result.constraints,
            layout_analysis: crate::compiler::layout_analysis::LayoutAnalysis::default(),
            materialization_plan: crate::compiler::materialization::MaterializationPlan::default(),
            component_layouts: result.component_layouts,
            component_field_types: result.component_field_types,
            indexed_component_fields: result.indexed_component_fields,
            transient_resources: result.transient_resources,
            component_versions: result.component_versions,
            variant_layouts: result.variant_layouts,
            global_names: result.global_names,
            program_source_identity: result.program_source_identity,
            warnings: Vec::new(),
            gc: result.gc,
        });
        vm.run(0).expect("program should run");
        (vm.print_buffer.clone(), warnings)
    }

    #[test]
    fn vectorized_pipeline_map_mul() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 4]
            let result = xs |> map(fn(x) { x * 2 }) |> map(fn(x) { x + 1 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[3, 5, 7, 9]"]);
    }

    #[test]
    fn vectorized_pipeline_map_and_filter() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 4, 5, 6]
            let result = xs |> map(fn(x) { x * 2 }) |> filter(fn(x) { x > 6 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[8, 10, 12]"]);
    }

    #[test]
    fn vectorized_pipeline_filter_and_map() {
        let output = run_source(
            r#"
            let xs = [10, 20, 30, 40, 50]
            let result = xs |> filter(fn(x) { x > 20 }) |> map(fn(x) { x - 5 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[25, 35, 45]"]);
    }

    #[test]
    fn vectorized_pipeline_arithmetic() {
        let output = run_source(
            r#"
            let xs = [10, 20, 30]
            let result = xs |> map(fn(x) { x / 2 + 1 }) |> map(fn(x) { x % 3 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[0, 2, 1]"]);
    }

    #[test]
    fn vectorized_pipeline_negation() {
        let output = run_source(
            r#"
            let xs = [1, -2, 3]
            let result = xs |> map(fn(x) { -x }) |> map(fn(x) { x * 2 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[-2, 4, -6]"]);
    }

    #[test]
    fn vectorized_pipeline_comparison_filter() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 4, 5]
            let result = xs |> filter(fn(x) { x >= 2 }) |> filter(fn(x) { x <= 4 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[2, 3, 4]"]);
    }

    #[test]
    fn vectorized_pipeline_empty_list() {
        let output = run_source(
            r#"
            let xs: list = []
            let result = xs |> map(fn(x) { x * 2 }) |> filter(fn(x) { x > 0 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[]"]);
    }

    #[test]
    fn vectorized_pipeline_with_captured_variable() {
        let output = run_source(
            r#"
            let factor = 10
            let xs = [1, 2, 3]
            let result = xs |> map(fn(x) { x * factor }) |> filter(fn(x) { x > 15 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[20, 30]"]);
    }

    #[test]
    fn vectorized_pipeline_float_arithmetic() {
        let output = run_source(
            r#"
            let xs = [1.0, 2.0, 3.0]
            let result = xs |> map(fn(x) { x * 2.5 }) |> map(fn(x) { x + 0.5 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[3.0, 5.5, 8.0]"]);
    }

    #[test]
    fn vectorized_pipeline_equality_filter() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 2, 1]
            let result = xs |> filter(fn(x) { x == 2 }) |> map(fn(x) { x * 10 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[20, 20]"]);
    }

    #[test]
    fn vectorized_pipeline_not_filter() {
        let output = run_source(
            r#"
            let xs = [true, false, true, false]
            let result = xs |> filter(fn(x) { !x }) |> map(fn(x) { 1 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[1, 1]"]);
    }

    /// `compile_vec_body` for `if`/`else` pushes cond, then, else (else on top); `exec_vec_select`
    /// pops false/else, then/true, mask — matching that emission order.
    #[test]
    fn vectorized_pipeline_map_if_else_scalar_branches() {
        let output = run_source(
            r#"
            let xs = [-1, 2, -3, 4]
            let result = xs |> map(fn(x) { if x > 0 { x } else { -x } })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[1, 2, 3, 4]"]);
    }

    #[test]
    fn vectorized_pipeline_map_if_else_then_filter() {
        let output = run_source(
            r#"
            let xs = [-2, 3, -4, 5]
            let result = xs |> map(fn(x) { if x > 0 { x * 2 } else { x } }) |> filter(fn(x) { x > 0 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[6, 10]"]);
    }