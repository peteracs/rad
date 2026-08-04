

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
    load_program_with_resolved_overrides(
        entry_path,
        parser_options,
        file_overrides,
        &HashMap::new(),
        false,
    )
}

/// Rebuild the original module graph from authenticated sources embedded in
/// a replay trace. Import targets come from the bundle, so replay does not
/// need to rediscover or fetch source files.
pub fn load_program_from_source_bundle(
    source: &str,
    layout: &SourceLayout,
    parser_options: ParserOptions,
) -> Result<LoadResult, Vec<ModuleLoadError>> {
    let bundle = layout.files(source).map_err(|message| {
        vec![ModuleLoadError {
            filepath: "<source bundle>".to_string(),
            source: source.to_string(),
            message,
            line: 1,
            col: 1,
        }]
    })?;
    load_program_with_resolved_overrides(
        &bundle.entry.to_string_lossy(),
        parser_options,
        &bundle.files,
        &bundle.imports,
        true,
    )
}

fn load_program_with_resolved_overrides(
    entry_path: &str,
    parser_options: ParserOptions,
    file_overrides: &HashMap<PathBuf, String>,
    resolved_imports: &HashMap<(PathBuf, String), PathBuf>,
    hermetic: bool,
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
    if !hermetic {
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
    }

    #[cfg(not(target_arch = "wasm32"))]
    let lock_path = entry_dir.join("forge.lock");
    #[cfg(not(target_arch = "wasm32"))]
    let lockfile = if hermetic {
        None
    } else {
        match fs::read_to_string(&lock_path) {
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
                        resolved_imports: resolved_imports.clone(),
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
        }
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
        resolved_imports: resolved_imports.clone(),
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
    resolved_imports: HashMap<(PathBuf, String), PathBuf>,
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

/// Resolve a `use` path from its importing source unit. Authenticated replay
/// bundles provide the exact target; ordinary loads fall back to filesystem
/// or remote resolution.
fn resolve_module_path(
    importer: &Path,
    use_path: &str,
    ctx: &LoadContext,
) -> Result<PathBuf, String> {
    if let Some(target) = ctx
        .resolved_imports
        .get(&(importer.to_path_buf(), use_path.to_string()))
    {
        return Ok(target.clone());
    }
    if is_remote_url(use_path) {
        fetch_remote_module_to_cache(use_path, ctx)
    } else {
        Ok(normalize_path(
            &importer.parent().unwrap_or(Path::new(".")).join(use_path),
        ))
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
    ctx.source_layout
        .push(ctx.merged_source.len(), lock_path_label.to_string());
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

    let section_index = ctx.source_layout.sections.len().saturating_sub(1);
    for decl in &program.declarations {
        if let Decl::Use(u) = decl {
            ctx.had_imports = true;
            let child = match resolve_module_path(path, &u.path, ctx) {
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
            if !is_remote_url(&u.path)
                && !child.exists()
                && !ctx.file_overrides.contains_key(&child)
            {
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
            if let Err(message) =
                ctx.source_layout
                    .add_import(section_index, u.path.clone(), child_label.clone())
            {
                ctx.errors.push(ModuleLoadError {
                    filepath: path.to_string_lossy().to_string(),
                    source: source.clone(),
                    message,
                    line: u.span.line,
                    col: u.span.col,
                });
                continue;
            }
            let _ = parse_pass(&child, ctx, &child_label);
        }
    }
    Ok(())
}

fn merge_pass(entry_path: &Path, ctx: &mut LoadContext) -> Result<(), ()> {
    let mut bare_targets = HashSet::new();
    bare_targets.insert(entry_path.to_path_buf());

    for parsed in ctx.parsed_files.values() {
        for decl in &parsed.decls {
            if let Decl::Use(u) = decl {
                if u.alias.is_none() {
                    match resolve_module_path(&parsed.path, &u.path, ctx) {
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

        let mut has_error = false;

        for decl in &parsed.decls {
            if let Decl::Use(u) = decl {
                match resolve_module_path(path, &u.path, ctx) {
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