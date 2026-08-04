

impl LspBackend {
    async fn check_document(&self, uri: Url, text: String) {
        let path = if let Ok(p) = uri.to_file_path() {
            p
        } else {
            return;
        };

        if self.experimental_relations && is_relation_document(&text) {
            let options = relation_options(&path, &text);
            let diagnostics = match crate::relation_frontend::compile(&text, &options) {
                Ok(_) => Vec::new(),
                Err(errors) => errors
                    .into_iter()
                    .map(|error| Diagnostic {
                        range: Range::new(
                            Position::new(
                                error.line.saturating_sub(1),
                                error.column.saturating_sub(1),
                            ),
                            Position::new(
                                error.line.saturating_sub(1),
                                error.column.saturating_sub(1).saturating_add(1),
                            ),
                        ),
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: Some(NumberOrString::String(error.code.as_str().to_string())),
                        source: Some("rad(relations)".to_string()),
                        message: error.message,
                        ..Default::default()
                    })
                    .collect(),
            };
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
            return;
        }

        #[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
        {
            let use_wasm = std::env::var("RAD_WASM_PHASE3")
                .map(|v| v == "1")
                .unwrap_or(false)
                || std::env::var("RAD_COMPILER_WASM").is_ok();
            if use_wasm {
                if let Ok(wasm_bytes) = crate::wasm_compiler_host::compiler_wasm_bytes_from_env() {
                    let mut vfs = crate::wasm_compiler_host::VfsState {
                        fallback_dir: std::env::var("RAD_VFS_ROOT")
                            .ok()
                            .map(PathBuf::from)
                            .or_else(|| path.parent().map(PathBuf::from)),
                        ..Default::default()
                    };
                    let docs = self.documents.read().await;
                    for (u, t) in docs.iter() {
                        if let Ok(p) = u.to_file_path() {
                            vfs.insert_str(p.to_string_lossy().to_string(), t.clone());
                        }
                    }
                    vfs.insert_str(path.to_string_lossy().to_string(), text.clone());
                    drop(docs);
                    if let Ok(mut host) = crate::wasm_compiler_host::WasmCompilerHost::from_bytes(
                        &wasm_bytes,
                        Arc::new(Mutex::new(vfs)),
                    ) {
                        let _ = host.rad_init();
                        if host.rad_update_buffer(0, &text).is_ok() {
                            if let Ok(wasm_diags) = host.rad_check() {
                                let diagnostics: Vec<Diagnostic> = wasm_diags
                                    .into_iter()
                                    .map(|d| Diagnostic {
                                        range: Range::new(
                                            Position::new(
                                                d.line.saturating_sub(1),
                                                d.col.saturating_sub(1),
                                            ),
                                            Position::new(
                                                d.line.saturating_sub(1),
                                                d.col.saturating_sub(1).saturating_add(20),
                                            ),
                                        ),
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        source: Some("rad(wasm)".to_string()),
                                        message: d.message,
                                        ..Default::default()
                                    })
                                    .collect();
                                self.client
                                    .publish_diagnostics(uri, diagnostics, None)
                                    .await;
                                return;
                            }
                        }
                    }
                }
            }
        }

        let mut overrides = HashMap::new();
        overrides.insert(path.clone(), text.clone());

        // Add other open documents to overrides
        let docs = self.documents.read().await;
        for (u, t) in docs.iter() {
            if let Ok(p) = u.to_file_path() {
                if p != path {
                    overrides.insert(p, t.clone());
                }
            }
        }

        let parser_options = ParserOptions {
            compat_v0_5_dx: false,
        };

        let mut diagnostics = vec![];

        let entry_path = path.to_string_lossy().to_string();
        match load_program_with_overrides(&entry_path, parser_options, &overrides) {
            Ok(r) => {
                let mut checker = Checker::new_with_options(CheckerOptions {
                    features: vec!["causal_laws".to_string()],
                    compat_v0_5_dx: false,
                    warn_compat: true,
                    strict_types: false,
                });
                checker.set_aliases(r.aliases.clone());
                let errors = checker.check(&r.program);

                for err in errors {
                    let mut is_this_file = false;
                    if let Some(fid) = err.file {
                        if let Some(sf) = r.source_map.get_file(fid) {
                            if sf.path == entry_path {
                                is_this_file = true;
                            }
                        }
                    } else {
                        is_this_file = true; // fallback
                    }

                    if is_this_file {
                        let line = err.line.saturating_sub(1);
                        let col = err.col.saturating_sub(1);
                        let mut message = err.message.clone();
                        if let Some(hint) = err.hint {
                            message.push_str(&format!("\nhint: {}", hint));
                        }
                        diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new(line, col),
                                Position::new(line, col + 20),
                            ),
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("rad".to_string()),
                            message,
                            ..Default::default()
                        });
                    }
                }
            }
            Err(errors) => {
                for e in errors {
                    if e.filepath == entry_path {
                        let line = e.line.saturating_sub(1);
                        let col = e.col.saturating_sub(1);
                        diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new(line, col),
                                Position::new(line, col + 20),
                            ),
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("rad".to_string()),
                            message: e.message,
                            ..Default::default()
                        });
                    }
                }
            }
        }

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}