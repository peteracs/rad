// Pointer-free native-extension manifests recorded in `.radr` files. Replay
// reconstructs callable stubs from this data and never opens the live plugin;
// `VM::call_native` consumes the recorded result before reaching the stub.

#[derive(serde::Deserialize, serde::Serialize)]
struct RecordedNativePlugin {
    extension_id: String,
    extension_version: String,
    content_digest: String,
    target: String,
    manifest_digest: String,
    exports: Vec<RecordedNativeExport>,
    abi_contract: NativeAbiContract,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct RecordedNativeExport {
    name: String,
    arity: u32,
    effects: Vec<String>,
    signature: String,
    deterministic: bool,
    replayable: bool,
}

pub(crate) type RecordedPluginRegistration = (
    Vec<(String, NativeFnInfo)>,
    std::sync::Arc<NativeExtensionManifest>,
);

pub(crate) fn encode_recorded_plugin(
    manifest: &NativeExtensionManifest,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(RecordedNativePlugin {
        extension_id: manifest.extension_id().to_string(),
        extension_version: manifest.extension_version().unwrap_or_default().to_string(),
        content_digest: manifest.content_digest().to_string(),
        target: manifest.target().to_string(),
        manifest_digest: manifest.digest().to_string(),
        exports: manifest
            .exported_functions()
            .iter()
            .map(|export| RecordedNativeExport {
                name: export.name().to_string(),
                arity: export.arity(),
                effects: export.effects().canonical_strings(),
                signature: export.signature().to_string(),
                deterministic: export.deterministic(),
                replayable: export.replayable(),
            })
            .collect(),
        abi_contract: manifest.abi_contract().clone(),
    })
    .map_err(|error| format!("cannot record native plugin manifest: {error}"))
}

pub(crate) fn decode_recorded_plugin(
    encoded: serde_json::Value,
) -> Result<RecordedPluginRegistration, String> {
    let recorded: RecordedNativePlugin = serde_json::from_value(encoded)
        .map_err(|error| format!("recorded native plugin manifest is malformed: {error}"))?;
    let exports = recorded
        .exports
        .iter()
        .map(|export| {
            Ok(NativeExportManifest {
                name: export.name.clone(),
                arity: export.arity,
                effects: NativeEffectSet::parse(&export.effects)?,
                signature: export.signature.clone(),
                deterministic: export.deterministic,
                replayable: export.replayable,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let manifest = std::sync::Arc::new(NativeExtensionManifest::from_replay(
        recorded.extension_id,
        recorded.extension_version,
        recorded.content_digest,
        recorded.target,
        exports.clone(),
        recorded.abi_contract,
        &recorded.manifest_digest,
    )?);
    let functions = exports
        .into_iter()
        .map(|export| {
            let name = export.name.clone();
            (
                name.clone(),
                NativeFnInfo {
                    name,
                    execution: NativeExecution::Replay,
                    arity: export.arity,
                    effects: export.effects,
                    signature: export.signature,
                    deterministic: export.deterministic,
                    replayable: export.replayable,
                    extension: std::sync::Arc::clone(&manifest),
                },
            )
        })
        .collect();
    Ok((functions, manifest))
}
