use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::lexer::Lexer;
use crate::manifest::parse_rad_toml;
#[cfg(not(target_arch = "wasm32"))]
use crate::manifest::RadManifest;
use crate::parser::{Parser, ParserOptions};
use crate::source_bundle::SourceLayout;

#[derive(Debug, Clone)]
pub struct ModuleLoadError {
    pub filepath: String,
    pub source: String,
    pub message: String,
    pub line: u32,
    pub col: u32,
}

pub struct LoadResult {
    pub program: Program,
    pub merged_source: String,
    pub source_layout: SourceLayout,
    pub had_imports: bool,
    pub source_map: SourceMap,
    pub module_fingerprints: Vec<ModuleFingerprint>,
    pub aliases: HashMap<String, Vec<Decl>>,
    pub errors: Vec<ModuleLoadError>,
}

#[derive(Debug, Clone)]
pub struct ModuleFingerprint {
    pub path: String,
    pub bytes: usize,
    pub checksum: u64,
    /// SHA-256 of raw module bytes (lowercase hex), for forge.lock and remote pins.
    pub sha256_hex: Option<String>,
}

const LOCKFILE_FORMAT_VERSION: &str = "1";

#[derive(Debug, Clone)]
pub struct LockFile {
    pub version: String,
    pub modules: Vec<ModuleFingerprint>,
    pub checksum: u64,
}
// Lexical sections preserve one private semantic namespace.
include!("module_loader/lockfile_and_loading.rs");
include!("module_loader/aliases_and_tests.rs");
