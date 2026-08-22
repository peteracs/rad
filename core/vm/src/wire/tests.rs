// Round-trip and rejection tests for the v2 wire codec.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::GcHeap;

    #[test]
    fn provenance_truncation_marker_roundtrips_exactly() {
        let provenance = WireProvenance {
            truncation: ProvenanceTruncation {
                evicted_records: 41,
                digest: *blake3::hash(b"omitted provenance").as_bytes(),
            },
            ..WireProvenance::default()
        };
        let mut encoded = String::new();
        encode_prov_into(&provenance, &mut encoded);
        let json: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        let decoded = decode_prov(&json).unwrap();
        assert_eq!(decoded.truncation, provenance.truncation);
    }

    #[test]
    fn provenance_truncation_marker_rejects_inconsistent_count_and_digest() {
        let malformed: serde_json::Value = serde_json::from_str(
            r#"[[],[],[],[],[],[],[0,"1111111111111111111111111111111111111111111111111111111111111111"]]"#,
        )
        .unwrap();
        assert_eq!(
            decode_prov(&malformed).unwrap_err(),
            "prov: inconsistent truncation marker"
        );
    }

    #[test]
    fn scalar_roundtrip_preserves_types() {
        let mut gc = GcHeap::new();
        let vals = vec![
            Value::NIL,
            Value::from_bool(true),
            Value::from_int(&mut gc, 42),
            Value::from_int(&mut gc, -7),
            Value::from_float(1.0), // integral float must stay float
            Value::from_float(2.5),
            Value::from_float(1e300),
            Value::from_string(&mut gc, "he said \"hi\"\n".to_string()),
            Value::from_entity_id(&mut gc, 9),
        ];
        for v in vals {
            let mut s = String::new();
            encode_value_into(&v, &mut s).unwrap();
            let j: serde_json::Value = serde_json::from_str(&s).unwrap();
            let back = decode_value(&mut gc, &j).unwrap();
            assert_eq!(v.type_name(), back.type_name(), "wire: {}", s);
            assert_eq!(v.to_string(), back.to_string(), "wire: {}", s);
            // And re-encoding is byte-identical (canonical form).
            let mut s2 = String::new();
            encode_value_into(&back, &mut s2).unwrap();
            assert_eq!(s, s2);
        }
    }

    /// A4 BUG 04 (seq 51): f64::MAX used to be written as its 309-digit
    /// decimal expansion, which serde_json rejects on re-parse ("number out
    /// of range") — save_world() wrote a save that load_world() and
    /// fork_from_bytes() refused. Every finite float the VM can hold must
    /// round-trip bit-exactly through the canonical wire text, and integral
    /// floats must keep their float marker (never decay to int).
    #[test]
    fn extreme_float_magnitudes_roundtrip_bit_exact() {
        let mut gc = GcHeap::new();
        for x in [
            f64::MAX,
            -f64::MAX,
            1.1e308,
            1e308,
            1e300,
            1.5e17,
            1e17,
            f64::MIN_POSITIVE,
            5e-324, // smallest subnormal
            3.0,
            -0.0,
            0.1,
        ] {
            let v = Value::from_float(x);
            let mut s = String::new();
            encode_value_into(&v, &mut s).unwrap();
            let j: serde_json::Value = serde_json::from_str(&s)
                .unwrap_or_else(|e| panic!("wire text for {:e} must re-parse ({}): {}", x, e, s));
            let back = decode_value(&mut gc, &j).unwrap();
            let y = back
                .as_float()
                .unwrap_or_else(|| panic!("float marker lost for {:e}: {}", x, s));
            assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "round-trip must be bit-exact for {:e}: {}",
                x,
                s
            );
            // Re-encoding the decoded value is byte-identical (canonical).
            let mut s2 = String::new();
            encode_value_into(&back, &mut s2).unwrap();
            assert_eq!(s, s2);
        }
    }

    /// The fix for BUG 04: extremes take the shortest exponent form, while
    /// everyday floats keep their established text (digest stability).
    #[test]
    fn float_wire_text_shape() {
        for (x, expected) in [
            (f64::MAX, "1.7976931348623157e308"),
            (-f64::MAX, "-1.7976931348623157e308"),
            (3.0, "3.0"),
            (-0.0, "-0.0"),
            (0.1, "0.1"),
            (1234567890.5, "1234567890.5"),
        ] {
            let mut s = String::new();
            encode_value_into(&Value::from_float(x), &mut s).unwrap();
            assert_eq!(s, expected);
        }
    }

    #[test]
    fn tuple_map_keys_roundtrip_canonically() {
        let mut gc = GcHeap::new();
        let four = Value::from_int(&mut gc, 4);
        let two = Value::from_int(&mut gc, 2);
        let k1 = Value::tuple(&mut gc, vec![four, two]);
        let a = Value::from_string(&mut gc, "a".into());
        let inner = Value::tuple(&mut gc, vec![a, Value::from_bool(true)]);
        let one = Value::from_int(&mut gc, 1);
        let k2 = Value::tuple(&mut gc, vec![one, inner]);
        let mut m = MapStorage::new();
        m.insert(
            MapKey::from_value(&k1).unwrap(),
            Value::from_int(&mut gc, 6),
        );
        m.insert(
            MapKey::from_value(&k2).unwrap(),
            Value::from_int(&mut gc, 9),
        );
        m.insert(MapKey::Str("plain".into()), Value::from_int(&mut gc, 1));
        let v = Value::map(&mut gc, m);

        let mut s = String::new();
        encode_value_into(&v, &mut s).unwrap();
        let j: serde_json::Value = serde_json::from_str(&s).unwrap();
        let back = decode_value(&mut gc, &j).unwrap();
        let bm = back.as_map().unwrap();
        assert_eq!(bm.len(), 3);
        assert_eq!(
            bm[&MapKey::from_value(&k1).unwrap()].as_int(),
            Some(6),
            "wire: {}",
            s
        );
        assert_eq!(bm[&MapKey::from_value(&k2).unwrap()].as_int(), Some(9));
        // canonical: re-encoding is byte-identical
        let mut s2 = String::new();
        encode_value_into(&back, &mut s2).unwrap();
        assert_eq!(s, s2);
    }

    #[test]
    fn float_map_key_is_rejected() {
        let mut gc = GcHeap::new();
        let bad = Value::tuple(&mut gc, vec![Value::from_float(1.5)]);
        assert!(MapKey::from_value(&bad).is_err());
    }

    #[test]
    fn float_int_distinction_survives() {
        let mut gc = GcHeap::new();
        let mut s = String::new();
        encode_value_into(&Value::from_float(5.0), &mut s).unwrap();
        assert_eq!(s, "5.0");
        let j: serde_json::Value = serde_json::from_str(&s).unwrap();
        let back = decode_value(&mut gc, &j).unwrap();
        assert!(back.as_float().is_some());
        assert!(back.as_int().is_none() || back.as_float() == Some(5.0));
    }

    #[test]
    fn provenance_u32_fields_reject_overflow_instead_of_wrapping() {
        let overflows = [u32::MAX as u64 + 1, u64::MAX];
        for overflow in overflows {
            let write = serde_json::json!([
                [[0, overflow, null, "Health", "{}", 0, [0], null, null, []]],
                [],
                [],
                [],
                [],
                [],
                [0, "0000000000000000000000000000000000000000000000000000000000000000"]
            ]);
            let error = decode_prov(&write).expect_err("write entity overflow");
            assert!(error.contains("write entity exceeds u32"), "{error}");

            let proposal_key = serde_json::json!([
                [],
                [],
                [[1, 0, [0]]],
                [[1, 1, "Damage", overflow, "{}", "Hit", 1]],
                [],
                [],
                [0, "0000000000000000000000000000000000000000000000000000000000000000"]
            ]);
            let error = decode_prov(&proposal_key).expect_err("proposal key overflow");
            assert!(error.contains("proposal key exceeds u32"), "{error}");

            let proposal_line = serde_json::json!([
                [],
                [],
                [[1, 0, [0]]],
                [[1, 1, "Damage", 0, "{}", "Hit", overflow]],
                [],
                [],
                [0, "0000000000000000000000000000000000000000000000000000000000000000"]
            ]);
            let error = decode_prov(&proposal_line).expect_err("source line overflow");
            assert!(
                error.contains("proposal source line exceeds u32"),
                "{error}"
            );

            let resolution = serde_json::json!([
                [],
                [],
                [[1, 0, [0]]],
                [],
                [[1, 1, "Damage", overflow, "ResolveDamage", []]],
                [],
                [0, "0000000000000000000000000000000000000000000000000000000000000000"]
            ]);
            let error = decode_prov(&resolution).expect_err("resolution key overflow");
            assert!(error.contains("resolution key exceeds u32"), "{error}");
        }
    }

    #[test]
    fn wire_entity_values_and_map_keys_reject_overflow() {
        let mut gc = GcHeap::new();
        for overflow in [u32::MAX as u64 + 1, u64::MAX] {
            let entity = serde_json::json!({"e": overflow});
            let error = decode_value(&mut gc, &entity).expect_err("entity overflow");
            assert!(error.contains("entity id exceeds u32"), "{error}");

            let map = serde_json::json!({"m": [[["e", overflow], 1]]});
            let error = decode_value(&mut gc, &map).expect_err("map key overflow");
            assert!(error.contains("entity map key exceeds u32"), "{error}");
        }
    }
}
