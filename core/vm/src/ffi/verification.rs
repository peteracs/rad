// Read-only extension verification used by the CLI and release gates. Loading
// and probing happen against a detached GC arena; no RAD world is available to
// mutate during verification.

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct NativeVerificationReport {
    pub plugin: String,
    pub extension_id: String,
    pub extension_version: String,
    pub abi_version: u32,
    pub content_digest: String,
    pub manifest_digest: String,
    pub target: String,
    pub calling_convention: String,
    pub exports: Vec<NativeVerificationExport>,
    pub layouts: Vec<NativeTypeLayout>,
    pub layout_agreement: bool,
    pub opaque_identity: bool,
    pub determinism_probe: bool,
    pub replay_support: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct NativeVerificationExport {
    pub name: String,
    pub signature: String,
    pub arity: u32,
    pub effects: Vec<String>,
    pub effect_class: String,
    pub deterministic: bool,
    pub replayable: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct NativeVerificationContract {
    pub extension_id: String,
    pub abi_version: u32,
    pub calling_convention: String,
    pub exports: Vec<NativeExpectedExport>,
    pub types: Vec<NativeTypeLayout>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct NativeExpectedExport {
    pub name: String,
    pub signature: String,
    pub effects: Vec<String>,
    pub deterministic: bool,
    pub replayable: bool,
}

pub fn verify_plugin_contract(
    report: &NativeVerificationReport,
    encoded: &str,
) -> Result<(), String> {
    let expected: NativeVerificationContract = serde_json::from_str(encoded)
        .map_err(|error| format!("expected FFI contract is invalid JSON: {error}"))?;
    if expected.extension_id != report.extension_id {
        return Err(format!(
            "extension identity mismatch: expected '{}', found '{}'",
            expected.extension_id, report.extension_id
        ));
    }
    if expected.abi_version != report.abi_version {
        return Err(format!(
            "ABI version mismatch: expected {}, found {}",
            expected.abi_version, report.abi_version
        ));
    }
    if expected.calling_convention != report.calling_convention {
        return Err(format!(
            "calling convention mismatch: expected {}, found {}",
            expected.calling_convention, report.calling_convention
        ));
    }
    if expected.exports.len() != report.exports.len() {
        return Err(format!(
            "native export count mismatch: expected {}, found {}",
            expected.exports.len(),
            report.exports.len()
        ));
    }
    for export in &expected.exports {
        let found = report
            .exports
            .iter()
            .find(|candidate| candidate.name == export.name)
            .ok_or_else(|| format!("missing required native export '{}'", export.name))?;
        if found.signature != export.signature {
            return Err(format!(
                "native export '{}' signature mismatch: expected {}, found {}",
                export.name, export.signature, found.signature
            ));
        }
        if found.effects != export.effects
            || found.deterministic != export.deterministic
            || found.replayable != export.replayable
        {
            return Err(format!(
                "native export '{}' effect/replay contract mismatch",
                export.name
            ));
        }
    }
    for layout in &expected.types {
        let found = report
            .layouts
            .iter()
            .find(|candidate| candidate.name == layout.name)
            .ok_or_else(|| format!("missing native layout '{}'", layout.name))?;
        if found.size != layout.size || found.alignment != layout.alignment {
            return Err(format!(
                "native layout '{}' size/alignment mismatch: expected {}/{}, found {}/{}",
                layout.name, layout.size, layout.alignment, found.size, found.alignment
            ));
        }
        for field in &layout.fields {
            let actual = found
                .fields
                .iter()
                .find(|candidate| candidate.name == field.name)
                .ok_or_else(|| format!("missing native field '{}.{}'", layout.name, field.name))?;
            if actual.type_name != field.type_name {
                return Err(format!(
                    "opaque type mismatch at '{}.{}': expected {}, found {}",
                    layout.name, field.name, field.type_name, actual.type_name
                ));
            }
            if actual.offset != field.offset || actual.size != field.size {
                return Err(format!(
                    "native field '{}.{}' layout mismatch: expected offset/size {}/{}, found {}/{}",
                    layout.name,
                    field.name,
                    field.offset,
                    field.size,
                    actual.offset,
                    actual.size
                ));
            }
        }
        if found.fields.len() != layout.fields.len() {
            return Err(format!(
                "native layout '{}' field count mismatch: expected {}, found {}",
                layout.name,
                layout.fields.len(),
                found.fields.len()
            ));
        }
    }
    if expected.types.len() != report.layouts.len() {
        return Err(format!(
            "native layout count mismatch: expected {}, found {}",
            expected.types.len(),
            report.layouts.len()
        ));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn verify_plugin(path: &str) -> Result<NativeVerificationReport, String> {
    let mut gc = GcHeap::new();
    let (functions, _library, manifest) = load_plugin(path, &mut gc)?;
    let mut probed = false;
    for (_, function) in &functions {
        if function.arity != 0
            || !function.deterministic
            || !function.effects.canonical_strings().is_empty()
        {
            continue;
        }
        probed = true;
        let first = invoke_native(function, &[], &mut gc)?;
        let second = invoke_native(function, &[], &mut gc)?;
        let first = crate::replay::encode_value(&first)?;
        let second = crate::replay::encode_value(&second)?;
        if first != second {
            return Err(format!(
                "determinism probe failed: native export {}() returned different results for identical input and generation",
                function.name
            ));
        }
    }

    let exports = manifest
        .exported_functions()
        .iter()
        .map(|export| {
            let effects = export.effects().canonical_strings();
            NativeVerificationExport {
                name: export.name().to_string(),
                signature: export.signature().to_string(),
                arity: export.arity(),
                effect_class: if effects.is_empty() {
                    "pure".to_string()
                } else {
                    "effectful".to_string()
                },
                effects,
                deterministic: export.deterministic(),
                replayable: export.replayable(),
            }
        })
        .collect::<Vec<_>>();
    let opaque_identity = manifest
        .abi_contract()
        .types
        .iter()
        .flat_map(|layout| &layout.fields)
        .any(|field| {
            !matches!(
                field.type_name.as_str(),
                "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "f32" | "f64"
            )
        });
    Ok(NativeVerificationReport {
        plugin: path.to_string(),
        extension_id: manifest.extension_id().to_string(),
        extension_version: manifest.extension_version().unwrap_or_default().to_string(),
        abi_version: manifest.abi_version(),
        content_digest: manifest.content_digest().to_string(),
        manifest_digest: manifest.digest().to_string(),
        target: manifest.target().to_string(),
        calling_convention: manifest.abi_contract().calling_convention.clone(),
        exports,
        layouts: manifest.abi_contract().types.clone(),
        layout_agreement: true,
        opaque_identity,
        determinism_probe: probed,
        replay_support: manifest
            .exported_functions()
            .iter()
            .all(NativeExportManifest::replayable),
    })
}

#[cfg(target_arch = "wasm32")]
pub fn verify_plugin(_path: &str) -> Result<NativeVerificationReport, String> {
    Err("native extension verification is not supported on wasm32".to_string())
}
