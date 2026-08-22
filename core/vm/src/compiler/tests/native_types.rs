
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
