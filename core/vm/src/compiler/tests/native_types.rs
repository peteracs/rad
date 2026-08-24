
    #[test]
    fn native_scalars_opaque_enums_and_bitflags_execute_with_runtime_identity() {
        let output = run_source(
            r#"
opaque type ManagerGid = u32
opaque type WireGid = u32

enum GoalSeekMode: u8 {
    Idle = 0,
    GoalSeek = 1,
    RemoteSourceGlide = 2,
}

bitflags CharacterState: u32 {
    Alive = 1,
    Moving = 2,
    Casting = 4,
}

let manager = ManagerGid(u32(42))
let wire = WireGid(u32(manager))
print(typeof(manager))
print(typeof(wire))
print(GoalSeekMode::RemoteSourceGlide)
print(CharacterState::Casting)
print(f32(0.1))
"#,
        );
        assert_eq!(
            output,
            vec![
                "ManagerGid",
                "WireGid",
                "GoalSeekMode(2)",
                "CharacterState(4)",
                "f32(0.10000000149011612)",
            ]
        );
    }

    #[test]
    fn native_integer_conversion_rejects_overflow() {
        let error = run_source_result("let value = u8(256)").unwrap_err();
        assert!(error.contains("overflow for u8"), "{error}");
    }

    #[test]
    fn native_and_opaque_integer_values_convert_back_to_int_with_range_checks() {
        let output = run_source(
            r#"
opaque type WireGid = u32
opaque type Sequence = u64
enum GoalSeekMode: u8 {
    Idle = 0,
    GoalSeek = 1,
    RemoteSourceGlide = 2,
}
print(int(u8(255)))
print(int(i32(-7)))
print(int(WireGid(u32(4294967295))))
print(int(Sequence(u64(77))))
print(int(GoalSeekMode::RemoteSourceGlide))
print(float(f32(1.25)))
assert(is_none(try_int(u64(9223372036854775807) + u64(1))), "overflow is fallible")
assert((try_float(f32(2.5)) |> unwrap) == 2.5, "native float is fallible-convertible")
"#,
        );
        assert_eq!(
            output,
            vec![
                "255",
                "-7",
                "4294967295",
                "77",
                "2",
                "1.25",
            ]
        );

        let error = run_source_result(
            "let too_large = u64(9223372036854775807) + u64(1)\nprint(int(too_large))",
        )
        .unwrap_err();
        assert!(error.contains("out of i64 range"), "{error}");
    }

    #[test]
    fn one_nominal_id_cannot_be_rewrapped_as_another_without_unwrapping() {
        let error = run_source_result(
            r#"
opaque type ManagerGid = u32
opaque type WireGid = u32
let manager = ManagerGid(u32(7))
let wire = WireGid(manager)
"#,
        )
        .unwrap_err();
        assert!(error.contains("cannot convert ManagerGid directly to WireGid"), "{error}");
    }

    #[test]
    fn packed_repr_layout_and_endian_descriptors_execute_exactly() {
        let output = run_source(
            r#"
            opaque type DeviceId = u32
            packed repr(C) struct Frame {
                device: DeviceId,
                sequence: u32,
                temperature: f32,
            }
            let bytes = bytebuf_from_list([42, 0, 0, 0])
            print(size_of(Frame))
            print(offset_of(Frame, "temperature"))
            print(decode_le(bytes, 0, u32))
            let signed = i32(-7)
            print(decode_le(encode_le(signed), 0, i32) == signed)
            "#,
        );
        assert_eq!(output, vec!["12", "8", "u32(42)", "true"]);
    }

    #[test]
    fn native_struct_encoding_uses_closed_layout_and_zero_padding() {
        let output = run_source(
            r#"
            packed repr(C) struct WireTail {
                skill: u32,
                region: u16,
                x: f32,
            }
            repr(C) struct Padded {
                tag: u8,
                value: u32,
            }
            repr(C) struct Nested {
                prefix: u16,
                payload: Padded,
            }

            let tail = WireTail {
                skill: u32(287454020),
                region: u16(39338),
                x: f32(-2.5),
            }
            let nested = Nested {
                prefix: u16(4660),
                payload: Padded { tag: u8(127), value: u32(1432778632) },
            }
            print(bytebuf_to_list(encode_le(tail)))
            print(bytebuf_to_list(encode_be(tail)))
            print(bytebuf_to_list(encode_le(nested)))
            "#,
        );
        assert_eq!(
            output,
            vec![
                "[68, 51, 34, 17, 170, 153, 0, 0, 32, 192]",
                "[17, 34, 51, 68, 153, 170, 192, 32, 0, 0]",
                "[52, 18, 0, 0, 127, 0, 0, 0, 136, 119, 102, 85]",
            ]
        );
    }
