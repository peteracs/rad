// Nominal typing for `opaque`, `enum`, and `bitflags` native types.
//
// These types share a representation with an ordinary number but are not one:
// `DeviceId` and `SessionId` over `u32` are distinct, and neither is `u32`.
// Argument position was the one place the checker forgot that.

    /// Argument position used to skip the type check whenever both sides were
    /// numeric, so two `opaque` types over the same representation were
    /// interchangeable there — `bookcore/negative/signed_as_unsigned.rad` and
    /// `forgelink/negative/wrong_nominal_id.rad` both compiled clean. The same
    /// mistake written as a `let` binding was always rejected.
    #[test]
    fn opaque_types_are_nominal_in_argument_position() {
        let errors = check_src(
            "opaque type DeviceId = u32
             opaque type SessionId = u32
             fn open_device(device: DeviceId) -> nil {}
             let session: SessionId = SessionId(u32(7))
             open_device(session)",
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("expects DeviceId, got SessionId")),
            "got: {:?}",
            errors
        );
    }

    /// The representation is not the type: a bare `u32` is not a `DeviceId`.
    #[test]
    fn a_raw_representation_is_not_its_opaque_type() {
        let errors = check_src(
            "opaque type DeviceId = u32
             fn open_device(device: DeviceId) -> nil {}
             open_device(u32(7))",
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("expects DeviceId, got u32")),
            "got: {:?}",
            errors
        );
    }

    /// Tightening nominal types must not make ordinary numbers stricter:
    /// `int` still reaches a `float` parameter, and a plain scalar native is
    /// still a number rather than a nominal identity.
    #[test]
    fn plain_numeric_arguments_still_convert() {
        let errors = check_src(
            "fn scale(factor: float) -> nil {}
             scale(2)",
        );
        assert!(
            !errors.iter().any(|e| e.message.contains("Argument 1")),
            "got: {:?}",
            errors
        );
    }
