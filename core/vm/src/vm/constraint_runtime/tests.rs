

#[cfg(test)]
mod resource_contract_tests {
    use super::{
        builtin_resource_charge, has_mechanically_checked_contract, NativeProofClass,
        NATIVE_CONTRACTS,
    };
    use crate::gc::GcHeap;
    use crate::leak_lab::measure_peak_bytes;
    use crate::value::{Builtin, Object, Value};
    use crate::vm::builtins_impl::{
        bi_bitset_clear, bi_bitset_set, bi_filled, bi_range, bi_replace, bi_typeof,
    };
    use std::collections::BTreeSet;
    use std::sync::Arc;

    #[test]
    fn constraint_native_whitelist_is_closed_and_enumerated() {
        let expected = BTreeSet::from([
            "abs",
            "bitset_clear",
            "bitset_has",
            "bitset_new",
            "bitset_set",
            "byte_at",
            "byte_len",
            "ceil",
            "chr",
            "clamp",
            "ctz",
            "ends_with",
            "filled",
            "floor",
            "int_div",
            "len",
            "max",
            "min",
            "ord",
            "popcount",
            "pow",
            "range",
            "replace",
            "round",
            "shl",
            "shr",
            "sign",
            "sqrt",
            "starts_with",
            "typeof",
        ]);
        let actual = Builtin::ALL
            .iter()
            .copied()
            .filter(|builtin| has_mechanically_checked_contract(*builtin))
            .map(Builtin::name)
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
        assert_eq!(NATIVE_CONTRACTS.len(), expected.len());
        assert!(NATIVE_CONTRACTS
            .iter()
            .all(|contract| !contract.proof_id.is_empty()));
    }

    #[test]
    fn exact_option_and_string_reduce_counterexamples_fail_closed() {
        let mut gc = GcHeap::new();
        let empty = Value::list(&mut gc, Vec::new());
        let text = Value::from_string(&mut gc, "abcdef".into());
        for (builtin, args) in [
            (Builtin::Find, vec![empty]),
            (Builtin::MaxBy, vec![empty]),
            (Builtin::MinBy, vec![empty]),
            (Builtin::Reduce, vec![text]),
        ] {
            let error = builtin_resource_charge(builtin, &args).unwrap_err();
            assert!(
                error.contains("no mechanically verified"),
                "{builtin:?}: {error}"
            );
        }
    }

    #[test]
    fn admitted_dynamic_contracts_dominate_measured_native_peak() {
        let mut gc = GcHeap::new();
        let mut covered = BTreeSet::new();

        let filled_args = vec![Value::int(16_384), Value::int(7)];
        let filled_quote = builtin_resource_charge(Builtin::Filled, &filled_args).unwrap();
        let (filled, filled_peak) = measure_peak_bytes(|| bi_filled(&mut gc, filled_args).unwrap());
        assert!(
            filled_peak <= filled_quote.heap,
            "filled: {filled_peak} > {}",
            filled_quote.heap
        );
        let _ = filled;
        covered.insert(Builtin::Filled.name());

        let range_args = vec![Value::int(0), Value::int(16_384)];
        let range_quote = builtin_resource_charge(Builtin::Range, &range_args).unwrap();
        let (range, range_peak) = measure_peak_bytes(|| bi_range(&mut gc, range_args).unwrap());
        assert!(
            range_peak <= range_quote.heap,
            "range: {range_peak} > {}",
            range_quote.heap
        );
        let _ = range;
        covered.insert(Builtin::Range.name());

        for values in [
            [i64::MAX - 1, i64::MAX, 2],
            [i64::MIN + 1, i64::MIN, -2],
            [i64::MAX, i64::MIN, i64::MIN],
        ] {
            let range_args = values
                .into_iter()
                .map(|value| Value::from_int(&mut gc, value))
                .collect::<Vec<_>>();
            let quote = builtin_resource_charge(Builtin::Range, &range_args).unwrap();
            let (result, peak) = measure_peak_bytes(|| bi_range(&mut gc, range_args).unwrap());
            assert!(
                peak <= quote.heap,
                "boundary range: {peak} > {}",
                quote.heap
            );
            assert_eq!(result.as_list().unwrap().len() as u64, quote.fuel);
        }

        let bitset = Value::bitset(&mut gc, vec![u64::MAX; 4_096]);
        let bitset_args = vec![bitset, Value::int(65_536)];
        let bitset_quote = builtin_resource_charge(Builtin::BitsetSet, &bitset_args).unwrap();
        let (bitset_result, bitset_peak) =
            measure_peak_bytes(|| bi_bitset_set(&mut gc, bitset_args).unwrap());
        assert!(
            bitset_peak <= bitset_quote.heap,
            "bitset_set: {bitset_peak} > {}",
            bitset_quote.heap
        );
        let _ = bitset_result;
        covered.insert(Builtin::BitsetSet.name());

        let clear_args = vec![bitset, Value::int(0)];
        let clear_quote = builtin_resource_charge(Builtin::BitsetClear, &clear_args).unwrap();
        let (clear_result, clear_peak) =
            measure_peak_bytes(|| bi_bitset_clear(&mut gc, clear_args).unwrap());
        assert!(
            clear_peak <= clear_quote.heap,
            "bitset_clear: {clear_peak} > {}",
            clear_quote.heap
        );
        let _ = clear_result;
        covered.insert(Builtin::BitsetClear.name());

        let replace_args = vec![
            Value::from_string(&mut gc, "a".repeat(8_192)),
            Value::from_string(&mut gc, String::new()),
            Value::from_string(&mut gc, "界".repeat(64)),
        ];
        let replace_quote = builtin_resource_charge(Builtin::Replace, &replace_args).unwrap();
        let (replace_result, replace_peak) =
            measure_peak_bytes(|| bi_replace(&mut gc, replace_args).unwrap());
        assert!(
            replace_peak <= replace_quote.heap,
            "replace: {replace_peak} > {}",
            replace_quote.heap
        );
        let _ = replace_result;
        covered.insert(Builtin::Replace.name());

        let component = Value::component(
            &mut gc,
            "VeryLongReplayComponent".repeat(512),
            Arc::new(Vec::new()),
            Vec::new(),
        );
        let typeof_args = vec![component];
        let typeof_quote = builtin_resource_charge(Builtin::TypeOf, &typeof_args).unwrap();
        let (typeof_result, typeof_peak) =
            measure_peak_bytes(|| bi_typeof(&mut gc, typeof_args).unwrap());
        assert!(
            typeof_peak <= typeof_quote.heap,
            "typeof: {typeof_peak} > {}",
            typeof_quote.heap
        );
        let _ = typeof_result;
        covered.insert(Builtin::TypeOf.name());

        let required = NATIVE_CONTRACTS
            .iter()
            .filter(|contract| {
                !matches!(
                    contract.proof_class,
                    NativeProofClass::Fixed | NativeProofClass::TextScan
                )
            })
            .map(|contract| contract.builtin.name())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            covered, required,
            "every dynamic proof class needs a peak case"
        );
    }

    #[test]
    fn admitted_fixed_contracts_dominate_measured_native_peak() {
        let mut vm = crate::vm::VM::new_with_seed(7);
        let text = Value::from_string(&mut vm.gc, "rad界".into());
        let prefix = Value::from_string(&mut vm.gc, "rad".into());
        let suffix = Value::from_string(&mut vm.gc, "界".into());
        let bitset = Value::bitset(&mut vm.gc, vec![0b1010]);
        let cases = vec![
            (Builtin::BitsetNew, vec![]),
            (Builtin::Abs, vec![Value::int(-7)]),
            (Builtin::Sign, vec![Value::int(-7)]),
            (Builtin::Min, vec![Value::int(1), Value::int(2)]),
            (Builtin::Max, vec![Value::int(1), Value::int(2)]),
            (Builtin::Chr, vec![Value::int(0x754c)]),
            (Builtin::Ord, vec![text]),
            (Builtin::IntDiv, vec![Value::int(7), Value::int(2)]),
            (Builtin::Popcount, vec![Value::int(0b1010)]),
            (Builtin::Ctz, vec![Value::int(0b1000)]),
            (Builtin::Shl, vec![Value::int(1), Value::int(4)]),
            (Builtin::Shr, vec![Value::int(16), Value::int(4)]),
            (
                Builtin::Clamp,
                vec![Value::int(5), Value::int(0), Value::int(10)],
            ),
            (Builtin::Round, vec![Value::from_float(1.5)]),
            (Builtin::Floor, vec![Value::from_float(1.5)]),
            (Builtin::Ceil, vec![Value::from_float(1.5)]),
            (Builtin::Sqrt, vec![Value::from_float(4.0)]),
            (
                Builtin::Pow,
                vec![Value::from_float(2.0), Value::from_float(8.0)],
            ),
            (Builtin::ByteAt, vec![text, Value::int(0)]),
            (Builtin::ByteLen, vec![text]),
            (Builtin::BitsetHas, vec![bitset, Value::int(3)]),
            (Builtin::Len, vec![text]),
            (Builtin::StartsWith, vec![text, prefix]),
            (Builtin::EndsWith, vec![text, suffix]),
        ];

        let covered = cases
            .iter()
            .map(|(builtin, _)| builtin.name())
            .collect::<BTreeSet<_>>();
        let required = NATIVE_CONTRACTS
            .iter()
            .filter(|contract| {
                matches!(
                    contract.proof_class,
                    NativeProofClass::Fixed | NativeProofClass::TextScan
                )
            })
            .map(|contract| contract.builtin.name())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            covered, required,
            "every fixed proof record needs a boundary case"
        );

        for (builtin, args) in cases {
            let quote = builtin_resource_charge(builtin, &args).unwrap();
            let (result, measured_peak) = measure_peak_bytes(|| vm.call_builtin(builtin, args));
            result.unwrap_or_else(|error| panic!("{builtin:?}: {error}"));
            assert!(
                measured_peak <= quote.heap,
                "{builtin:?}: measured {measured_peak} > quoted {}",
                quote.heap
            );
        }
    }

    #[test]
    fn bitset_clear_quote_includes_the_existing_vector_clone() {
        let mut gc = GcHeap::new();
        let words = 16_384usize;
        let bitset = Value::bitset(&mut gc, vec![u64::MAX; words]);
        let quote = builtin_resource_charge(Builtin::BitsetClear, &[bitset, Value::int(0)])
            .expect("bitset_clear is audited");

        assert!(
            quote.heap
                >= words
                    .saturating_mul(std::mem::size_of::<u64>())
                    .saturating_add(std::mem::size_of::<Object>()),
            "the quote must cover cloning the complete existing bitset"
        );
    }

    #[test]
    fn empty_pattern_replace_quote_covers_the_replacement_output() {
        let mut gc = GcHeap::new();
        let source = Value::from_string(&mut gc, String::new());
        let pattern = Value::from_string(&mut gc, String::new());
        let replacement_text = "x".repeat(32_768);
        let replacement = Value::from_string(&mut gc, replacement_text.clone());
        let quote = builtin_resource_charge(Builtin::Replace, &[source, pattern, replacement])
            .expect("replace is audited");

        assert!(
            quote.heap
                >= replacement_text
                    .len()
                    .saturating_mul(4)
                    .saturating_add(std::mem::size_of::<Object>()),
            "the quote must cover Rust's temporary String and the retained RAD string"
        );
    }

    #[test]
    fn bitset_growth_quote_covers_source_clone_and_resized_result() {
        let mut gc = GcHeap::new();
        let words = 257usize;
        let target_bit = 65_536i64;
        let bitset = Value::bitset(&mut gc, vec![u64::MAX; words]);
        let args = [bitset, Value::int(target_bit)];
        let quote =
            builtin_resource_charge(Builtin::BitsetSet, &args).expect("bitset_set is audited");
        let before = gc.bytes_allocated();
        bi_bitset_set(&mut gc, args.to_vec()).unwrap();
        assert!(gc.bytes_allocated().saturating_sub(before) <= quote.heap);
        assert!(
            quote.heap
                >= words
                    .saturating_mul(std::mem::size_of::<u64>())
                    .saturating_add(
                        usize::try_from(target_bit)
                            .unwrap()
                            .saturating_div(64)
                            .saturating_add(1)
                            .saturating_mul(std::mem::size_of::<u64>()),
                    )
        );
    }

    #[test]
    fn shrinking_replace_quote_keeps_the_no_match_source_as_upper_bound() {
        let mut gc = GcHeap::new();
        let source_text = "x".repeat(32_769);
        let args = [
            Value::from_string(&mut gc, source_text.clone()),
            Value::from_string(&mut gc, "absent-pattern".into()),
            Value::from_string(&mut gc, String::new()),
        ];
        let quote = builtin_resource_charge(Builtin::Replace, &args).expect("replace is audited");
        assert!(
            quote.heap
                >= source_text
                    .len()
                    .saturating_mul(4)
                    .saturating_add(std::mem::size_of::<Object>())
        );
    }

    #[test]
    fn audited_quotes_dominate_retained_allocation_across_boundaries() {
        for words in [0usize, 1, 8, 257, 16_384] {
            let mut gc = GcHeap::new();
            let bitset = Value::bitset(&mut gc, vec![u64::MAX; words]);
            let args = [bitset, Value::int(0)];
            let quote = builtin_resource_charge(Builtin::BitsetClear, &args).unwrap();
            let before = gc.bytes_allocated();
            bi_bitset_clear(&mut gc, args.to_vec()).unwrap();
            assert!(gc.bytes_allocated().saturating_sub(before) <= quote.heap);
        }

        for (source, pattern, replacement) in [
            ("", "", "x".repeat(4096)),
            ("ééé", "", "界".repeat(256)),
            ("aaaaaa", "aa", "replacement".repeat(64)),
            ("unchanged", "missing", "x".repeat(1024)),
        ] {
            let mut gc = GcHeap::new();
            let args = [
                Value::from_string(&mut gc, source.to_string()),
                Value::from_string(&mut gc, pattern.to_string()),
                Value::from_string(&mut gc, replacement),
            ];
            let quote = builtin_resource_charge(Builtin::Replace, &args).unwrap();
            let before = gc.bytes_allocated();
            bi_replace(&mut gc, args.to_vec()).unwrap();
            assert!(gc.bytes_allocated().saturating_sub(before) <= quote.heap);
        }
    }

    #[test]
    fn native_helpers_without_proven_bounds_fail_closed() {
        let mut gc = GcHeap::new();
        let empty = Value::map(&mut gc, Default::default());
        for builtin in [
            Builtin::Keys,
            Builtin::Entries,
            Builtin::Merge,
            Builtin::RemoveKey,
            Builtin::Sort,
            Builtin::IndexOf,
            Builtin::GroupBy,
        ] {
            let error = builtin_resource_charge(builtin, &[empty]).unwrap_err();
            assert!(
                error.contains("no proven") || error.contains("no mechanically verified"),
                "{builtin:?}: {error}"
            );
        }
    }
}
