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

fn escape_lock_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

fn lockfile_checksum_for_sorted_modules(modules: &[ModuleFingerprint]) -> u64 {
    let mut body = String::new();
    for m in modules {
        body.push('"');
        body.push_str(&escape_lock_path(&m.path));
        body.push('"');
        body.push(' ');
        body.push_str(&m.bytes.to_string());
        body.push(' ');
        body.push_str(&m.checksum.to_string());
        if let Some(ref h) = m.sha256_hex {
            body.push(' ');
            body.push_str(h);
        }
        body.push('\n');
    }
    fnv1a64(body.as_bytes())
}

fn parse_quoted_path_prefix(line: &str) -> Result<(String, &str), String> {
    let mut chars = line.chars();
    match chars.next() {
        Some('"') => {}
        _ => return Err("module line must start with a double-quoted path".to_string()),
    }
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c == '"' {
            return Ok((out, chars.as_str()));
        }
        if c == '\\' {
            let esc = chars
                .next()
                .ok_or_else(|| "unterminated escape in quoted path".to_string())?;
            match esc {
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => {
                    return Err(format!("invalid escape sequence in path: \\{other}"));
                }
            }
        } else {
            out.push(c);
        }
    }
    Err("unterminated quoted path".to_string())
}

fn parse_module_line(line: &str) -> Result<ModuleFingerprint, String> {
    let line = line.trim();
    let (path, rest) = parse_quoted_path_prefix(line)?;
    let rest = rest.trim_start();
    let mut it = rest.split_whitespace();
    let bytes_s = it
        .next()
        .ok_or_else(|| format!("missing byte count after path: {line}"))?;
    let checksum_s = it
        .next()
        .ok_or_else(|| format!("missing checksum after path: {line}"))?;
    let bytes: usize = bytes_s
        .parse()
        .map_err(|_| format!("invalid byte count: {bytes_s}"))?;
    let checksum: u64 = checksum_s
        .parse()
        .map_err(|_| format!("invalid module checksum: {checksum_s}"))?;
    let sha256_hex = match it.next() {
        None => None,
        Some(h) => {
            if it.next().is_some() {
                return Err(format!("trailing tokens on module line: {line}"));
            }
            let h = h.trim();
            if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "invalid sha256 on module line (expected 64 hex chars): {line}"
                ));
            }
            Some(h.to_ascii_lowercase())
        }
    };
    Ok(ModuleFingerprint {
        path,
        bytes,
        checksum,
        sha256_hex,
    })
}

impl LockFile {
    pub fn generate(fingerprints: &[ModuleFingerprint]) -> Self {
        let mut modules: Vec<ModuleFingerprint> = fingerprints.to_vec();
        modules.sort_by(|a, b| a.path.cmp(&b.path));
        let checksum = lockfile_checksum_for_sorted_modules(&modules);
        Self {
            version: LOCKFILE_FORMAT_VERSION.to_string(),
            modules,
            checksum,
        }
    }

    pub fn serialize(&self) -> String {
        let mut modules: Vec<ModuleFingerprint> = self.modules.clone();
        modules.sort_by(|a, b| a.path.cmp(&b.path));
        let checksum = lockfile_checksum_for_sorted_modules(&modules);
        let mut s = String::new();
        s.push_str("rad-lock ");
        s.push_str(&self.version);
        s.push('\n');
        s.push_str("checksum ");
        s.push_str(&checksum.to_string());
        s.push_str("\n\n");
        for m in &modules {
            s.push('"');
            s.push_str(&escape_lock_path(&m.path));
            s.push('"');
            s.push(' ');
            s.push_str(&m.bytes.to_string());
            s.push(' ');
            s.push_str(&m.checksum.to_string());
            if let Some(ref h) = m.sha256_hex {
                s.push(' ');
                s.push_str(h);
            }
            s.push('\n');
        }
        s
    }

    pub fn parse(content: &str) -> Result<Self, String> {
        let mut lines = content.lines().map(str::trim);
        let line_iter = lines.by_ref();

        let first = loop {
            match line_iter.next() {
                None => return Err("empty lockfile".to_string()),
                Some("") => continue,
                Some(l) => break l,
            }
        };

        let (label, version_raw) = first
            .split_once(' ')
            .ok_or_else(|| format!("expected 'rad-lock VERSION' header, got: {first}"))?;
        if label != "rad-lock" {
            return Err(format!("expected rad-lock header, got: {first}"));
        }
        let version = version_raw.to_string();

        let second = loop {
            match line_iter.next() {
                None => return Err("missing checksum line".to_string()),
                Some("") => continue,
                Some(l) => break l,
            }
        };

        let mut chk_parts = second.split_whitespace();
        let chk_label = chk_parts
            .next()
            .ok_or_else(|| format!("invalid checksum line: {second}"))?;
        if chk_label != "checksum" {
            return Err(format!("expected 'checksum <u64>' line, got: {second}"));
        }
        let checksum_s = chk_parts
            .next()
            .ok_or_else(|| format!("missing checksum value: {second}"))?;
        if chk_parts.next().is_some() {
            return Err(format!("trailing tokens on checksum line: {second}"));
        }
        let checksum: u64 = checksum_s
            .parse()
            .map_err(|_| format!("invalid lockfile checksum: {checksum_s}"))?;

        let mut modules = Vec::new();
        let mut seen = HashSet::new();
        for line in line_iter {
            if line.is_empty() {
                continue;
            }
            let fp = parse_module_line(line)?;
            if !seen.insert(fp.path.clone()) {
                return Err(format!("duplicate module path in lockfile: {}", fp.path));
            }
            modules.push(fp);
        }

        modules.sort_by(|a, b| a.path.cmp(&b.path));
        let expected = lockfile_checksum_for_sorted_modules(&modules);
        if expected != checksum {
            return Err(format!(
                "lockfile checksum mismatch: file declares {checksum}, recomputed {expected}"
            ));
        }

        Ok(LockFile {
            version,
            modules,
            checksum,
        })
    }

    pub fn verify(&self, current: &[ModuleFingerprint]) -> Result<(), Vec<String>> {
        let lock_by_path: HashMap<&str, &ModuleFingerprint> =
            self.modules.iter().map(|m| (m.path.as_str(), m)).collect();
        let current_by_path: HashMap<&str, &ModuleFingerprint> =
            current.iter().map(|m| (m.path.as_str(), m)).collect();

        let mut paths: Vec<&str> = lock_by_path
            .keys()
            .chain(current_by_path.keys())
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        paths.sort();

        let mut mismatches = Vec::new();
        for path in paths {
            match (lock_by_path.get(path), current_by_path.get(path)) {
                (None, Some(_)) => {
                    mismatches.push(format!("unexpected module (not in lock): {path}"));
                }
                (Some(_), None) => {
                    mismatches.push(format!("missing module (in lock, not loaded): {path}"));
                }
                (Some(l), Some(c)) => {
                    if l.bytes != c.bytes || l.checksum != c.checksum {
                        mismatches.push(format!(
                            "module `{path}` mismatch: lock bytes={} checksum={}, current bytes={} checksum={}",
                            l.bytes, l.checksum, c.bytes, c.checksum
                        ));
                    }
                    if (l.sha256_hex.is_some() || c.sha256_hex.is_some())
                        && l.sha256_hex.as_ref() != c.sha256_hex.as_ref()
                    {
                        mismatches.push(format!(
                            "module `{path}` sha256 mismatch: lock={:?} current={:?}",
                            l.sha256_hex, c.sha256_hex
                        ));
                    }
                }
                (None, None) => unreachable!(),
            }
        }

        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(mismatches)
        }
    }
}

pub fn load_lockfile(path: &str) -> Result<LockFile, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("failed to read lockfile {path}: {e}"))?;
    LockFile::parse(&content)
}

pub fn write_lockfile(path: &str, lock: &LockFile) -> Result<(), String> {
    fs::write(path, lock.serialize()).map_err(|e| format!("failed to write lockfile {path}: {e}"))
}

#[derive(Debug, Clone)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<PackageDep>,
}

#[derive(Debug, Clone)]
pub struct PackageDep {
    pub name: String,
    pub version: String,
    pub registry: Option<String>,
}

pub fn load_program_with_uses(
    entry_path: &str,
) -> Result<(Program, String, bool), Vec<ModuleLoadError>> {
    let result = load_program_with_source_map(entry_path)?;
    if !result.errors.is_empty() {
        return Err(result.errors);
    }
    Ok((result.program, result.merged_source, result.had_imports))
}

pub fn load_program_with_source_map(entry_path: &str) -> Result<LoadResult, Vec<ModuleLoadError>> {
    load_program_with_source_map_and_options(entry_path, ParserOptions::default())
}

pub fn load_program_with_source_map_and_options(
    entry_path: &str,
    parser_options: ParserOptions,
) -> Result<LoadResult, Vec<ModuleLoadError>> {
    load_program_with_overrides(entry_path, parser_options, &HashMap::new())
}

pub fn load_program_with_overrides(
    entry_path: &str,
    parser_options: ParserOptions,
    file_overrides: &HashMap<PathBuf, String>,
) -> Result<LoadResult, Vec<ModuleLoadError>> {
    let entry = normalize_path(Path::new(entry_path));
    let entry_dir = entry
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let rad_toml_path = entry_dir.join("rad.toml");
    #[cfg(not(target_arch = "wasm32"))]
    let mut manifest = RadManifest::default();
    let mut manifest_errors: Vec<ModuleLoadError> = Vec::new();
    if let Ok(content) = fs::read_to_string(&rad_toml_path) {
        match parse_rad_toml(&content) {
            #[cfg(not(target_arch = "wasm32"))]
            Ok(m) => manifest = m,
            #[cfg(target_arch = "wasm32")]
            Ok(_) => {}
            Err(e) => {
                manifest_errors.push(ModuleLoadError {
                    filepath: rad_toml_path.to_string_lossy().to_string(),
                    source: String::new(),
                    message: format!("invalid rad.toml: {e}"),
                    line: 1,
                    col: 1,
                });
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    let lock_path = entry_dir.join("forge.lock");
    #[cfg(not(target_arch = "wasm32"))]
    let lockfile = match fs::read_to_string(&lock_path) {
        Ok(content) => match LockFile::parse(&content) {
            Ok(l) => Some(l),
            Err(e) => {
                let ctx = LoadContext {
                    visited: HashSet::new(),
                    parsed_files: HashMap::new(),
                    symbols: HashMap::new(),
                    merged: Vec::new(),
                    merged_source: String::new(),
                    source_layout: SourceLayout::default(),
                    had_imports: false,
                    source_map: SourceMap::new(),
                    parser_options,
                    module_fingerprints: Vec::new(),
                    aliases: HashMap::new(),
                    file_overrides: file_overrides.clone(),
                    errors: vec![ModuleLoadError {
                        filepath: lock_path.to_string_lossy().to_string(),
                        source: String::new(),
                        message: format!("invalid forge.lock: {e}"),
                        line: 1,
                        col: 1,
                    }],
                    #[cfg(not(target_arch = "wasm32"))]
                    lockfile: None,
                    #[cfg(not(target_arch = "wasm32"))]
                    manifest: manifest.clone(),
                };
                let mut errs = ctx.errors;
                errs.extend(manifest_errors);
                return Ok(LoadResult {
                    program: Program {
                        declarations: ctx.merged,
                    },
                    merged_source: ctx.merged_source,
                    source_layout: ctx.source_layout,
                    had_imports: ctx.had_imports,
                    source_map: ctx.source_map,
                    module_fingerprints: ctx.module_fingerprints,
                    aliases: ctx.aliases,
                    errors: errs,
                });
            }
        },
        Err(_) => None,
    };

    let mut ctx = LoadContext {
        visited: HashSet::new(),
        parsed_files: HashMap::new(),
        symbols: HashMap::new(),
        merged: Vec::new(),
        merged_source: String::new(),
        source_layout: SourceLayout::default(),
        had_imports: false,
        source_map: SourceMap::new(),
        parser_options,
        module_fingerprints: Vec::new(),
        aliases: HashMap::new(),
        file_overrides: file_overrides.clone(),
        errors: Vec::new(),
        #[cfg(not(target_arch = "wasm32"))]
        lockfile,
        #[cfg(not(target_arch = "wasm32"))]
        manifest: manifest.clone(),
    };
    ctx.errors.extend(manifest_errors);

    let _ = parse_pass(&entry, &mut ctx, &entry.to_string_lossy());

    let _ = merge_pass(&entry, &mut ctx);
    let _ = alias_pass(&mut ctx);

    Ok(LoadResult {
        program: Program {
            declarations: ctx.merged,
        },
        merged_source: ctx.merged_source,
        source_layout: ctx.source_layout,
        had_imports: ctx.had_imports,
        source_map: ctx.source_map,
        module_fingerprints: ctx.module_fingerprints,
        aliases: ctx.aliases,
        errors: ctx.errors,
    })
}

#[derive(Clone)]
struct ParsedFile {
    path: PathBuf,
    source: String,
    decls: Vec<Decl>,
}

struct LoadContext {
    visited: HashSet<PathBuf>,
    parsed_files: HashMap<PathBuf, ParsedFile>,
    symbols: HashMap<String, SymbolDef>,
    merged: Vec<Decl>,
    merged_source: String,
    source_layout: SourceLayout,
    had_imports: bool,
    source_map: SourceMap,
    parser_options: ParserOptions,
    module_fingerprints: Vec<ModuleFingerprint>,
    aliases: HashMap<String, Vec<Decl>>,
    file_overrides: HashMap<PathBuf, String>,
    errors: Vec<ModuleLoadError>,
    /// Parsed `forge.lock` next to the entry, if present and valid.
    #[cfg(not(target_arch = "wasm32"))]
    lockfile: Option<LockFile>,
    /// Workspace `rad.toml` `[network]` policy (defaults when missing).
    #[cfg(not(target_arch = "wasm32"))]
    manifest: RadManifest,
}

struct SymbolDef {
    file: String,
    line: u32,
}

fn modules_debug_enabled() -> bool {
    matches!(
        env::var("RAD_DEBUG_MODULES").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
    )
}

fn modules_debug(msg: &str) {
    if modules_debug_enabled() {
        eprintln!("[module_loader] {msg}");
    }
}

fn sha256_hex_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn is_remote_url(p: &str) -> bool {
    p.starts_with("http://") || p.starts_with("https://")
}

#[cfg(not(target_arch = "wasm32"))]
fn rad_user_cache_dir() -> Result<PathBuf, String> {
    let base = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| "HOME/USERPROFILE not set; cannot cache remote modules".to_string())?;
    let dir = PathBuf::from(base).join(".rad").join("cache");
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create cache dir: {e}"))?;
    Ok(dir)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_remote_module_to_cache(url: &str, ctx: &LoadContext) -> Result<PathBuf, String> {
    let expected_hex: Option<&str> = match &ctx.lockfile {
        None => None,
        Some(lock) => {
            let m = lock.modules.iter().find(|m| m.path == url);
            match m {
                None => {
                    return Err(format!(
                        "remote module '{url}' is not listed in forge.lock (unlocked dependency)"
                    ));
                }
                Some(fp) => match &fp.sha256_hex {
                    None => {
                        return Err(format!(
                            "remote module '{url}' has no sha256 pin in forge.lock; run `rad <entry.rad> --write-lock` after dependencies resolve"
                        ));
                    }
                    Some(h) => Some(h.as_str()),
                },
            }
        }
    };

    let max_bytes = ctx.manifest.max_remote_module_bytes;
    let config = ureq::config::Config::builder()
        .timeout_global(Some(ctx.manifest.fetch_timeout))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("failed to fetch remote module '{url}': {e}"))?;
    let mut body = resp.into_body();
    let mut reader = body.as_reader();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("failed to read body from '{url}': {e}"))?;
        if n == 0 {
            break;
        }
        if buf.len() + n > max_bytes {
            return Err(format!(
                "remote module body from '{url}' exceeds {} bytes (see rad.toml [network].max_module_size)",
                max_bytes
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    let got_hex = sha256_hex_of(&buf);
    if let Some(exp) = expected_hex {
        if got_hex != exp.to_ascii_lowercase() {
            return Err(format!(
                "SHA-256 mismatch for remote module '{url}': lockfile pin does not match downloaded bytes (supply chain security violation)"
            ));
        }
    } else {
        modules_debug(&format!(
            "fetched remote '{url}' without forge.lock sha256 pin (not recommended for CI)"
        ));
    }

    let cache_dir = rad_user_cache_dir()?;
    let path = cache_dir.join(format!("{got_hex}.rad"));
    match fs::read(&path) {
        Ok(existing) if existing == buf => {}
        _ => {
            fs::write(&path, &buf)
                .map_err(|e| format!("failed to write cache file {}: {e}", path.display()))?;
        }
    }
    Ok(path)
}

#[cfg(target_arch = "wasm32")]
fn fetch_remote_module_to_cache(url: &str, _ctx: &LoadContext) -> Result<PathBuf, String> {
    Err(format!(
        "remote module import '{url}' is not supported on wasm32"
    ))
}

/// Resolve a `use` path: local file relative to `parent`, or HTTP(S) URL fetched into `~/.rad/cache/`.
fn resolve_module_path(
    parent: &Path,
    use_path: &str,
    ctx: &LoadContext,
) -> Result<PathBuf, String> {
    if is_remote_url(use_path) {
        fetch_remote_module_to_cache(use_path, ctx)
    } else {
        Ok(normalize_path(&parent.join(use_path)))
    }
}

fn parse_pass(path: &Path, ctx: &mut LoadContext, lock_path_label: &str) -> Result<(), ()> {
    modules_debug(&format!("enter parse_pass {}", path.to_string_lossy()));
    if ctx.visited.contains(path) {
        return Ok(());
    }
    ctx.visited.insert(path.to_path_buf());

    let source = if let Some(overridden) = ctx.file_overrides.get(path) {
        overridden.clone()
    } else {
        match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                ctx.errors.push(ModuleLoadError {
                    filepath: path.to_string_lossy().to_string(),
                    source: String::new(),
                    message: format!("Error reading '{}': {}", path.to_string_lossy(), e),
                    line: 1,
                    col: 1,
                });
                return Err(());
            }
        }
    };

    let file_id = ctx
        .source_map
        .add_file(path.to_string_lossy().to_string(), source.clone());
    ctx.module_fingerprints.push(ModuleFingerprint {
        path: lock_path_label.to_string(),
        bytes: source.len(),
        checksum: fnv1a64(source.as_bytes()),
        sha256_hex: Some(sha256_hex_of(source.as_bytes())),
    });

    if !ctx.merged_source.is_empty() {
        ctx.merged_source.push('\n');
    }
    ctx.source_layout.push(
        ctx.merged_source.len(),
        path.to_string_lossy().to_string(),
    );
    let rendered_source = normalize_file_source_for_merge(&source);
    ctx.merged_source.push_str(&rendered_source);

    let mut lexer = Lexer::new(&source);
    let (tokens, lex_errors) = lexer.tokenize();
    for e in lex_errors {
        ctx.errors.push(ModuleLoadError {
            filepath: path.to_string_lossy().to_string(),
            source: source.clone(),
            message: e.message,
            line: e.line,
            col: e.col,
        });
    }

    let mut parser = Parser::new(tokens)
        .with_options(ctx.parser_options)
        .with_file_id(file_id);
    let program = parser.parse();

    for err in parser.errors() {
        ctx.errors.push(ModuleLoadError {
            filepath: path.to_string_lossy().to_string(),
            source: source.clone(),
            message: err.message.clone(),
            line: err.line,
            col: err.col,
        });
    }

    ctx.parsed_files.insert(
        path.to_path_buf(),
        ParsedFile {
            path: path.to_path_buf(),
            source: source.clone(),
            decls: program.declarations.clone(),
        },
    );

    let parent = path.parent().unwrap_or(Path::new("."));
    for decl in &program.declarations {
        if let Decl::Use(u) = decl {
            ctx.had_imports = true;
            let child = match resolve_module_path(parent, &u.path, ctx) {
                Ok(p) => p,
                Err(msg) => {
                    ctx.errors.push(ModuleLoadError {
                        filepath: path.to_string_lossy().to_string(),
                        source: source.clone(),
                        message: msg,
                        line: u.span.line,
                        col: u.span.col,
                    });
                    continue;
                }
            };
            if !is_remote_url(&u.path) && !child.exists() {
                ctx.errors.push(ModuleLoadError {
                    filepath: path.to_string_lossy().to_string(),
                    source: source.clone(),
                    message: format!("Module not found: {}", u.path),
                    line: u.span.line,
                    col: u.span.col,
                });
                continue;
            }
            let child_label = if is_remote_url(&u.path) {
                u.path.clone()
            } else {
                child.to_string_lossy().into_owned()
            };
            let _ = parse_pass(&child, ctx, &child_label);
        }
    }
    Ok(())
}

fn merge_pass(entry_path: &Path, ctx: &mut LoadContext) -> Result<(), ()> {
    let mut bare_targets = HashSet::new();
    bare_targets.insert(entry_path.to_path_buf());

    for parsed in ctx.parsed_files.values() {
        let parent = parsed.path.parent().unwrap_or(Path::new("."));
        for decl in &parsed.decls {
            if let Decl::Use(u) = decl {
                if u.alias.is_none() {
                    match resolve_module_path(parent, &u.path, ctx) {
                        Ok(child) => {
                            bare_targets.insert(child);
                        }
                        Err(msg) => {
                            ctx.errors.push(ModuleLoadError {
                                filepath: parsed.path.to_string_lossy().to_string(),
                                source: parsed.source.clone(),
                                message: format!(
                                    "Internal compiler error: module missing during merge pass (cache corrupted?): {msg}"
                                ),
                                line: u.span.line,
                                col: u.span.col,
                            });
                        }
                    }
                }
            }
        }
    }

    let mut visited = HashSet::new();

    fn dfs(
        path: &Path,
        ctx: &mut LoadContext,
        bare_targets: &HashSet<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        entry_path: &Path,
    ) -> Result<(), ()> {
        if visited.contains(path) {
            return Ok(());
        }
        visited.insert(path.to_path_buf());

        let parsed = match ctx.parsed_files.get(path) {
            Some(p) => p.clone(),
            None => return Err(()),
        };
        let parent = path.parent().unwrap_or(Path::new("."));

        let mut has_error = false;

        for decl in &parsed.decls {
            if let Decl::Use(u) = decl {
                match resolve_module_path(parent, &u.path, ctx) {
                    Ok(child) => {
                        if dfs(&child, ctx, bare_targets, visited, entry_path).is_err() {
                            has_error = true;
                        }
                    }
                    Err(msg) => {
                        ctx.errors.push(ModuleLoadError {
                            filepath: path.to_string_lossy().to_string(),
                            source: parsed.source.clone(),
                            message: format!(
                                "Internal compiler error: module missing during merge pass (cache corrupted?): {msg}"
                            ),
                            line: u.span.line,
                            col: u.span.col,
                        });
                        has_error = true;
                    }
                }
            }
        }

        if bare_targets.contains(path) {
            for decl in &parsed.decls {
                if let Decl::Use(_) = decl {
                    ctx.merged.push(decl.clone());
                } else {
                    let symbol = decl_symbol(decl);
                    if let Some((name, line, col)) = symbol {
                        let current_file = path.to_string_lossy().to_string();
                        let is_pub = decl_is_pub(decl);
                        let is_entry = path == entry_path;

                        if is_pub || is_entry {
                            if let Some(existing) = ctx.symbols.get(&name) {
                                ctx.errors.push(ModuleLoadError {
                                    filepath: current_file.clone(),
                                    source: parsed.source.clone(),
                                    message: format!(
                                        "Duplicate top-level declaration '{}' (already defined at {}:{})",
                                        name, existing.file, existing.line
                                    ),
                                    line,
                                    col,
                                });
                                has_error = true;
                            } else {
                                ctx.symbols.insert(
                                    name.clone(),
                                    SymbolDef {
                                        file: current_file,
                                        line,
                                    },
                                );
                            }
                        }
                        ctx.merged.push(decl.clone());
                    } else {
                        ctx.merged.push(decl.clone());
                    }
                }
            }
        } else {
            for decl in &parsed.decls {
                if let Decl::Use(_) = decl {
                    ctx.merged.push(decl.clone());
                }
            }
        }
        if has_error {
            Err(())
        } else {
            Ok(())
        }
    }

    dfs(entry_path, ctx, &bare_targets, &mut visited, entry_path)
}

fn alias_pass(ctx: &mut LoadContext) -> Result<(), ()> {
    let mut alias_targets = HashMap::new();
    let mut paths: Vec<_> = ctx.parsed_files.keys().cloned().collect();
    paths.sort();
    let mut has_error = false;

    for path in paths {
        let parsed = match ctx.parsed_files.get(&path) {
            Some(p) => p.clone(),
            None => continue,
        };
        let parent = path.parent().unwrap_or(Path::new("."));

        for decl in &parsed.decls {
            if let Decl::Use(u) = decl {
                if let Some(alias) = &u.alias {
                    let child = match resolve_module_path(parent, &u.path, ctx) {
                        Ok(p) => p,
                        Err(msg) => {
                            ctx.errors.push(ModuleLoadError {
                                filepath: path.to_string_lossy().to_string(),
                                source: parsed.source.clone(),
                                message: msg,
                                line: u.span.line,
                                col: u.span.col,
                            });
                            has_error = true;
                            continue;
                        }
                    };

                    if let Some(existing_target) = alias_targets.get(alias) {
                        if existing_target != &child {
                            ctx.errors.push(ModuleLoadError {
                                filepath: path.to_string_lossy().to_string(),
                                source: parsed.source.clone(),
                                message: format!(
                                    "Duplicate module alias '{}' points to different files",
                                    alias
                                ),
                                line: u.span.line,
                                col: u.span.col,
                            });
                            has_error = true;
                        }
                    } else {
                        if ctx.symbols.contains_key(alias) {
                            ctx.errors.push(ModuleLoadError {
                                filepath: path.to_string_lossy().to_string(),
                                source: parsed.source.clone(),
                                message: format!(
                                    "Module alias '{}' conflicts with an existing declaration",
                                    alias
                                ),
                                line: u.span.line,
                                col: u.span.col,
                            });
                            has_error = true;
                        }
                        alias_targets.insert(alias.clone(), child.clone());

                        let child_parsed = match ctx.parsed_files.get(&child) {
                            Some(p) => p,
                            None => continue,
                        };
                        let mut alias_decls = Vec::new();
                        for d in &child_parsed.decls {
                            if !matches!(d, Decl::Use(_)) {
                                alias_decls.push(d.clone());
                            }
                        }
                        ctx.aliases.insert(alias.clone(), alias_decls);
                    }
                }
            }
        }
    }
    if has_error {
        Err(())
    } else {
        Ok(())
    }
}

fn decl_is_pub(decl: &Decl) -> bool {
    match decl {
        Decl::Component(c) => c.is_pub,
        Decl::Struct(s) => s.is_pub,
        Decl::Intent(i) => i.is_pub,
        Decl::Law(l) => l.is_pub,
        Decl::Resolver(r) => r.is_pub,
        Decl::Constraint(c) => c.is_pub,
        Decl::Entity(e) => e.is_pub,
        Decl::State(s) => s.is_pub,
        Decl::System(s) => s.is_pub,
        Decl::Event(e) => e.is_pub,
        Decl::Phase(p) => p.is_pub,
        Decl::Fn(f) => f.is_pub,
        Decl::Type(t) => t.is_pub,
        Decl::TypeAlias(a) => a.is_pub,
        Decl::Stmt(Stmt::Let(l)) => l.is_pub,
        _ => false,
    }
}

fn decl_symbol(decl: &Decl) -> Option<(String, u32, u32)> {
    match decl {
        Decl::Component(c) => Some((c.name.clone(), c.span.line, c.span.col)),
        Decl::Intent(i) => Some((i.name.clone(), i.span.line, i.span.col)),
        Decl::Law(l) => Some((l.name.clone(), l.span.line, l.span.col)),
        Decl::Resolver(r) => Some((r.name.clone(), r.span.line, r.span.col)),
        Decl::Constraint(c) => Some((c.name.clone(), c.span.line, c.span.col)),
        Decl::Entity(e) => Some((e.name.clone(), e.span.line, e.span.col)),
        Decl::State(s) => Some((s.name.clone(), s.span.line, s.span.col)),
        Decl::System(s) => Some((s.name.clone(), s.span.line, s.span.col)),
        Decl::Event(e) => Some((e.name.clone(), e.span.line, e.span.col)),
        Decl::Phase(p) => Some((p.name.clone(), p.span.line, p.span.col)),
        Decl::Fn(f) => Some((f.name.clone(), f.span.line, f.span.col)),
        Decl::Type(t) => Some((t.name.clone(), t.span.line, t.span.col)),
        Decl::TypeAlias(a) => Some((a.name.clone(), a.span.line, a.span.col)),
        // Exported constants join duplicate detection; private top-level
        // lets keep their historical file-local coexistence.
        Decl::Stmt(Stmt::Let(l)) if l.is_pub => l
            .names
            .first()
            .map(|n| (n.clone(), l.span.line, l.span.col)),
        _ => None,
    }
}

fn normalize_file_source_for_merge(source: &str) -> String {
    if source.ends_with('\n') {
        source.to_string()
    } else {
        format!("{source}\n")
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(p) = fs::canonicalize(path) {
        return p;
    }
    path.to_path_buf()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::{Checker, CheckerOptions};
    use crate::compiler::Compiler;
    use crate::vm::VM;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn mk_temp_dir() -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rad_module_loader_{ts}_{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lockfile_roundtrip_preserves_sha256_pins() {
        let sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let fingerprints = vec![ModuleFingerprint {
            path: "https://example.test/lib.rad".to_string(),
            bytes: 17,
            checksum: 12345,
            sha256_hex: Some(sha.to_string()),
        }];

        let lock = LockFile::generate(&fingerprints);
        let serialized = lock.serialize();
        assert!(serialized.starts_with("rad-lock 1\nchecksum "));
        assert!(serialized.contains(sha));

        let parsed = LockFile::parse(&serialized).expect("parse lockfile");
        assert_eq!(parsed.modules[0].sha256_hex.as_deref(), Some(sha));
        parsed
            .verify(&fingerprints)
            .expect("verify current modules");
    }

    #[test]
    fn expands_relative_use_and_marks_imports() {
        let dir = mk_temp_dir();
        let sub = dir.join("mods");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("lib.rad"), "fn libf() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"mods/lib.rad\"\nfn main() -> nil { print(libf()) }\n",
        )
        .unwrap();

        let (program, _source, had_imports) =
            load_program_with_uses(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(had_imports);
        assert!(program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "libf")));
    }

    #[test]
    fn rejects_duplicate_top_level_symbols() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn same() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn same() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0]
                .message
                .contains("Duplicate top-level declaration"));
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn duplicate_error_uses_local_file_span() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn same() { return 1 }\n").unwrap();
        fs::write(
            dir.join("b.rad"),
            "fn filler() { return 0 }\npub fn same() { return 2 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(
                err_vec[0].message.contains("a.rad:1")
                    || err_vec[0].message.contains("already defined")
            );
            assert_eq!(err_vec[0].line, 2);
            assert_eq!(err_vec[0].col, 5);
            assert_eq!(
                err_vec[0].source.lines().next().unwrap_or_default(),
                "fn filler() { return 0 }"
            );
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn duplicate_in_same_file_reports_local_line() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("main.rad"),
            "fn same() { return 1 }\nfn same() { return 2 }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0]
                .message
                .contains("Duplicate top-level declaration"));
            assert!(
                err_vec[0].message.contains("main.rad:1")
                    || err_vec[0].message.contains("already defined")
            );
            assert_eq!(err_vec[0].line, 2);
            assert_eq!(err_vec[0].col, 1);
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn merged_source_has_structured_module_boundaries() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "fn a() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nfn main() -> nil { print(a()) }\n",
        )
        .unwrap();
        let result =
            load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(result.had_imports);
        assert!(!result.merged_source.contains("// -- "));
        assert_eq!(result.source_layout.sections.len(), 2);
        assert!(result
            .source_layout
            .sections
            .iter()
            .any(|section| section.name.ends_with("a.rad")));
        result.source_layout.validate(&result.merged_source).unwrap();
    }

    #[test]
    fn file_local_lines_resolve_via_source_map() {
        let dir = mk_temp_dir();
        fs::write(dir.join("empty.rad"), "").unwrap();
        fs::write(dir.join("lib.rad"), "fn helper() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"empty.rad\"\nuse \"lib.rad\"\nfn main() -> nil { print(helper()) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let helper_fn = result
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                Decl::Fn(f) if f.name == "helper" => Some(f),
                _ => None,
            })
            .expect("expected helper declaration");

        assert_eq!(
            helper_fn.span.line, 1,
            "helper should be at line 1 in its own file"
        );
        assert!(helper_fn.span.file.is_some(), "helper should have a FileId");

        let file = result
            .source_map
            .get_file(helper_fn.span.file.unwrap())
            .unwrap();
        let source_line = file.source.lines().next().unwrap_or_default();
        assert_eq!(source_line, "fn helper() { return 1 }");
    }

    #[test]
    fn merged_source_uses_single_blank_line_between_modules() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "fn a() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "fn b() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(a() + b()) }\n",
        )
        .unwrap();

        let result =
            load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(!result.merged_source.contains("\n\n\n"));
        assert_eq!(result.source_layout.sections.len(), 3);
    }

    #[test]
    fn flat_namespace_rejects_cross_kind_duplicate_names() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("main.rad"),
            "fn same() { return 1 }\ntype same { One {} }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        assert!(err.is_err());
        let err_vec = err.unwrap_err();
        assert!(err_vec[0]
            .message
            .contains("Duplicate top-level declaration 'same'"));
    }

    #[test]
    fn cyclic_imports_do_not_reprocess_files() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "use \"b.rad\"\nfn fa() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "use \"a.rad\"\nfn fb() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let (program, _merged_source, had_imports) =
            load_program_with_uses(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(had_imports);

        let fn_names = program
            .declarations
            .iter()
            .filter_map(|d| match d {
                Decl::Fn(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        let fa_count = fn_names.iter().filter(|n| **n == "fa").count();
        let fb_count = fn_names.iter().filter(|n| **n == "fb").count();
        let main_count = fn_names.iter().filter(|n| **n == "main").count();
        assert_eq!(fa_count, 1);
        assert_eq!(fb_count, 1);
        assert_eq!(main_count, 1);
    }

    #[test]
    fn declaration_lines_resolve_via_source_map() {
        let dir = mk_temp_dir();
        fs::write(dir.join("empty.rad"), "").unwrap();
        fs::write(dir.join("a.rad"), "fn a() { return 1 }").unwrap();
        fs::write(dir.join("c.rad"), "\nfn c() { return 3 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"empty.rad\"\nuse \"a.rad\"\nuse \"c.rad\"\nfn main() -> nil { print(a() + c()) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        for decl in &result.program.declarations {
            if let Decl::Fn(f) = decl {
                let file_id = f.span.file.expect("fn should have FileId");
                let file = result.source_map.get_file(file_id).unwrap();
                let line_text = file
                    .source
                    .lines()
                    .nth((f.span.line as usize).saturating_sub(1))
                    .unwrap_or_default();
                assert!(
                    line_text.contains(&format!("fn {}", f.name)),
                    "expected source line {} to contain fn {}, got '{}' in file '{}'",
                    f.span.line,
                    f.name,
                    line_text,
                    file.path
                );
            }
        }
    }

    #[test]
    fn aliased_import_keeps_decls_separate() {
        let dir = mk_temp_dir();
        fs::write(dir.join("math.rad"), "pub fn square(x) { return x * x }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"math.rad\" as math\nfn main() -> nil { print(1) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let has_square = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "square"));
        assert!(!has_square, "'square' should NOT be in the flat namespace");

        assert!(result.aliases.contains_key("math"));
        let entries = &result.aliases["math"];
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            Decl::Fn(f) => assert_eq!(f.name, "square", "Alias decls keep original names"),
            other => panic!("Expected Decl::Fn, got {:?}", std::mem::discriminant(other)),
        }
    }

    #[test]
    fn duplicate_alias_names_rejected() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn af() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn bf() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\" as m\nuse \"b.rad\" as m\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0].message.contains("Duplicate module alias"));
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn duplicate_pub_let_across_modules_rejected() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub let LIMIT = 10\n").unwrap();
        fs::write(dir.join("b.rad"), "pub let LIMIT = 99\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(LIMIT) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(
                err_vec[0]
                    .message
                    .contains("Duplicate top-level declaration 'LIMIT'"),
                "expected duplicate pub let error, got: {}",
                err_vec[0].message
            );
        } else {
            panic!("Expected duplicate pub let error, got Ok");
        }
    }

    #[test]
    fn private_top_level_lets_keep_coexisting_across_modules() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("a.rad"),
            "let scratch = 1\npub fn af() -> int { return scratch }\n",
        )
        .unwrap();
        fs::write(
            dir.join("b.rad"),
            "let scratch = 2\npub fn bf() -> int { return scratch }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(af() + bf()) }\n",
        )
        .unwrap();

        // Historical behavior: private lets never participated in duplicate
        // detection; only exported (pub) lets do.
        let result = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        assert!(result.is_ok(), "private let coexistence must keep loading");
    }

    #[test]
    fn bare_use_still_works_alongside_aliased() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn af() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn bf() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\" as m\nfn main() -> nil { print(af()) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let has_af = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "af"));
        assert!(has_af, "Bare import 'af' should be in flat namespace");

        let bf_in_flat = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "bf"));
        assert!(!bf_in_flat, "Aliased 'bf' should NOT be in flat namespace");

        assert!(result.aliases.contains_key("m"));
        let m_decls = &result.aliases["m"];
        assert_eq!(m_decls.len(), 1);
        match &m_decls[0] {
            Decl::Fn(f) => assert_eq!(f.name, "bf"),
            other => panic!("Expected Decl::Fn, got {:?}", std::mem::discriminant(other)),
        }
    }

    #[test]
    fn aliased_import_allows_same_name_in_different_modules() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn helper() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn helper() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\" as a\nuse \"b.rad\" as b\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let helper_in_flat = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "helper"));
        assert!(!helper_in_flat, "'helper' should NOT be in flat namespace");

        assert!(result.aliases.contains_key("a"));
        assert!(result.aliases.contains_key("b"));

        let a_decls = &result.aliases["a"];
        assert_eq!(a_decls.len(), 1);
        match &a_decls[0] {
            Decl::Fn(f) => assert_eq!(f.name, "helper"),
            other => panic!(
                "Expected Decl::Fn in alias 'a', got {:?}",
                std::mem::discriminant(other)
            ),
        }

        let b_decls = &result.aliases["b"];
        assert_eq!(b_decls.len(), 1);
        match &b_decls[0] {
            Decl::Fn(f) => assert_eq!(f.name, "helper"),
            other => panic!(
                "Expected Decl::Fn in alias 'b', got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn alias_conflicts_with_existing_declaration() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn af() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "fn math() { return 0 }\nuse \"a.rad\" as math\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0]
                .message
                .contains("conflicts with an existing declaration"));
        } else {
            panic!("Expected error, got Ok");
        }
    }

    /// Regression for Bug #1: `set(e, lex.Tok { ... })` and `has(e, lex.Tok)` must use the same
    /// mangled component type name (`__mod_lex__Tok`), not a mix of bare and qualified names.
    #[test]
    fn aliased_import_component_set_has_roundtrip() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("tok.rad"),
            "pub component Tok { kind: str = \"\" }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"tok.rad\" as lex\n\nfn main() -> nil {\n    let e = spawn()\n    set(e, lex.Tok { kind: \"hi\" })\n    print(has(e, lex.Tok))\n}\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(
            result.errors.is_empty(),
            "parse/load errors: {:?}",
            result.errors
        );

        let mut checker = Checker::new_with_options(CheckerOptions::default());
        checker.set_aliases(result.aliases.clone());
        let errors = checker.check(&result.program);
        assert!(
            errors.is_empty(),
            "typecheck errors: {:?}",
            errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
        );

        let compiler = Compiler::new()
            .with_checker_output(checker.output())
            .with_aliases(result.aliases);
        let compile_result = compiler.compile(&result.program).expect("compile");

        let mut vm = VM::new();
        vm.load_compile_result(compile_result);
        vm.run(0).expect("vm run");
        assert_eq!(
            vm.print_buffer,
            vec!["true"],
            "has(e, lex.Tok) must find the component set with lex.Tok literal (Bug #1 if false)"
        );
    }

    /// Bug #7: match arms must accept qualified paths like `lex.Tok.IntLit { n }`, not only bare
    /// `IntLit`. Parser + checker + codegen are exercised end-to-end.
    #[test]
    fn aliased_sum_type_match_qualified_variant_pattern() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("kinds.rad"),
            "pub type Tok {\n    IntLit { n: 0 }\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"kinds.rad\" as lex\n\nfn main() -> nil {\n    let k = lex.Tok::IntLit { n: 42 }\n    match k {\n        lex.Tok.IntLit { n } => { print(n) }\n    }\n}\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(
            result.errors.is_empty(),
            "parse/load errors: {:?}",
            result.errors
        );

        let mut checker = Checker::new_with_options(CheckerOptions::default());
        checker.set_aliases(result.aliases.clone());
        let errors = checker.check(&result.program);
        assert!(
            errors.is_empty(),
            "typecheck errors: {:?}",
            errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
        );

        let compiler = Compiler::new()
            .with_checker_output(checker.output())
            .with_aliases(result.aliases);
        let compile_result = compiler.compile(&result.program).expect("compile");

        let mut vm = VM::new();
        vm.load_compile_result(compile_result);
        vm.run(0).expect("vm run");
        assert_eq!(
            vm.print_buffer,
            vec!["42"],
            "qualified match pattern lex.Tok.IntLit should bind and run (Bug #7 if wrong)"
        );
    }
}
