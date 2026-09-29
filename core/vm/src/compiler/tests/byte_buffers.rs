    #[test]
    fn bytebuf_u16_little_endian_round_trips_through_both_inplace_forms() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let mut bytes: bytebuf = bytebuf_new(4)
                bytes = bytebuf_set_u16_le(bytes, 0, 4660)
                bytes = bytes |> bytebuf_set_u16_le(2, 43981)
                print(bytebuf_get_u16_le(bytes, 0))
                print(bytebuf_get_u16_le(bytes, 2))
                print(bytebuf_to_list(bytes))
            }
        "#,
        );
        assert_eq!(output, vec!["4660", "43981", "[52, 18, 205, 171]"]);
    }

    #[test]
    fn bytebuf_u16_rejects_out_of_range_values_and_partial_writes() {
        let range_error = run_source_result(
            r#"
            let mut bytes: bytebuf = bytebuf_new(2)
            bytes = bytebuf_set_u16_le(bytes, 0, 65536)
        "#,
        )
        .expect_err("u16 writes must not truncate");
        assert!(range_error.contains("0..65535"), "{range_error}");

        let bounds_error = run_source_result(
            r#"
            let mut bytes: bytebuf = bytebuf_new(2)
            bytes = bytebuf_set_u16_le(bytes, 1, 1)
        "#,
        )
        .expect_err("two-byte writes must fit in full");
        assert!(bounds_error.contains("2-byte write"), "{bounds_error}");
    }

    #[test]
    fn bytebuf_bulk_operations_are_checked_and_preserve_value_semantics() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let source: bytebuf = bytebuf_from_list([10, 20, 30])
                let destination: bytebuf = bytebuf_from_list([1, 2, 3, 4, 5])
                let copied: bytebuf = bytebuf_copy(destination, 1, source)
                print(bytebuf_to_list(destination))
                print(bytebuf_to_list(copied))
                print(bytebuf_to_list(bytebuf_slice(copied, 1, 4)))
            }
            "#,
        );
        assert_eq!(
            output,
            vec!["[1, 2, 3, 4, 5]", "[1, 10, 20, 30, 5]", "[10, 20, 30]"]
        );

        for (source, expected) in [
            (
                "let bytes = bytebuf_new(3)\nlet _ = bytebuf_slice(bytes, 2, 1)",
                "range 2..1 out of bounds",
            ),
            (
                "let bytes = bytebuf_new(3)\nlet _ = bytebuf_slice(bytes, 0, 4)",
                "range 0..4 out of bounds",
            ),
            (
                "let destination = bytebuf_new(3)\nlet source = bytebuf_new(2)\nlet _ = bytebuf_copy(destination, 2, source)",
                "source length 2 at offset 2 exceeds destination length 3",
            ),
            (
                "let mut destination = bytebuf_new(3)\nlet source = bytebuf_new(2)\ndestination = bytebuf_copy(destination, 2, source)",
                "source length 2 at offset 2 exceeds destination length 3",
            ),
        ] {
            let error = run_source_result(source).expect_err("invalid bulk bytebuf operation");
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn bytebuf_copy_to_unique_local_lowers_in_place_in_both_assignment_forms() {
        let source = r#"
            fn main() -> nil {
                let source: bytebuf = bytebuf_from_list([7, 8])
                let mut destination: bytebuf = bytebuf_new(4)
                destination = bytebuf_copy(destination, 0, source)
                destination = destination |> bytebuf_copy(2, source)
                destination = bytebuf_copy(destination, 0, destination)
                assert(bytebuf_to_list(destination) == [7, 8, 7, 8], "bulk copy")
            }
        "#;
        let tokens = Lexer::new(source).tokenize().0;
        let program = Parser::new(tokens).parse();
        let result = Compiler::new().compile(&program).expect("program compiles");
        let mut vm = VM::new();
        vm.op_profile = true;
        vm.load_compile_result(result);
        vm.run(0).expect("bulk-copy program runs");
        assert_eq!(
            vm.op_counts[crate::opcode::Op::ByteBufCopyInplace as usize],
            3,
            "both assignment shapes and alias-safe self-copy must use the allocation-free opcode"
        );
    }

    #[test]
    fn bytebuf_bulk_native_loops_cannot_hide_work_from_system_budgets() {
        for operation in [
            "let mut destination = bytebuf_new(64)\nlet source = bytebuf_new(64)\ndestination = bytebuf_copy(destination, 0, source)",
            "let source = bytebuf_new(64)\nlet _ = bytebuf_slice(source, 0, 64)",
        ] {
            let source = format!(
                "@budget(instructions: 16)\nsystem Limited() {{\n{operation}\n}}\nLimited()"
            );
            let error = run_source_result(&source)
                .expect_err("bulk native loops must consume proportional system work");
            assert!(
                error.contains("exceeded its @budget(instructions: 16)"),
                "{error}"
            );
        }
    }

    #[test]
    fn checked_pure_bytebuf_helpers_only_need_causal_lowering_when_enabled() {
        let helper = r#"
            pure fn encoded_value() -> bytebuf {
                let mut bytes: bytebuf = bytebuf_new(7)
                bytes = bytebuf_set_u8(bytes, 0, 42)
                bytes = bytes |> bytebuf_set_u16_le(1, 4660)
                bytes = bytebuf_set_u32_le(bytes, 3, 123456)
                return bytes
            }
            pure fn aliased_values() -> int {
                let mut bytes: bytebuf = bytebuf_new(1)
                let saved: bytebuf = bytes
                bytes = bytebuf_set_u8(bytes, 0, 7)
                assert(bytebuf_get(saved, 0) == 0, "the copied value must remain independent")
                return bytebuf_get(bytes, 0)
            }
        "#;
        // `experimental-laws` is the accepted alias of `causal_laws`. Every
        // spelling that enables settlement syntax must also enable the
        // conservative helper lowering, or a law could reach an in-place op.
        for feature in [None, Some("causal_laws"), Some("experimental-laws")] {
            let causal_enabled = feature.is_some();
            let caller = if causal_enabled {
                r#"
                    component ResultValue { value: int = 0 }
                    intent ResultIntent { key target: entity, value: int }
                    law Evaluate(target: entity) {
                        let bytes: bytebuf = encoded_value()
                        propose ResultIntent { target: target, value: bytebuf_get_u16_le(bytes, 1) }
                    }
                    resolver Store for ResultIntent(target, proposals) {
                        next(target, ResultValue { value: proposals[0].value })
                    }
                    entity output { ResultValue {} }
                    settle { Evaluate(output) }
                    print(require(output, ResultValue).value)
                "#
            } else {
                "print(bytebuf_get_u16_le(encoded_value(), 1))"
            };
            let result = crate::test_support::compile_checked_source(
                &format!("{helper}\n{caller}\nassert(aliased_values() == 7, \"updated value\")"),
                ParserOptions,
                crate::checker::CheckerOptions {
                    features: feature.map(str::to_string).into_iter().collect(),
                    ..Default::default()
                },
            )
            .expect("checked helper and its caller compile");
            let mut vm = VM::new();
            vm.op_profile = true;
            vm.load_compile_result(result);
            vm.run(0).expect("helper executes, including inside a law");
            assert_eq!(vm.print_buffer, vec!["4660"]);
            for op in [
                crate::opcode::Op::ByteBufSetU8Inplace,
                crate::opcode::Op::ByteBufSetU16LeInplace,
                crate::opcode::Op::ByteBufSetU32LeInplace,
            ] {
                assert_eq!(
                    vm.op_counts[op as usize],
                    u64::from(!causal_enabled),
                    "ordinary checked helpers must avoid copies, while causal helpers retain functional lowering: {op:?} under {feature:?}"
                );
            }
        }
    }
