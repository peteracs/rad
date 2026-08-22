struct CliSemanticAnalysis {
    semantic: rad_vm::pipeline::SemanticAnalysis,
    errors: Vec<String>,
    warnings: Vec<String>,
}

struct CliCompiledProgram {
    loaded: rad_vm::module_loader::LoadResult,
    authority: rad_vm::types::AuthorityReport,
    compiled: rad_vm::compiler::CompileResult,
}

impl CliSemanticAnalysis {
    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    fn rendered_errors(&self) -> String {
        self.errors.join("\n")
    }
}

fn load_cli_program(
    filepath: &str,
    parser_options: ParserOptions,
) -> Result<rad_vm::module_loader::LoadResult, String> {
    load_program_with_source_map_and_options(filepath, parser_options)
        .map_err(render_module_load_errors)
}

fn render_module_load_errors(errors: Vec<rad_vm::module_loader::ModuleLoadError>) -> String {
    errors
        .into_iter()
        .map(|error| {
            format_error(
                &error.source,
                &error.filepath,
                &error.message,
                error.line,
                error.col,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn analyze_cli_program(
    loaded: &rad_vm::module_loader::LoadResult,
    filepath: &str,
    checker_options: CheckerOptions,
) -> CliSemanticAnalysis {
    let display_path = if loaded.had_imports {
        format!("<module graph from {filepath}>")
    } else {
        filepath.to_string()
    };
    let semantic =
        rad_vm::pipeline::analyze_program(&loaded.program, &loaded.aliases, checker_options);
    let mut errors = loaded
        .errors
        .iter()
        .map(|error| {
            format_error(
                &error.source,
                &error.filepath,
                &error.message,
                error.line,
                error.col,
            )
        })
        .collect::<Vec<_>>();
    errors.extend(semantic.errors().iter().map(|error| {
        let (source, path) = resolve_source_for_error(
            error.file,
            &loaded.source_map,
            &loaded.merged_source,
            &display_path,
        );
        let mut rendered = format_error(source, path, &error.message, error.line, error.col);
        if let Some(hint) = &error.hint {
            rendered.push_str(&format!("\n  hint: {hint}"));
        }
        rendered
    }));
    let warnings = semantic
        .warnings()
        .iter()
        .map(|warning| {
            let (source, path) = resolve_source_for_error(
                warning.file,
                &loaded.source_map,
                &loaded.merged_source,
                &display_path,
            );
            let mut rendered =
                format_warning(source, path, &warning.message, warning.line, warning.col);
            if let Some(hint) = &warning.hint {
                rendered.push_str(&format!("\n  hint: {hint}"));
            }
            rendered
        })
        .collect();
    CliSemanticAnalysis {
        semantic,
        errors,
        warnings,
    }
}

fn checked_source_identity(loaded: &rad_vm::module_loader::LoadResult) -> Result<String, String> {
    loaded
        .source_layout
        .digest(&loaded.merged_source)
        .map_err(|error| format!("module loader produced an invalid source layout: {error}"))
}

fn load_checked_cli_program(filepath: &str) -> Result<CliCompiledProgram, String> {
    let loaded = load_cli_program(filepath, ParserOptions)?;
    let analysis = analyze_cli_program(&loaded, filepath, CheckerOptions::default());
    if analysis.has_errors() {
        return Err(analysis.rendered_errors());
    }
    let authority = analysis.semantic.output().authority().clone();
    let checked = analysis.semantic.into_checked().map_err(|invalid| {
        format!(
            "semantic analysis unexpectedly failed: {:?}",
            invalid.errors()
        )
    })?;
    let source_identity = checked_source_identity(&loaded)?;
    let compiled = rad_vm::pipeline::compile_checked_program(
        &loaded.program,
        loaded.aliases.clone(),
        checked,
        rad_vm::pipeline::CheckedCompileOptions {
            source_identity: Some(source_identity),
            ..rad_vm::pipeline::CheckedCompileOptions::default()
        },
    )
    .map_err(|error| {
        format_error(
            &loaded.merged_source,
            filepath,
            &error.message,
            error.line,
            error.col,
        )
    })?;
    Ok(CliCompiledProgram {
        loaded,
        authority,
        compiled,
    })
}
