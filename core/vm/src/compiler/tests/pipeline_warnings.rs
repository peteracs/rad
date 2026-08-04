

    #[test]
    fn w2505_warning_for_non_vectorizable_closure() {
        let (output, warnings) = compile_with_warnings(
            r#"
            fn complex(x) {
                if x > 0 { return x } else { return -x }
            }
            let xs = [1, -2, 3]
            let result = xs |> map(complex) |> map(fn(x) { x + 1 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[2, 3, 4]"]);
        assert!(
            warnings.iter().any(|w| w.message.contains("W2505")),
            "Expected W2505 warning for non-vectorizable pipeline, got: {:?}",
            warnings
        );
    }

    #[test]
    fn no_w2505_for_fully_vectorizable_pipeline() {
        let (output, warnings) = compile_with_warnings(
            r#"
            let xs = [1, 2, 3]
            let result = xs |> map(fn(x) { x * 2 }) |> filter(fn(x) { x > 3 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[4, 6]"]);
        assert!(
            !warnings.iter().any(|w| w.message.contains("W2505")),
            "Should not emit W2505 for vectorizable pipeline"
        );
    }

    #[test]
    fn no_w2505_for_flat_map_pipeline() {
        let (_, warnings) = compile_with_warnings(
            r#"
            let xs = [[1, 2], [3, 4]]
            let result = xs |> flat_map(fn(x) { x }) |> map(fn(x) { x * 2 })
            print(result)
        "#,
        );
        assert!(
            !warnings.iter().any(|w| w.message.contains("W2505")),
            "Should not emit W2505 when FlatMap is present"
        );
    }

    #[test]
    fn load_column_ecs_test() {
        let output = run_source(
            r#"
            component Position { x: 0.0, y: 0.0 }

            fn main() -> nil {
                spawn(Position { x: 10.0, y: 20.0 })
                spawn(Position { x: 30.0, y: 40.0 })

                let entities = query { Position }
                let xs = entities |> map(fn(e) { (get(e, Position) |> unwrap).x }) |> map(fn(x) { x + 1.0 })
                print(xs)
            }
        "#,
        );
        assert_eq!(output, vec!["[11.0, 31.0]"]);
    }

    #[test]
    fn fstring_format_spec_float_precision() {
        let output = run_source(
            r#"
            let pi = 3.14159265
            print(f"{pi:.2f}")
        "#,
        );
        assert_eq!(output, vec!["3.14"]);
    }

    #[test]
    fn fstring_format_spec_int_zero_pad() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:06d}")
        "#,
        );
        assert_eq!(output, vec!["000042"]);
    }

    #[test]
    fn fstring_format_spec_right_align() {
        let output = run_source(
            r#"
            let name = "hi"
            print(f"{name:>10}")
        "#,
        );
        assert_eq!(output, vec!["        hi"]);
    }

    #[test]
    fn fstring_format_spec_left_align() {
        let output = run_source(
            r#"
            let x = 5
            print(f"{x:<5d}")
        "#,
        );
        assert_eq!(output, vec!["5    "]);
    }

    #[test]
    fn fstring_format_spec_center_align() {
        let output = run_source(
            r#"
            let x = "ab"
            print(f"{x:^6}")
        "#,
        );
        assert_eq!(output, vec!["  ab  "]);
    }

    #[test]
    fn fstring_format_spec_hex() {
        let output = run_source(
            r#"
            let x = 255
            print(f"{x:#x}")
        "#,
        );
        assert_eq!(output, vec!["0xff"]);
    }

    #[test]
    fn fstring_format_spec_binary() {
        let output = run_source(
            r#"
            let x = 10
            print(f"{x:#b}")
        "#,
        );
        assert_eq!(output, vec!["0b1010"]);
    }

    #[test]
    fn fstring_format_spec_sign_plus() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:+d}")
        "#,
        );
        assert_eq!(output, vec!["+42"]);
    }

    #[test]
    fn fstring_format_spec_percentage() {
        let output = run_source(
            r#"
            let x = 0.75
            print(f"{x:.1%}")
        "#,
        );
        assert_eq!(output, vec!["75.0%"]);
    }

    #[test]
    fn fstring_format_spec_fill_char() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:*>6d}")
        "#,
        );
        assert_eq!(output, vec!["****42"]);
    }

    #[test]
    fn fstring_format_spec_string_truncation() {
        let output = run_source(
            r#"
            let s = "hello world"
            print(f"{s:.5}")
        "#,
        );
        assert_eq!(output, vec!["hello"]);
    }

    #[test]
    fn fstring_format_spec_no_spec() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x}")
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn fstring_format_spec_mixed_parts() {
        let output = run_source(
            r#"
            let x = 3.14159
            print(f"Pi is {x:.2f}!")
        "#,
        );
        assert_eq!(output, vec!["Pi is 3.14!"]);
    }

    #[test]
    fn fstring_format_spec_dollar_brace() {
        let output = run_source(
            r#"
            let x = 255
            print(f"${x:#06x}")
        "#,
        );
        assert_eq!(output, vec!["0x00ff"]);
    }

    #[test]
    fn fstring_format_spec_scientific() {
        let output = run_source(
            r#"
            let x = 12345.6789
            print(f"{x:.2e}")
        "#,
        );
        assert_eq!(output, vec!["1.23e+04"]);
    }

    #[test]
    fn format_value_builtin_direct() {
        let output = run_source(
            r#"
            print(format_value(42, "08d"))
        "#,
        );
        assert_eq!(output, vec!["00000042"]);
    }

    #[test]
    fn fstring_format_default_int_align() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:10d}")
        "#,
        );
        assert_eq!(output, vec!["        42"]);
    }

    #[test]
    fn fstring_format_default_str_align() {
        let output = run_source(
            r#"
            let s = "hi"
            print(f"{s:10}")
        "#,
        );
        assert_eq!(output, vec!["hi        "]);
    }

    #[test]
    fn fstring_format_negative_with_plus() {
        let output = run_source(
            r#"
            let x = -42
            print(f"{x:+d}")
        "#,
        );
        assert_eq!(output, vec!["-42"]);
    }

    #[test]
    fn fstring_format_large_width() {
        let output = run_source(
            r#"
            let x = 1
            print(f"{x:50d}")
        "#,
        );
        let expected = format!("{:>50}", "1");
        assert_eq!(output, vec![expected]);
    }

    #[test]
    fn fstring_format_zero_pad_no_width() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:0d}")
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn fstring_format_int_width_no_type() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:10}")
        "#,
        );
        assert_eq!(output, vec!["        42"]);
    }

    #[test]
    fn fstring_format_sci_upper() {
        let output = run_source(
            r#"
            let x = 12345.6789
            print(f"{x:.2E}")
        "#,
        );
        assert_eq!(output, vec!["1.23E+04"]);
    }

    #[test]
    fn fstring_format_sci_negative_exp() {
        let output = run_source(
            r#"
            let x = 0.001
            print(f"{x:.2e}")
        "#,
        );
        assert_eq!(output, vec!["1.00e-03"]);
    }

    #[test]
    fn anonymous_entity_literal_basic() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            fn main() -> nil {
                let e = entity { Health { hp: 42 } }
                let h = require(e, Health)
                print(h.hp)
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn anonymous_entity_literal_multiple_components() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            component Position { x: int = 0, y: int = 0 }
            fn main() -> nil {
                let e = entity { Health { hp: 10 }, Position { x: 3, y: 4 } }
                let h = require(e, Health)
                let p = require(e, Position)
                print(h.hp)
                print(p.x)
                print(p.y)
            }
        "#,
        );
        assert_eq!(output, vec!["10", "3", "4"]);
    }

    #[test]
    fn anonymous_entity_literal_as_argument() {
        let output = run_source(
            r#"
            component Tag { name: str = "" }
            fn show_tag(e: entity) -> nil {
                let t = require(e, Tag)
                print(t.name)
            }
            fn main() -> nil {
                show_tag(entity { Tag { name: "hello" } })
            }
        "#,
        );
        assert_eq!(output, vec!["hello"]);
    }

    #[test]
    fn anonymous_entity_literal_as_return_value() {
        let output = run_source(
            r#"
            component Label { text: str = "" }
            fn make_label(t: str) -> entity {
                return entity { Label { text: t } }
            }
            fn main() -> nil {
                let e = make_label("world")
                print(require(e, Label).text)
            }
        "#,
        );
        assert_eq!(output, vec!["world"]);
    }

    #[test]
    fn anonymous_entity_literal_nested() {
        let output = run_source(
            r#"
            component Inner { val: int = 0 }
            component Outer { child: entity = spawn() }
            fn main() -> nil {
                let e = entity {
                    Outer { child: entity { Inner { val: 99 } } }
                }
                let o = require(e, Outer)
                let i = require(o.child, Inner)
                print(i.val)
            }
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn named_entity_still_works_after_anonymous_feature() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            entity player { Health { hp: 100 } }
            fn main() -> nil {
                print(require(player, Health).hp)
            }
        "#,
        );
        assert_eq!(output, vec!["100"]);
    }

    #[test]
    fn named_entity_literal_with_string_name() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            fn main() -> nil {
                let e = entity "hero" { Health { hp: 42 } }
                let found = get_entity("hero")
                print(require(found, Health).hp)
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn named_entity_literal_with_variable_name() {
        let output = run_source(
            r#"
            component Tag { label: str = "" }
            fn make(name: str) -> entity {
                return entity name { Tag { label: "ok" } }
            }
            fn main() -> nil {
                let e = make("dynamic")
                let found = get_entity("dynamic")
                print(require(found, Tag).label)
            }
        "#,
        );
        assert_eq!(output, vec!["ok"]);
    }

    #[test]
    fn named_entity_literal_anonymous_still_works() {
        let output = run_source(
            r#"
            component Val { x: int = 0 }
            fn main() -> nil {
                let e = entity { Val { x: 7 } }
                print(require(e, Val).x)
            }
        "#,
        );
        assert_eq!(output, vec!["7"]);
    }

    #[test]
    fn named_entity_literal_variable_empty_body() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let name = "empty_one"
                let e = entity name {}
                let found = get_entity("empty_one")
                print(found == e)
            }
        "#,
        );
        assert_eq!(output, vec!["true"]);
    }

    #[test]
    fn entity_literal_with_variable_component_entry() {
        let output = run_source(
            r#"
            component Val { x: int = 0 }
            fn main() -> nil {
                let v = Val { x: 42 }
                let e = entity { v }
                print(require(e, Val).x)
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn entity_literal_with_call_component_entry() {
        let output = run_source(
            r#"
            component Val { x: int = 0 }
            fn make_val() -> Val {
                return Val { x: 99 }
            }
            fn main() -> nil {
                let e = entity { make_val() }
                print(require(e, Val).x)
            }
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn entity_literal_mixed_init_and_expr() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            component Tag { name: str = "" }
            fn main() -> nil {
                let t = Tag { name: "hero" }
                let e = entity { Health { hp: 100 }, t }
                print(require(e, Health).hp)
                print(require(e, Tag).name)
            }
        "#,
        );
        assert_eq!(output, vec!["100", "hero"]);
    }

    #[test]
    fn closure_destructure_works_in_filter_and_map() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [["a", 2], ["b", 1], ["c", 2]]
                let names = rows
                    |> filter(fn([name, phase]) { return phase == 2 })
                    |> map(fn([name, phase]) { return name })
                print(len(names))
                print(names[0])
                print(names[1])
            }
        "#,
        );
        assert_eq!(output, vec!["2", "a", "c"]);
    }

    #[test]
    fn closure_destructure_works_with_mixed_params() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [["x", 2], ["y", 3]]
                let total = rows |> reduce(0, fn(acc, [name, phase]) { return acc + phase })
                print(total)
            }
        "#,
        );
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn for_loop_destructure_unpacks_each_row() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[1, 2], [3, 4]]
                let mut sum = 0
                for [a, b] in rows {
                    sum = sum + a + b
                }
                print(sum)
            }
        "#,
        );
        assert_eq!(output, vec!["10"]);
    }

    #[test]
    fn closure_destructure_arity_mismatch_fails_loudly() {
        let err = run_source_result(
            r#"
            fn main() -> nil {
                let rows = [[1, 2]]
                let _ = rows |> map(fn([a, b, c]) { return a + b + c })
            }
        "#,
        )
        .expect_err("expected destructure arity mismatch to fail");
        assert!(
            err.contains("out of bounds"),
            "expected out-of-bounds error, got: {}",
            err
        );
    }

    #[test]
    fn closure_single_element_destructure() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[10], [20], [30]]
                let out = rows |> map(fn([x]) { return x * 2 })
                print(out[0])
                print(out[1])
                print(out[2])
            }
        "#,
        );
        assert_eq!(output, vec!["20", "40", "60"]);
    }

    #[test]
    fn for_loop_destructure_with_index_value_pairs() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let pairs = [[0, "a"], [1, "b"], [2, "c"]]
                for [idx, val] in pairs {
                    print(f"{idx}:{val}")
                }
            }
        "#,
        );
        assert_eq!(output, vec!["0:a", "1:b", "2:c"]);
    }

    #[test]
    fn for_loop_destructure_tuple_rows() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [(1, 2), (3, 4), (5, 6)]
                let mut sum = 0
                for [a, b] in rows {
                    sum = sum + a + b
                }
                print(sum)
            }
        "#,
        );
        assert_eq!(output, vec!["21"]);
    }

    #[test]
    fn closure_both_params_destructured_in_reduce() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[1, 2], [3, 4], [5, 6]]
                let out = rows |> reduce([0, 0], fn([sa, sb], [a, b]) {
                    return [sa + a, sb + b]
                })
                print(out[0])
                print(out[1])
            }
        "#,
        );
        assert_eq!(output, vec!["9", "12"]);
    }

    #[test]
    fn closure_three_params_all_destructured() {
        let output = run_source(
            r#"
            fn apply3(f, a, b, c) { return f(a, b, c) }
            fn main() -> nil {
                let r = apply3(
                    fn([x1, x2], [y1, y2], [z1, z2]) {
                        return x1 + y1 + z1 + x2 + y2 + z2
                    },
                    [1, 2], [3, 4], [5, 6]
                )
                print(r)
            }
        "#,
        );
        assert_eq!(output, vec!["21"]);
    }

    #[test]
    fn closure_underscore_discard_in_destructure() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[1, 2, 3], [4, 5, 6]]
                let mids = rows |> map(fn([_, mid, _]) { return mid })
                print(mids[0])
                print(mids[1])
            }
        "#,
        );
        assert_eq!(output, vec!["2", "5"]);
    }

    // ================================================================
    // BUG FIX VERIFICATION TESTS
    // ================================================================

    // BUG 1: Integer overflow now errors instead of silently wrapping
    #[test]
    fn bug1_integer_overflow_add() {
        let result = run_source_result(
            r#"
            let x = 9223372036854775807
            let y = x + 1
            print(y)
        "#,
        );
        assert!(result.is_err(), "overflow on add should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug1_integer_overflow_sub() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807
            let y = x - 2
            print(y)
        "#,
        );
        assert!(result.is_err(), "overflow on sub should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug1_integer_overflow_mul() {
        let result = run_source_result(
            r#"
            let x = 9223372036854775807
            let y = x * 2
            print(y)
        "#,
        );
        assert!(result.is_err(), "overflow on mul should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug1_normal_arithmetic_still_works() {
        let output = run_source(
            r#"
            print(100 + 200)
            print(500 - 100)
            print(10 * 20)
            print(-50 + 50)
        "#,
        );
        assert_eq!(output, vec!["300", "400", "200", "0"]);
    }

    // BUG 2: Double despawn now errors
    #[test]
    fn bug2_double_despawn_errors() {
        let result = run_source_result(
            r#"
            component HP { val: 100 }
            fn main() -> nil {
                let e = spawn(HP { val: 42 })
                despawn(e)
                despawn(e)
            }
        "#,
        );
        assert!(result.is_err(), "double despawn should error");
        assert!(result.unwrap_err().contains("non-existent entity"));
    }

    #[test]
    fn bug2_single_despawn_still_works() {
        let output = run_source(
            r#"
            component HP { val: 100 }
            fn main() -> nil {
                let e = spawn(HP { val: 42 })
                print("before")
                despawn(e)
                print("after")
            }
        "#,
        );
        assert_eq!(output, vec!["before", "after"]);
    }

    // BUG 3: set() / update() on despawned entity now errors
    #[test]
    fn bug3_set_on_despawned_entity_errors() {
        let result = run_source_result(
            r#"
            component HP { val: 100 }
            fn main() -> nil {
                let e = spawn(HP { val: 42 })
                despawn(e)
                set(e, HP { val: 999 })
            }
        "#,
        );
        assert!(result.is_err(), "set on despawned entity should error");
        assert!(result.unwrap_err().contains("non-existent entity"));
    }

    // BUG 4: -0.0 == 0.0 now works correctly (IEEE 754)
    #[test]
    fn bug4_negative_zero_equals_positive_zero() {
        let output = run_source(
            r#"
            let a = -0.0
            let b = 0.0
            if a == b {
                print("equal")
            } else {
                print("not equal")
            }
        "#,
        );
        assert_eq!(output, vec!["equal"]);
    }

    #[test]
    fn bug4_negative_zero_arithmetic() {
        let output = run_source(
            r#"
            let a = -0.0
            let b = 0.0
            let c = a + b
            if c == 0.0 {
                print("sum is zero")
            }
            if a == b {
                print("neg zero equals zero")
            }
            let d = -1.0 * 0.0
            if d == 0.0 {
                print("minus one times zero equals zero")
            }
        "#,
        );
        assert_eq!(
            output,
            vec![
                "sum is zero",
                "neg zero equals zero",
                "minus one times zero equals zero"
            ]
        );
    }

    #[test]
    fn bug4_normal_float_equality_still_works() {
        let output = run_source(
            r#"
            let a = 3.14
            let b = 3.14
            if a == b {
                print("equal")
            } else {
                print("not equal")
            }
        "#,
        );
        assert_eq!(output, vec!["equal"]);
    }

    // BUG 6: ? propagation from main() now errors
    #[test]
    fn bug6_question_mark_err_from_main_errors() {
        let result = run_source_result(
            r#"
            type Result {
                Ok { value: int }
                Err { value: str }
            }
            fn failing() -> Result {
                return Result::Err { value: "something broke" }
            }
            fn main() -> Result {
                let x = failing()?
                return Result::Ok { value: x }
            }
        "#,
        );
        assert!(
            result.is_err(),
            "Err propagated from main via ? should error"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("Unhandled error")
                || err.contains("something broke")
                || err.contains("propagated"),
            "Error message should mention the unhandled error: {}",
            err
        );
    }