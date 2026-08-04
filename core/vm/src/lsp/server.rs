

#[tower_lsp::async_trait]
impl LanguageServer for LspBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Rad LSP server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.documents
            .write()
            .await
            .insert(uri.clone(), text.clone());
        self.check_document(uri, text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = if let Some(change) = params.content_changes.into_iter().last() {
            change.text
        } else {
            return;
        };
        self.documents
            .write()
            .await
            .insert(uri.clone(), text.clone());
        self.check_document(uri, text).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(text) = params.text {
            self.documents
                .write()
                .await
                .insert(uri.clone(), text.clone());
            self.check_document(uri, text).await;
        } else {
            let text = {
                let docs = self.documents.read().await;
                docs.get(&uri).cloned()
            };
            if let Some(text) = text {
                self.check_document(uri, text).await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.write().await.remove(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let text = {
            let docs = self.documents.read().await;
            docs.get(&uri).cloned()
        };
        let Some(text) = text else {
            return Ok(None);
        };

        let formatted = if self.experimental_relations && is_relation_document(&text) {
            let path = uri.to_file_path().ok();
            let options = path
                .as_deref()
                .map(|path| relation_options(path, &text))
                .unwrap_or_else(relation_default_options);
            match crate::relation_frontend::format_source(&text, &options) {
                Ok(formatted) => relation_module_directive(&text)
                    .map(|module| format!("// module: {module}\n{formatted}"))
                    .unwrap_or(formatted),
                Err(_) => return Ok(None),
            }
        } else {
            crate::formatter::format_rad(&text)
        };
        if formatted == text {
            return Ok(Some(vec![]));
        }

        Ok(Some(vec![TextEdit {
            range: Range::new(Position::new(0, 0), document_end_position(&text)),
            new_text: formatted,
        }]))
    }

    #[allow(deprecated)]
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        if !self.experimental_relations {
            return Ok(None);
        }
        let uri = params.text_document.uri;
        let text = {
            let documents = self.documents.read().await;
            documents.get(&uri).cloned()
        };
        let Some(text) = text else {
            return Ok(None);
        };
        if !is_relation_document(&text) {
            return Ok(None);
        }
        let path = uri.to_file_path().ok();
        let options = path
            .as_deref()
            .map(|path| relation_options(path, &text))
            .unwrap_or_else(relation_default_options);
        let Ok(symbols) = crate::relation_frontend::symbols(&text, &options) else {
            return Ok(Some(DocumentSymbolResponse::Nested(Vec::new())));
        };
        let range = Range::new(Position::new(0, 0), document_end_position(&text));
        Ok(Some(DocumentSymbolResponse::Nested(
            symbols
                .into_iter()
                .map(|symbol| DocumentSymbol {
                    name: symbol.identity,
                    detail: Some("RFC-0003 experimental front end".to_string()),
                    kind: match symbol.kind {
                        crate::relation_frontend::FrontendSymbolKind::Rule => SymbolKind::FUNCTION,
                        crate::relation_frontend::FrontendSymbolKind::AuthoritativeRelation
                        | crate::relation_frontend::FrontendSymbolKind::DerivedRelation => {
                            SymbolKind::CLASS
                        }
                    },
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                })
                .collect(),
        )))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let text = {
            let docs = self.documents.read().await;
            if let Some(t) = docs.get(&uri) {
                t.clone()
            } else {
                return Ok(None);
            }
        };

        let lines: Vec<&str> = text.lines().collect();
        if pos.line as usize >= lines.len() {
            return Ok(None);
        }
        let line_text = lines[pos.line as usize];
        let char_col = utf16_col_to_char_idx(line_text, pos.character);

        let chars: Vec<char> = line_text.chars().collect();
        let col = char_col.min(chars.len());
        let mut start = col;
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
        let mut end = col;
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        let word: String = chars[start..end].iter().collect();
        if word.is_empty() {
            return Ok(None);
        }

        let path = uri.to_file_path().unwrap_or_default();
        let entry_path = path.to_string_lossy().to_string();
        let mut overrides = HashMap::new();
        overrides.insert(path.clone(), text.clone());

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
        if let Ok(r) = load_program_with_overrides(&entry_path, parser_options, &overrides) {
            let mut checker = Checker::new_with_options(CheckerOptions {
                features: vec!["causal_laws".to_string()],
                compat_v0_5_dx: false,
                warn_compat: true,
                strict_types: false,
            });
            checker.set_aliases(r.aliases.clone());
            let _ = checker.check(&r.program);

            if let Some(path) = system_ref_path_at(line_text, char_col) {
                let q = simulate_syntax::system_ref_qualified_string(&path);
                let resolved = checker.resolve_canonical_name(&q);
                if let Some(sys) = checker.systems.get(&resolved) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Scalar(MarkedString::String(
                            format_system_hover_markdown(&resolved, sys),
                        )),
                        range: None,
                    }));
                }
            }

            if let Some(comp) = checker.components.get(&word) {
                let fs: Vec<String> = comp
                    .fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t))
                    .collect();
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\ncomponent {} {{ {} }}\n```",
                        word,
                        fs.join(", ")
                    ))),
                    range: None,
                }));
            }
            if let Some(intent) = checker.intents.get(&word) {
                let fs: Vec<String> = intent
                    .fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t))
                    .collect();
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\nintent {} {{ {} }}\n```",
                        word,
                        fs.join(", ")
                    ))),
                    range: None,
                }));
            }
            if let Some(law) = checker.laws.get(&word) {
                let params = law
                    .params
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\nlaw {}({})\n```",
                        word, params
                    ))),
                    range: None,
                }));
            }
            if let Some(constraint) = checker.constraints.get(&word) {
                let mut watches = constraint.watches.iter().cloned().collect::<Vec<_>>();
                watches.sort();
                let watches = if watches.is_empty() {
                    String::new()
                } else {
                    format!(" watches {}", watches.join(", "))
                };
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\nconstraint {} for {}(subject, proposed){}\n```",
                        word, constraint.attached_component, watches
                    ))),
                    range: None,
                }));
            }
            if let Some(sys) = checker.systems.get(&word) {
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(
                        format_system_hover_markdown(&word, sys),
                    )),
                    range: None,
                }));
            }
            if let Some(ev) = checker.events.get(&word) {
                let fs: Vec<String> = ev
                    .fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t))
                    .collect();
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\nevent {} {{ {} }}\n```",
                        word,
                        fs.join(", ")
                    ))),
                    range: None,
                }));
            }
            if let Some(sm) = checker.state_machines.get(&word) {
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\nstate {} {{ {} }}\n```",
                        word,
                        sm.states.join(", ")
                    ))),
                    range: None,
                }));
            }
            if let Some(sum) = checker.sum_types.get(&word) {
                let mut variants = vec![];
                for v in &sum.variants {
                    if v.fields.is_empty() {
                        variants.push(format!("  {} {{}}", v.name));
                    } else {
                        let fs: Vec<String> = v
                            .fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t))
                            .collect();
                        variants.push(format!("  {} {{ {} }}", v.name, fs.join(", ")));
                    }
                }
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\ntype {} {{\n{}\n}}\n```",
                        word,
                        variants.join("\n")
                    ))),
                    range: None,
                }));
            }
            if let Some(_f) = checker.functions.get(&word) {
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "```rad\nfn {}\n```",
                        word
                    ))),
                    range: None,
                }));
            }
        }

        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let text = {
            let docs = self.documents.read().await;
            if let Some(t) = docs.get(&uri) {
                t.clone()
            } else {
                return Ok(None);
            }
        };

        let lines: Vec<&str> = text.lines().collect();
        if pos.line as usize >= lines.len() {
            return Ok(None);
        }
        let line_text = lines[pos.line as usize];
        let char_col = utf16_col_to_char_idx(line_text, pos.character);

        let chars: Vec<char> = line_text.chars().collect();
        let col = char_col.min(chars.len());
        let mut start = col;
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
        let mut end = col;
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        let word: String = chars[start..end].iter().collect();
        if word.is_empty() {
            return Ok(None);
        }

        let path = uri.to_file_path().unwrap_or_default();
        let entry_path = path.to_string_lossy().to_string();
        let mut overrides = HashMap::new();
        overrides.insert(path.clone(), text.clone());

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
        if let Ok(r) = load_program_with_overrides(&entry_path, parser_options, &overrides) {
            if let Some(path) = system_ref_path_at(line_text, char_col) {
                let q = simulate_syntax::system_ref_qualified_string(&path);
                let mut checker = Checker::new_with_options(CheckerOptions {
                    features: vec!["causal_laws".to_string()],
                    compat_v0_5_dx: false,
                    warn_compat: true,
                    strict_types: false,
                });
                checker.set_aliases(r.aliases.clone());
                let _ = checker.check(&r.program);
                let resolved = checker.resolve_canonical_name(&q);
                for decl in &r.program.declarations {
                    if let crate::ast::Decl::System(s) = decl {
                        if checker.resolve_canonical_name(&s.name) == resolved {
                            let target_uri = if let Some(fid) = s.span.file {
                                if let Some(sf) = r.source_map.get_file(fid) {
                                    if let Ok(p) = std::path::PathBuf::from(&sf.path).canonicalize()
                                    {
                                        Url::from_file_path(p).unwrap_or(uri.clone())
                                    } else {
                                        uri.clone()
                                    }
                                } else {
                                    uri.clone()
                                }
                            } else {
                                uri.clone()
                            };
                            let line = s.span.line.saturating_sub(1);
                            let c = s.span.col.saturating_sub(1);
                            let n = s.name.len() as u32;
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: target_uri,
                                range: Range::new(
                                    Position::new(line, c),
                                    Position::new(line, c + n),
                                ),
                            })));
                        }
                    }
                }
            }
            for decl in &r.program.declarations {
                let (name, span) = match decl {
                    crate::ast::Decl::Component(c) => (Some(&c.name), Some(&c.span)),
                    crate::ast::Decl::Struct(s) => (Some(&s.name), Some(&s.span)),
                    crate::ast::Decl::Entity(e) => (Some(&e.name), Some(&e.span)),
                    crate::ast::Decl::State(s) => (Some(&s.name), Some(&s.span)),
                    crate::ast::Decl::System(s) => (Some(&s.name), Some(&s.span)),
                    crate::ast::Decl::Event(e) => (Some(&e.name), Some(&e.span)),
                    crate::ast::Decl::Intent(i) => (Some(&i.name), Some(&i.span)),
                    crate::ast::Decl::Law(l) => (Some(&l.name), Some(&l.span)),
                    crate::ast::Decl::Resolver(r) => (Some(&r.name), Some(&r.span)),
                    crate::ast::Decl::Constraint(c) => (Some(&c.name), Some(&c.span)),
                    crate::ast::Decl::Fn(f) => (Some(&f.name), Some(&f.span)),
                    crate::ast::Decl::Type(t) => (Some(&t.name), Some(&t.span)),
                    crate::ast::Decl::TypeAlias(a) => (Some(&a.name), Some(&a.span)),
                    _ => (None, None),
                };

                if let (Some(n), Some(s)) = (name, span) {
                    if n == &word {
                        let target_uri = if let Some(fid) = s.file {
                            if let Some(sf) = r.source_map.get_file(fid) {
                                if let Ok(p) = std::path::PathBuf::from(&sf.path).canonicalize() {
                                    Url::from_file_path(p).unwrap_or(uri.clone())
                                } else {
                                    uri.clone()
                                }
                            } else {
                                uri.clone()
                            }
                        } else {
                            uri.clone()
                        };

                        let line = s.line.saturating_sub(1);
                        let col = s.col.saturating_sub(1);
                        return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                            uri: target_uri,
                            range: Range::new(
                                Position::new(line, col),
                                Position::new(line, col + n.len() as u32),
                            ),
                        })));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let text = {
            let docs = self.documents.read().await;
            if let Some(t) = docs.get(&uri) {
                t.clone()
            } else {
                return Ok(None);
            }
        };

        let lines: Vec<&str> = text.lines().collect();
        if pos.line as usize >= lines.len() {
            return Ok(None);
        }
        let line_text = lines[pos.line as usize];
        let char_col = utf16_col_to_char_idx(line_text, pos.character);

        let chars: Vec<char> = line_text.chars().collect();
        let col = char_col.min(chars.len());
        let mut prefix_start = col;
        while prefix_start > 0
            && (chars[prefix_start - 1].is_alphanumeric() || chars[prefix_start - 1] == '_')
        {
            prefix_start -= 1;
        }
        let prefix: String = chars[prefix_start..col].iter().collect();

        let mut after_dot = false;
        let mut dot_obj = String::new();
        let mut i = col.saturating_sub(1);
        while i > 0 && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i -= 1;
        }
        if chars.get(i) == Some(&'.') {
            after_dot = true;
            let end = i;
            i = i.saturating_sub(1);
            while i > 0 && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i -= 1;
            }
            if chars
                .get(i)
                .map(|c| c.is_alphanumeric() || *c == '_')
                .unwrap_or(false)
            {
                dot_obj = chars[i..end].iter().collect();
            } else {
                dot_obj = chars[i + 1..end].iter().collect();
            }
        }

        let mut after_dcolon = false;
        let mut dcolon_obj = String::new();
        let mut i = col.saturating_sub(1);
        while i > 0 && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i -= 1;
        }
        if i > 0 && chars.get(i) == Some(&':') && chars.get(i - 1) == Some(&':') {
            after_dcolon = true;
            let end = i - 1;
            i = end.saturating_sub(1);
            while i > 0 && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i -= 1;
            }
            if chars
                .get(i)
                .map(|c| c.is_alphanumeric() || *c == '_')
                .unwrap_or(false)
            {
                dcolon_obj = chars[i..end].iter().collect();
            } else {
                dcolon_obj = chars[i + 1..end].iter().collect();
            }
        }

        let path = uri.to_file_path().unwrap_or_default();
        let entry_path = path.to_string_lossy().to_string();
        let mut overrides = HashMap::new();
        overrides.insert(path.clone(), text.clone());

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
        let mut items = vec![];

        if let Ok(r) = load_program_with_overrides(&entry_path, parser_options, &overrides) {
            let mut checker = Checker::new_with_options(CheckerOptions {
                features: vec!["causal_laws".to_string()],
                compat_v0_5_dx: false,
                warn_compat: true,
                strict_types: false,
            });
            checker.set_aliases(r.aliases.clone());
            let _ = checker.check(&r.program);

            if let Some((segs, partial)) = system_path_completion_prefix(line_text, char_col) {
                let q_dot_prefix = if segs.is_empty() {
                    String::new()
                } else {
                    format!("{}.", simulate_syntax::system_ref_qualified_string(&segs))
                };
                for (name, sys) in &checker.systems {
                    let rest = if q_dot_prefix.is_empty() {
                        name.as_str()
                    } else if let Some(r) = name.strip_prefix(&q_dot_prefix) {
                        r
                    } else {
                        continue;
                    };
                    if !rest.starts_with(&partial) {
                        continue;
                    }
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(format_system_completion_detail(sys)),
                        ..Default::default()
                    });
                }
            } else if after_dot {
                // Component/Struct fields
                if let Some(comp) = checker.components.get(&dot_obj) {
                    for (fname, ftype) in &comp.fields {
                        if fname.starts_with(&prefix) {
                            items.push(CompletionItem {
                                label: fname.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(format!("{}", ftype)),
                                ..Default::default()
                            });
                        }
                    }
                } else if let Some(strct) = checker.structs.get(&dot_obj) {
                    for (fname, ftype) in &strct.fields {
                        if fname.starts_with(&prefix) {
                            items.push(CompletionItem {
                                label: fname.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(format!("{}", ftype)),
                                ..Default::default()
                            });
                        }
                    }
                } else {
                    // Fallback: all fields
                    let mut seen = std::collections::HashSet::new();
                    for comp in checker.components.values() {
                        for (fname, ftype) in &comp.fields {
                            if fname.starts_with(&prefix) && seen.insert(fname.clone()) {
                                items.push(CompletionItem {
                                    label: fname.clone(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail: Some(format!("{}.{}", comp.name, ftype)),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                    for strct in checker.structs.values() {
                        for (fname, ftype) in &strct.fields {
                            if fname.starts_with(&prefix) && seen.insert(fname.clone()) {
                                items.push(CompletionItem {
                                    label: fname.clone(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail: Some(format!("{}.{}", strct.name, ftype)),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            } else if after_dcolon {
                // State/Type variants
                if let Some(sm) = checker.state_machines.get(&dcolon_obj) {
                    for state_name in &sm.states {
                        if state_name.starts_with(&prefix) {
                            items.push(CompletionItem {
                                label: state_name.clone(),
                                kind: Some(CompletionItemKind::ENUM_MEMBER),
                                detail: Some(format!("{} state", sm.name)),
                                ..Default::default()
                            });
                        }
                    }
                }
                if let Some(sum) = checker.sum_types.get(&dcolon_obj) {
                    for vtype in &sum.variants {
                        let vname = &vtype.name;
                        if vname.starts_with(&prefix) {
                            let detail = if vtype.fields.is_empty() {
                                format!("{}::{}", sum.name, vname)
                            } else {
                                let fs: Vec<String> = vtype
                                    .fields
                                    .iter()
                                    .map(|(n, t)| format!("{}: {}", n, t))
                                    .collect();
                                format!("{}::{} {{ {} }}", sum.name, vname, fs.join(", "))
                            };
                            items.push(CompletionItem {
                                label: vname.clone(),
                                kind: Some(CompletionItemKind::ENUM_MEMBER),
                                detail: Some(detail),
                                ..Default::default()
                            });
                        }
                    }
                }
            } else {
                // General completions
                for (name, comp) in &checker.components {
                    if name.starts_with(&prefix) {
                        let fs: Vec<String> = comp
                            .fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t))
                            .collect();
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::CLASS),
                            detail: Some(format!("component {{ {} }}", fs.join(", "))),
                            ..Default::default()
                        });
                    }
                }
                for (name, intent) in &checker.intents {
                    if name.starts_with(&prefix) {
                        let fs = intent
                            .fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t))
                            .collect::<Vec<_>>()
                            .join(", ");
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::STRUCT),
                            detail: Some(format!("intent {{ {} }}", fs)),
                            ..Default::default()
                        });
                    }
                }
                for (name, law) in &checker.laws {
                    if name.starts_with(&prefix) {
                        let params = law
                            .params
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ");
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some(format!("law({}) — settle only", params)),
                            ..Default::default()
                        });
                    }
                }
                for (name, constraint) in &checker.constraints {
                    if name.starts_with(&prefix) {
                        let mut watches = constraint.watches.iter().cloned().collect::<Vec<_>>();
                        watches.sort();
                        let watches = if watches.is_empty() {
                            String::new()
                        } else {
                            format!(" watches {}", watches.join(", "))
                        };
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some(format!(
                                "constraint for {}{}",
                                constraint.attached_component, watches
                            )),
                            ..Default::default()
                        });
                    }
                }
                for (name, sys) in &checker.systems {
                    if name.starts_with(&prefix) {
                        let ps: Vec<String> = sys
                            .params
                            .iter()
                            .map(|p| {
                                format!(
                                    "{}{}: {}",
                                    if p.is_mut { "mut " } else { "" },
                                    p.name,
                                    p.component_type
                                )
                            })
                            .collect();
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some(format!("system({})", ps.join(", "))),
                            ..Default::default()
                        });
                    }
                }
                for (name, ev) in &checker.events {
                    if name.starts_with(&prefix) {
                        let fs: Vec<String> = ev
                            .fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t))
                            .collect();
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::EVENT),
                            detail: Some(format!("event {{ {} }}", fs.join(", "))),
                            ..Default::default()
                        });
                    }
                }
                for (name, sm) in &checker.state_machines {
                    if name.starts_with(&prefix) {
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::ENUM),
                            detail: Some(format!("state {{ {} }}", sm.states.join(", "))),
                            ..Default::default()
                        });
                    }
                }
                for (name, sum) in &checker.sum_types {
                    if name.starts_with(&prefix) {
                        let vs: Vec<String> = sum.variants.iter().map(|v| v.name.clone()).collect();
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::ENUM),
                            detail: Some(format!("type {{ {} }}", vs.join(", "))),
                            ..Default::default()
                        });
                    }
                }
                for name in checker.functions.keys() {
                    if name.starts_with(&prefix) {
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some("fn".to_string()),
                            ..Default::default()
                        });
                    }
                }

                // System parameters context
                // We can find if the cursor is inside a system block
                for decl in &r.program.declarations {
                    if let crate::ast::Decl::System(s) = decl {
                        if pos.line >= s.span.line.saturating_sub(1)
                            && pos.line <= s.body.span.line.saturating_sub(1) + 1000
                        {
                            // Rough check
                            for (pname, _, _) in &s.params {
                                if pname.starts_with(&prefix) {
                                    items.push(CompletionItem {
                                        label: pname.clone(),
                                        kind: Some(CompletionItemKind::VARIABLE),
                                        detail: Some("system parameter".to_string()),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }
}

fn format_system_hover_markdown(display_name: &str, sys: &SystemType) -> String {
    let ps: Vec<String> = sys
        .params
        .iter()
        .map(|p| {
            format!(
                "{}{}: {}",
                if p.is_mut { "mut " } else { "" },
                p.name,
                p.component_type
            )
        })
        .collect();
    format!("```rad\nsystem {}({})\n```", display_name, ps.join(", "))
}

fn format_system_completion_detail(sys: &SystemType) -> String {
    let ps: Vec<String> = sys
        .params
        .iter()
        .map(|p| {
            format!(
                "{}{}: {}",
                if p.is_mut { "mut " } else { "" },
                p.name,
                p.component_type
            )
        })
        .collect();
    format!("system({})", ps.join(", "))
}

fn document_end_position(text: &str) -> Position {
    let mut line = 0u32;
    let mut character = 0u32;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line += 1;
                character = 0;
            }
            '\n' => {
                line += 1;
                character = 0;
            }
            _ => character += ch.len_utf16() as u32,
        }
    }
    Position::new(line, character)
}

/// LSP `Position.character` is a UTF-16 code unit offset from the start of the line (LSP 3.16).
fn utf16_col_to_char_idx(line: &str, utf16_col: u32) -> usize {
    let mut utf16_seen = 0u32;
    for (idx, ch) in line.chars().enumerate() {
        let u = ch.len_utf16() as u32;
        if utf16_seen + u > utf16_col {
            return idx;
        }
        utf16_seen += u;
    }
    line.chars().count()
}

#[inline]
fn is_system_path_delimiter(c: char) -> bool {
    matches!(c, ',' | ']' | ')' | ';')
}

fn skip_chars_ws(chars: &[char], mut pos: usize) -> usize {
    while pos < chars.len() && chars[pos].is_whitespace() {
        pos += 1;
    }
    pos
}