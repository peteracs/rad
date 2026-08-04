

#[cfg(test)]
mod tests {
    use super::{FrozenFloat, FrozenMapKey, FrozenValue};
    use crate::value::{ComponentData, Value};
    use crate::vm::VM;
    use crate::{CausalValueError, CausalValueLimits};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn frozen_values_round_trip_without_raw_heap_handles() {
        let source = FrozenValue::Component {
            type_name: "Payload".into(),
            fields: BTreeMap::from([
                ("name".into(), FrozenValue::String("hero".into())),
                (
                    "data".into(),
                    FrozenValue::Map(BTreeMap::from([(
                        FrozenMapKey::String("hp".into()),
                        FrozenValue::Int(100),
                    )])),
                ),
            ]),
        };
        let mut vm = VM::new();
        let handle = vm.import_value(&source).expect("owned import");
        assert_eq!(handle.to_owned().expect("owned export"), source);
    }

    #[test]
    fn importing_every_nan_pattern_stays_float() {
        for bits in [
            0x7FF0_0000_0000_0001,
            0x7FFC_0000_0000_0000,
            0xFFFC_0000_0000_0000,
            0xFFFF_FFFF_FFFF_FFFF,
        ] {
            let source = FrozenValue::Float(f64::from_bits(bits).into());
            let mut vm = VM::new();
            let exported = vm
                .import_value(&source)
                .expect("NaN import")
                .to_owned()
                .expect("NaN export");
            assert!(matches!(exported, FrozenValue::Float(value) if value.get().is_nan()));
        }
    }

    #[test]
    fn frozen_constructors_reject_duplicates_and_canonicalize_order_and_nan() {
        let duplicate_key = FrozenMapKey::String("same".into());
        assert!(matches!(
            FrozenValue::try_map([
                (duplicate_key.clone(), FrozenValue::Int(1)),
                (duplicate_key, FrozenValue::Int(2)),
            ]),
            Err(CausalValueError::DuplicateMapKey { .. })
        ));
        assert!(matches!(
            FrozenValue::try_component(
                "Pair",
                [
                    ("same".into(), FrozenValue::Int(1)),
                    ("same".into(), FrozenValue::Int(2)),
                ],
            ),
            Err(CausalValueError::DuplicateField { .. })
        ));
        assert!(matches!(
            FrozenValue::try_sum(
                "Choice",
                "One",
                [
                    ("same".into(), FrozenValue::Int(1)),
                    ("same".into(), FrozenValue::Int(2)),
                ],
            ),
            Err(CausalValueError::DuplicateField { .. })
        ));

        let first = FrozenValue::try_map([
            (FrozenMapKey::String("z".into()), FrozenValue::Int(2)),
            (FrozenMapKey::String("a".into()), FrozenValue::Int(1)),
        ])
        .expect("unique map");
        let second = FrozenValue::try_map([
            (FrozenMapKey::String("a".into()), FrozenValue::Int(1)),
            (FrozenMapKey::String("z".into()), FrozenValue::Int(2)),
        ])
        .expect("unique map");
        assert_eq!(first, second);
        assert_eq!(
            FrozenFloat::new(f64::from_bits(0xFFFF_FFFF_FFFF_FFFF)),
            FrozenFloat::new(f64::from_bits(0x7FF0_0000_0000_0001))
        );
    }

    #[test]
    fn encoded_byte_limit_is_exact_canonical_output_length() {
        let value = FrozenValue::try_component(
            "Escaped",
            [(
                "text".into(),
                FrozenValue::String("quote=\" newline=\n snowman=☃".into()),
            )],
        )
        .expect("canonical component");
        let unlimited = CausalValueLimits::default();
        let bytes = value.canonical_bytes(&unlimited).expect("canonical bytes");
        let text = String::from_utf8(bytes.clone()).expect("canonical UTF-8");
        assert_eq!(
            text,
            "{\"c\":[\"Escaped\",{\"text\":\"quote=\\\" newline=\\n snowman=☃\"}]}"
        );

        let exact = CausalValueLimits::default()
            .with_max_encoded_bytes(bytes.len())
            .expect("exact profile");
        assert_eq!(value.canonical_bytes(&exact).unwrap().len(), bytes.len());
        let short = CausalValueLimits::default()
            .with_max_encoded_bytes(bytes.len() - 1)
            .expect("short profile");
        assert_eq!(
            value.canonical_bytes(&short),
            Err(CausalValueError::EncodedByteLimit {
                limit: bytes.len() - 1,
                actual: bytes.len(),
            })
        );
    }

    #[test]
    fn fuzz_frozen_import_export_never_panics_or_changes_values() {
        fn next(seed: &mut u64) -> u64 {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            *seed
        }

        fn generate(seed: &mut u64, depth: usize) -> FrozenValue {
            if depth == 0 {
                return match next(seed) % 6 {
                    0 => FrozenValue::Nil,
                    1 => FrozenValue::Bool(next(seed) & 1 == 1),
                    2 => FrozenValue::Int(next(seed) as i64),
                    3 => FrozenValue::Float(((next(seed) as i32) as f64 / 7.0).into()),
                    4 => FrozenValue::String(format!("s{:016x}", next(seed))),
                    _ => FrozenValue::Bytes(next(seed).to_le_bytes().to_vec()),
                };
            }
            match next(seed) % 7 {
                0..=2 => generate(seed, 0),
                3 => FrozenValue::List(
                    (0..(next(seed) as usize % 5))
                        .map(|_| generate(seed, depth - 1))
                        .collect(),
                ),
                4 => FrozenValue::Tuple(
                    (0..(next(seed) as usize % 4))
                        .map(|_| generate(seed, depth - 1))
                        .collect(),
                ),
                5 => FrozenValue::Component {
                    type_name: "Generated".into(),
                    fields: (0..(next(seed) as usize % 4))
                        .map(|index| (format!("f{index}"), generate(seed, depth - 1)))
                        .collect(),
                },
                _ => FrozenValue::Map(
                    (0..(next(seed) as usize % 4))
                        .map(|index| {
                            (
                                FrozenMapKey::String(format!("k{index}")),
                                generate(seed, depth - 1),
                            )
                        })
                        .collect(),
                ),
            }
        }

        let iterations = std::env::var("RAD_FUZZ_ITERS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2_000);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut seed = 0xA11C_E5ED_5AFE_CAFE;
            let mut vm = VM::new();
            for _ in 0..iterations {
                let value = generate(&mut seed, 5);
                let round_trip = vm
                    .import_value(&value)
                    .expect("generated value is within default limits")
                    .to_owned()
                    .expect("generated value exports");
                assert_eq!(round_trip, value);
            }
        }));
        assert!(result.is_ok(), "FrozenValue import/export must never panic");
    }

    #[test]
    fn frozen_import_rejects_deep_and_wide_values_before_allocation() {
        let mut deep = FrozenValue::Nil;
        for _ in 0..256 {
            deep = FrozenValue::List(vec![deep]);
        }
        let mut vm = VM::new();
        let limits = CausalValueLimits::default()
            .with_max_depth(32)
            .expect("test profile");
        assert!(matches!(
            vm.import_value_with_limits(&deep, &limits),
            Err(CausalValueError::DepthLimit { limit: 32 })
        ));

        let wide = FrozenValue::List(vec![FrozenValue::Nil; 100_001]);
        assert!(matches!(
            vm.import_value(&wide),
            Err(CausalValueError::CollectionItemLimit { .. })
        ));
    }

    #[test]
    fn component_and_resource_exports_apply_one_cumulative_budget() {
        let mut vm = VM::new();
        let entity = vm.world.spawn_entity(Some("hero"));
        let component = ComponentData {
            type_name: "Pair".into(),
            layout: Arc::new(vec!["left".into(), "right".into()]),
            values: vec![
                Value::from_string(&mut vm.gc, "12345".into()),
                Value::from_string(&mut vm.gc, "67890".into()),
            ],
        };
        assert!(vm.world.set_component(entity, component.clone()));
        vm.world.init_resource("Pair", component);
        vm.set_causal_value_limits(
            CausalValueLimits::default()
                .with_max_encoded_bytes(12)
                .expect("test profile"),
        );

        assert!(matches!(
            vm.component_value(entity, "Pair"),
            Err(CausalValueError::EncodedByteLimit { limit: 12, .. })
        ));
        assert!(matches!(
            vm.resource_value("Pair"),
            Err(CausalValueError::EncodedByteLimit { limit: 12, .. })
        ));
    }
}
