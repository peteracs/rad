// Effect-declaration and ABI-manifest tests for the native bridge.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod semantic_tests {
    use super::*;
    use std::ffi::CString;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static NATIVE_INVOCATIONS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn nil_native(_args: *const u64, _argc: usize) -> u64 {
        Value::NIL.to_raw()
    }

    unsafe extern "C" fn counted_native(_args: *const u64, _argc: usize) -> u64 {
        NATIVE_INVOCATIONS.fetch_add(1, AtomicOrdering::SeqCst);
        Value::NIL.to_raw()
    }

    fn empty_contract() -> NativeAbiContract {
        NativeAbiContract::parse(
            r#"{"contract_version":1,"calling_convention":"C","types":[]}"#,
        )
        .unwrap()
    }

    #[test]
    fn native_effects_are_exact_sorted_and_fail_closed() {
        let effects = NativeEffectSet::declare(&[
            "writes:Live",
            "reads:Identity",
            "emits:Published",
            "io",
            "async",
        ])
        .unwrap();
        assert_eq!(effects.reads(), ["Identity"]);
        assert_eq!(effects.writes(), ["Live"]);
        assert_eq!(effects.emits(), ["Published"]);
        assert!(effects.performs_io());
        assert!(effects.is_async());
        assert!(NativeEffectSet::declare(&["reads:Live", "reads:Live"])
            .unwrap_err()
            .contains("duplicate"));
        assert!(NativeEffectSet::declare(&["network"])
            .unwrap_err()
            .contains("unknown native effect"));
        assert!(NativeEffectSet::declare(&["writes:"])
            .unwrap_err()
            .contains("invalid authority name"));
    }

    #[test]
    fn zero_effect_registration_never_forms_a_slice_from_null() {
        let name = CString::new("plain").unwrap();
        let signature = CString::new("()->nil").unwrap();
        let declaration = RadNativeFunctionDecl {
            name: name.as_ptr(),
            func: nil_native,
            arity: 0,
            effects: std::ptr::null(),
            effect_count: 0,
            signature: signature.as_ptr(),
            deterministic: true,
            replayable: true,
        };
        let mut context = RegistrationContext {
            functions: Vec::new(),
            errors: Vec::new(),
        };
        unsafe {
            api_register_fn(
                &mut context as *mut RegistrationContext as *mut c_void,
                &declaration,
            );
        }
        assert!(context.errors.is_empty(), "{:?}", context.errors);
        assert_eq!(context.functions.len(), 1);
        assert_eq!(context.functions[0].0, "plain");
        assert!(context.functions[0].3.canonical_strings().is_empty());
    }

    #[test]
    fn extension_manifest_identity_covers_binary_version_and_effects() {
        let export = NativeExportManifest::new(
            "retire",
            1,
            NativeEffectSet::declare(&["writes:Live"]).unwrap(),
            "(entity)->nil",
            true,
            true,
        )
        .unwrap();
        let base = NativeExtensionManifest::from_binary(
            Path::new("retire.dll"),
            b"binary-a",
            "entity.lifecycle".to_string(),
            "1.0.0".to_string(),
            std::slice::from_ref(&export),
            empty_contract(),
        );
        let changed_binary = NativeExtensionManifest::from_binary(
            Path::new("retire.dll"),
            b"binary-b",
            "entity.lifecycle".to_string(),
            "1.0.0".to_string(),
            std::slice::from_ref(&export),
            empty_contract(),
        );
        let changed_effect = NativeExtensionManifest::from_binary(
            Path::new("retire.dll"),
            b"binary-a",
            "entity.lifecycle".to_string(),
            "1.0.0".to_string(),
            &[NativeExportManifest::new(
                "retire",
                1,
                NativeEffectSet::declare(&["writes:Grave"]).unwrap(),
                "(entity)->nil",
                true,
                true,
            )
            .unwrap()],
            empty_contract(),
        );
        assert_ne!(base.digest(), changed_binary.digest());
        assert_ne!(base.digest(), changed_effect.digest());
        assert_eq!(base.abi_version(), RAD_EXTENSION_ABI_VERSION);
    }

    #[test]
    fn abi_contract_rejects_overlap_and_invalid_alignment() {
        let overlap = NativeAbiContract::parse(
            r#"{"contract_version":1,"calling_convention":"C","types":[{"name":"Packet","size":8,"alignment":4,"fields":[{"name":"a","type_name":"u32","offset":0,"size":4},{"name":"b","type_name":"u32","offset":2,"size":4}]}]}"#,
        )
        .unwrap_err();
        assert!(overlap.contains("overlap"), "{overlap}");
        let alignment = NativeAbiContract::parse(
            r#"{"contract_version":1,"calling_convention":"C","types":[{"name":"Packet","size":8,"alignment":3,"fields":[]}]}"#,
        )
        .unwrap_err();
        assert!(alignment.contains("invalid size/alignment"), "{alignment}");
    }

    #[test]
    fn extension_manifest_identity_covers_exact_layout_and_replay_contract() {
        let export = NativeExportManifest::new(
            "score",
            1,
            NativeEffectSet::default(),
            "(RiskInput)->RiskOutput",
            true,
            true,
        )
        .unwrap();
        let contract = NativeAbiContract::parse(
            r#"{"contract_version":1,"calling_convention":"C","types":[{"name":"RiskInput","size":8,"alignment":8,"fields":[{"name":"customer","type_name":"CustomerId","offset":0,"size":8}]}]}"#,
        )
        .unwrap();
        let base = NativeExtensionManifest::from_binary(
            Path::new("risk.dll"),
            b"same-binary",
            "risk".to_string(),
            "17".to_string(),
            std::slice::from_ref(&export),
            contract.clone(),
        );
        let changed_layout = NativeExtensionManifest::from_binary(
            Path::new("risk.dll"),
            b"same-binary",
            "risk".to_string(),
            "17".to_string(),
            std::slice::from_ref(&export),
            NativeAbiContract::parse(
                r#"{"contract_version":1,"calling_convention":"C","types":[{"name":"RiskInput","size":16,"alignment":8,"fields":[{"name":"customer","type_name":"CustomerId","offset":0,"size":8}]}]}"#,
            )
            .unwrap(),
        );
        let changed_replay = NativeExtensionManifest::from_binary(
            Path::new("risk.dll"),
            b"same-binary",
            "risk".to_string(),
            "17".to_string(),
            &[NativeExportManifest::new(
                "score",
                1,
                NativeEffectSet::default(),
                "(RiskInput)->RiskOutput",
                true,
                false,
            )
            .unwrap()],
            contract,
        );
        assert_ne!(base.digest(), changed_layout.digest());
        assert_ne!(base.digest(), changed_replay.digest());
    }

    #[test]
    fn verification_contract_reports_opaque_identity_at_the_field() {
        let report = NativeVerificationReport {
            plugin: "risk.dll".to_string(),
            extension_id: "risk".to_string(),
            extension_version: "17".to_string(),
            abi_version: RAD_EXTENSION_ABI_VERSION,
            content_digest: "content".to_string(),
            manifest_digest: "manifest".to_string(),
            target: "x86_64-windows".to_string(),
            calling_convention: "C".to_string(),
            exports: Vec::new(),
            layouts: vec![NativeTypeLayout {
                name: "RiskInput".to_string(),
                size: 8,
                alignment: 8,
                fields: vec![NativeFieldLayout {
                    name: "customer".to_string(),
                    type_name: "TransactionId".to_string(),
                    offset: 0,
                    size: 8,
                }],
            }],
            layout_agreement: true,
            opaque_identity: true,
            determinism_probe: false,
            replay_support: true,
        };
        let error = verify_plugin_contract(
            &report,
            r#"{"extension_id":"risk","abi_version":3,"calling_convention":"C","exports":[],"types":[{"name":"RiskInput","size":8,"alignment":8,"fields":[{"name":"customer","type_name":"CustomerId","offset":0,"size":8}]}]}"#,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "opaque type mismatch at 'RiskInput.customer': expected CustomerId, found TransactionId"
        );
    }

    #[test]
    fn replayable_false_rejects_before_native_execution() {
        NATIVE_INVOCATIONS.store(0, AtomicOrdering::SeqCst);
        let manifest = std::sync::Arc::new(NativeExtensionManifest::from_binary(
            Path::new("risk.dll"),
            b"generation-17",
            "risk".to_string(),
            "17".to_string(),
            &[],
            empty_contract(),
        ));
        let native = NativeFnInfo {
            name: "score".to_string(),
            execution: NativeExecution::InProcess(counted_native),
            arity: 0,
            effects: NativeEffectSet::default(),
            signature: "()->nil".to_string(),
            deterministic: true,
            replayable: false,
            extension: manifest,
        };
        let mut vm = crate::vm::VM::new_with_seed(1);
        vm.enable_recording("");
        let error = vm.call_native(&native, Vec::new()).unwrap_err();
        assert!(error.contains("replayable=false"), "{error}");
        assert_eq!(NATIVE_INVOCATIONS.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn same_extension_id_retains_independent_content_generations() {
        let first = NativeExtensionManifest::from_binary(
            Path::new("risk.dll"),
            b"generation-17",
            "risk".to_string(),
            "17".to_string(),
            &[],
            empty_contract(),
        );
        let second = NativeExtensionManifest::from_binary(
            Path::new("risk.dll"),
            b"generation-18",
            "risk".to_string(),
            "18".to_string(),
            &[],
            empty_contract(),
        );
        assert_eq!(first.extension_id(), second.extension_id());
        assert_ne!(first.content_digest(), second.content_digest());
        assert_ne!(first.digest(), second.digest());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::seal_native_library;
    use sha2::{Digest, Sha256};

    #[test]
    fn native_image_is_content_addressed_and_detached_from_source_path() {
        let nonce = super::SEALED_IMAGE_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "rad-native-seal-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let source = directory.join(format!("extension.{}", std::env::consts::DLL_EXTENSION));
        let original = format!("sealed-native-image-{nonce}").into_bytes();
        std::fs::write(&source, &original).expect("write source image");

        let sealed = seal_native_library(&source).expect("seal native image");
        let expected_digest = hex::encode(Sha256::digest(&original));
        assert!(sealed
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&expected_digest)));

        std::fs::write(&source, b"replacement image").expect("replace source path");
        assert_eq!(
            std::fs::read(&sealed.path).expect("read sealed image"),
            original
        );
        assert_eq!(
            std::fs::read(sealed.loader_path()).expect("read loader image"),
            original
        );
        assert_eq!(sealed.bytes, original);

        std::fs::remove_file(&source).expect("remove source image");
        std::fs::remove_dir(&directory).expect("remove test directory");
    }
}
