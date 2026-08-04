

    #[test]
    fn bug6_question_mark_ok_from_main_succeeds() {
        let output = run_source(
            r#"
            type Result {
                Ok { value: int }
                Err { value: str }
            }
            fn succeeding() -> Result {
                return Result::Ok { value: 42 }
            }
            fn main() -> Result {
                let x = succeeding()?
                print(x)
                return Result::Ok { value: x }
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    // ================================================================
    // BUG FIX ROUND 2 — VERIFICATION TESTS
    // ================================================================

    #[test]
    fn bug7_int_min_div_neg1_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = x / -1
            print(y)
        "#,
        );
        assert!(result.is_err(), "INT_MIN / -1 should error, not panic");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug7b_int_min_mod_neg1_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = x % -1
            print(y)
        "#,
        );
        assert!(result.is_err(), "INT_MIN % -1 should error, not panic");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug7_normal_div_still_works() {
        let output = run_source(
            r#"
            print(10 / 3)
            print(-10 / 3)
            print(100 % 7)
        "#,
        );
        assert_eq!(output, vec!["3", "-3", "2"]);
    }

    #[test]
    fn bug8_negate_int_min_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = -x
            print(y)
        "#,
        );
        assert!(result.is_err(), "negating INT_MIN should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug8_normal_negation_works() {
        let output = run_source(
            r#"
            print(-42)
            print(-(-100))
            print(-(0))
        "#,
        );
        assert_eq!(output, vec!["-42", "100", "0"]);
    }

    #[test]
    fn bug9_int_of_huge_float_errors() {
        let result = run_source_result(
            r#"
            let x = 1.0e19
            let y = int(x)
            print(y)
        "#,
        );
        assert!(
            result.is_err(),
            "int(1e19) should error, not silently saturate"
        );
        assert!(result.unwrap_err().contains("out of i64 range"));
    }

    #[test]
    fn bug9_int_of_normal_float_works() {
        let output = run_source(
            r#"
            print(int(3.14))
            print(int(-2.7))
            print(int(42.0))
        "#,
        );
        assert_eq!(output, vec!["3", "-2", "42"]);
    }

    #[test]
    fn bug10_int_div_builtin_min_neg1_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = int_div(x, -1)
            print(y)
        "#,
        );
        assert!(result.is_err(), "int_div(INT_MIN, -1) should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug11_abs_int_min_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = abs(x)
            print(y)
        "#,
        );
        assert!(
            result.is_err(),
            "abs(INT_MIN) should error, not return negative"
        );
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug11_normal_abs_works() {
        let output = run_source(
            r#"
            print(abs(-42))
            print(abs(100))
            print(abs(0))
            print(abs(-3.14))
        "#,
        );
        assert_eq!(output, vec!["42", "100", "0", "3.14"]);
    }

    #[test]
    fn math_round_floor_ceil() {
        let output = run_source(
            r#"
            print(round(2.5))
            print(round(-2.5))
            print(round(2.4))
            print(round(7))
            print(floor(-1.2))
            print(floor(1.8))
            print(ceil(-1.2))
            print(ceil(1.2))
        "#,
        );
        assert_eq!(output, vec!["3", "-3", "2", "7", "-2", "1", "-1", "2"]);
    }

    #[test]
    fn math_sqrt_pow() {
        let output = run_source(
            r#"
            print(sqrt(144.0))
            print(sqrt(2))
            print(pow(2, 10))
            print(pow(2.0, -1.0))
        "#,
        );
        assert_eq!(output, vec!["12.0", "1.4142135623730951", "1024", "0.5"]);
    }

    #[test]
    fn math_sqrt_negative_errors() {
        let result = run_source_result("print(sqrt(-1.0))");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("sqrt() of negative number"));
    }

    #[test]
    fn math_pow_int_overflow_errors() {
        let result = run_source_result("print(pow(10, 200))");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn to_fixed_formats_negatives_correctly() {
        let output = run_source(
            r#"
            print(to_fixed(-143.90, 2))
            print(to_fixed(3.14159, 3))
            print(to_fixed(2.0, 0))
            print(to_fixed(5, 2))
        "#,
        );
        assert_eq!(output, vec!["-143.90", "3.142", "2", "5.00"]);
    }

    #[test]
    fn json_round_trip_struct_and_collections() {
        let output = run_source(
            r#"
            struct Task { id: int = 0, text: str = "", done: bool = false }
            let t = Task { id: 7, text: "buy milk", done: true }
            let s = json_stringify(t)
            print(s)
            let back = json_parse(s) |> unwrap
            print(back["id"])
            print(back["text"])
            print(back["done"])
            let nested = {"a": [1, 2], "b": [3]}
            let n2 = json_parse(json_stringify(nested)) |> unwrap
            print(n2["a"][1])
            print(n2["b"][0])
        "#,
        );
        assert_eq!(
            output,
            vec![
                "{\"done\":true,\"id\":7,\"text\":\"buy milk\"}",
                "7",
                "buy milk",
                "true",
                "2",
                "3"
            ]
        );
    }

    #[test]
    fn json_parse_invalid_returns_none() {
        let output = run_source(
            r#"
            print(is_none(json_parse("{nope")))
            print(is_none(json_parse("null")))
            print(json_parse("[1, 2.5, \"x\", null, true]") |> unwrap)
        "#,
        );
        assert_eq!(output, vec!["true", "false", "[1, 2.5, \"x\", nil, true]"]);
    }

    #[test]
    fn json_stringify_rejects_non_finite() {
        let result = run_source_result(
            r#"
            let huge = pow(10.0, 400.0)
            print(json_stringify(huge))
        "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("non-finite"));
    }

    #[test]
    fn self_referential_record_assignment_survives_getlocal2_fusion() {
        let output = run_source(
            r#"
            component SearchState {
                items: list = []
                depth: int = 0
            }

            fn rebuild(items: list<int>, depth: int) -> SearchState {
                return SearchState { items: items, depth: depth }
            }

            let mut candidate = SearchState { items: [], depth: 0 }
            let items = push(candidate.items, 7)
            candidate = rebuild(items, candidate.depth + 1)
            print(candidate.depth)
            print(candidate.items)
        "#,
        );
        assert_eq!(output, vec!["1", "[7]"]);
    }
