//! Sealed multi-module source packages for browser `RadRuntime` sessions.
//!
//! The native module loader is the only producer of canonical source layouts.
//! Browser hosts transport this package unchanged; they never concatenate or
//! reinterpret modules themselves.

use crate::source_bundle::SourceLayout;
use serde::{Deserialize, Serialize};

pub const BROWSER_PROGRAM_FORMAT: &str = "rad-browser-program/v1";
pub const BROWSER_RUNTIME_API: u32 = 2;
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACKAGE_JSON_BYTES: usize = 96 * 1024 * 1024;
const MAX_SOURCE_UNITS: usize = 4_096;

pub fn browser_semantic_features() -> Vec<String> {
    vec!["causal_laws".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserProgramPackage {
    format: String,
    compiler_version: String,
    runtime_api: u32,
    semantic_features: Vec<String>,
    source: String,
    source_layout: SourceLayout,
    package_digest: String,
}

impl BrowserProgramPackage {
    pub fn new(
        source: String,
        source_layout: SourceLayout,
        semantic_features: Vec<String>,
    ) -> Result<Self, String> {
        validate_feature_order(&semantic_features)?;
        validate_package_limits(source.len(), source_layout.sections.len())?;
        source_layout.validate(&source)?;
        let package_digest = package_digest(&source, &source_layout, &semantic_features)?;
        Ok(Self {
            format: BROWSER_PROGRAM_FORMAT.to_string(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            runtime_api: BROWSER_RUNTIME_API,
            semantic_features,
            source,
            source_layout,
            package_digest,
        })
    }

    pub fn from_json(json: &str, expected_features: &[String]) -> Result<Self, String> {
        if json.len() > MAX_PACKAGE_JSON_BYTES {
            return Err("browser program package exceeds the 96 MiB input limit".to_string());
        }
        let package: Self = serde_json::from_str(json)
            .map_err(|error| format!("invalid browser program package: {error}"))?;
        package.validate(expected_features)?;
        Ok(package)
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self)
            .map_err(|error| format!("could not encode browser program package: {error}"))
    }

    pub fn validate(&self, expected_features: &[String]) -> Result<(), String> {
        if self.format != BROWSER_PROGRAM_FORMAT {
            return Err(format!(
                "unsupported browser program format '{}'; expected '{}'",
                self.format, BROWSER_PROGRAM_FORMAT
            ));
        }
        if self.compiler_version != env!("CARGO_PKG_VERSION") {
            return Err(format!(
                "browser package compiler version '{}' does not match runtime '{}'",
                self.compiler_version,
                env!("CARGO_PKG_VERSION")
            ));
        }
        if self.runtime_api != BROWSER_RUNTIME_API {
            return Err(format!(
                "browser package runtime API {} does not match {}",
                self.runtime_api, BROWSER_RUNTIME_API
            ));
        }
        validate_feature_order(&self.semantic_features)?;
        validate_feature_order(expected_features)?;
        if self.semantic_features != expected_features {
            return Err(format!(
                "browser package semantic features {:?} do not match runtime {:?}",
                self.semantic_features, expected_features
            ));
        }
        validate_package_limits(self.source.len(), self.source_layout.sections.len())?;
        self.source_layout.validate(&self.source)?;
        let actual = package_digest(&self.source, &self.source_layout, &self.semantic_features)?;
        if actual != self.package_digest {
            return Err(format!(
                "browser program package digest mismatch: claimed {}, computed {}",
                self.package_digest, actual
            ));
        }
        Ok(())
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn source_layout(&self) -> &SourceLayout {
        &self.source_layout
    }

    pub fn package_digest(&self) -> &str {
        &self.package_digest
    }
}

fn validate_feature_order(features: &[String]) -> Result<(), String> {
    if features.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(
            "browser package semantic features must be sorted and contain no duplicates"
                .to_string(),
        );
    }
    Ok(())
}

fn validate_package_limits(source_bytes: usize, source_units: usize) -> Result<(), String> {
    if source_bytes > MAX_SOURCE_BYTES {
        return Err("browser program source exceeds the 64 MiB limit".to_string());
    }
    if source_units == 0 {
        return Err("browser program package has no source units".to_string());
    }
    if source_units > MAX_SOURCE_UNITS {
        return Err("browser program package exceeds the 4096-unit limit".to_string());
    }
    Ok(())
}

fn package_digest(
    source: &str,
    source_layout: &SourceLayout,
    semantic_features: &[String],
) -> Result<String, String> {
    let source_digest = source_layout.digest(source)?;
    let mut digest = blake3::Hasher::new();
    digest.update(b"rad-browser-program/v1\0");
    digest.update(env!("CARGO_PKG_VERSION").as_bytes());
    digest.update(&BROWSER_RUNTIME_API.to_le_bytes());
    digest.update(&(semantic_features.len() as u64).to_le_bytes());
    for feature in semantic_features {
        digest.update(&(feature.len() as u64).to_le_bytes());
        digest.update(feature.as_bytes());
    }
    digest.update(source_digest.as_bytes());
    Ok(digest.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> BrowserProgramPackage {
        BrowserProgramPackage::new(
            "print(1)\n".to_string(),
            SourceLayout::single("main.rad"),
            browser_semantic_features(),
        )
        .unwrap()
    }

    #[test]
    fn package_round_trip_binds_source_layout_and_semantics() {
        let package = package();
        let decoded = BrowserProgramPackage::from_json(
            &package.to_json().unwrap(),
            &browser_semantic_features(),
        )
        .unwrap();
        assert_eq!(decoded.source(), "print(1)\n");
    }

    #[test]
    fn package_rejects_mutated_source_and_semantic_configuration() {
        let mut value: serde_json::Value =
            serde_json::from_str(&package().to_json().unwrap()).unwrap();
        value["source"] = serde_json::Value::String("print(2)\n".to_string());
        let error =
            BrowserProgramPackage::from_json(&value.to_string(), &browser_semantic_features())
                .unwrap_err();
        assert!(error.contains("digest mismatch"), "{error}");

        let error = package().validate(&[]).unwrap_err();
        assert!(error.contains("semantic features"), "{error}");
    }
}
