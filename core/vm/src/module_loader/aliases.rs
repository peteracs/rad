fn alias_pass(ctx: &mut LoadContext) -> Result<(), ()> {
    let mut alias_targets = HashMap::<String, PathBuf>::new();
    let mut paths: Vec<_> = ctx.parsed_files.keys().cloned().collect();
    paths.sort();
    let mut has_error = false;
    let mut bare_targets = HashSet::new();
    for path in &paths {
        let Some(parsed) = ctx.parsed_files.get(path) else {
            continue;
        };
        for decl in &parsed.decls {
            let Decl::Use(import) = decl else {
                continue;
            };
            if import.alias.is_none() {
                if let Ok(child) = resolve_module_path(&parsed.path, &import.path, ctx) {
                    bare_targets.insert(child);
                }
            }
        }
    }
    for path in &paths {
        let parsed = match ctx.parsed_files.get(path) {
            Some(p) => p.clone(),
            None => continue,
        };

        for decl in &parsed.decls {
            if let Decl::Use(u) = decl {
                if let Some(alias) = &u.alias {
                    let child = match resolve_module_path(&parsed.path, &u.path, ctx) {
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
                    }
                }
            }
        }
    }
    for (alias, child) in alias_targets {
        let Some(child_parsed) = ctx.parsed_files.get(&child) else {
            continue;
        };
        let declarations = child_parsed
            .decls
            .iter()
            .filter(|declaration| !matches!(declaration, Decl::Use(_)))
            .cloned()
            .collect();
        let binding = if bare_targets.contains(&child) {
            ModuleAlias::flattened(declarations, child_parsed.module_identity.clone())
        } else {
            ModuleAlias::namespaced(declarations, child_parsed.module_identity.clone())
        };
        ctx.aliases.insert(alias, binding);
    }
    if has_error {
        Err(())
    } else {
        Ok(())
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
