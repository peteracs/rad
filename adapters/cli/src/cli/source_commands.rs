fn run_surface_command(filepath: &str, json: bool) {
    let loaded = match load_cli_program(filepath, ParserOptions) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    };
    if !loaded.errors.is_empty() {
        eprintln!("{}", render_module_load_errors(loaded.errors.clone()));
        process::exit(1);
    }
    let coverage = rad_vm::surface_coverage::LanguageSurfaceCoverage::loaded(
        &loaded.program,
        &loaded.aliases,
        &loaded.source_map,
    );
    if !coverage.lexical_errors.is_empty() {
        eprintln!(
            "language-surface collection found lexical errors:\n{}",
            serde_json::to_string_pretty(&coverage.lexical_errors)
                .expect("lexical error map is serializable")
        );
        process::exit(1);
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&coverage)
                .expect("language-surface report is serializable")
        );
    } else {
        println!("Language surface: {filepath}");
        println!("  source files: {}", coverage.source_files.len());
        println!("  tokens: {}", coverage.tokens.len());
        println!("  declarations: {}", coverage.declarations.len());
        println!("  statements: {}", coverage.statements.len());
        println!("  expressions: {}", coverage.expressions.len());
        println!("  builtins called: {}", coverage.builtins.len());
    }
}

fn run_relations_check_command(filepath: &str, module_id: String, experimental_relations: bool) {
    let source = match fs::File::open(filepath) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("Error reading {filepath}: {error}");
            process::exit(1);
        }
    };
    let options = rad_vm::relation::frontend::FrontendOptions {
        enabled: experimental_relations,
        module_id,
        ..rad_vm::relation::frontend::FrontendOptions::default()
    };
    match rad_vm::relation::frontend::compile_reader(source, &options) {
        Ok(artifacts) => println!(
            "relations: {} schemas, {} rules, manifest {}",
            artifacts.relations.schemas().len(),
            artifacts.rules.len(),
            hex::encode(artifacts.manifest_digest.as_bytes())
        ),
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                eprintln!(
                    "{}:{}:{} [{}] {}",
                    filepath,
                    diagnostic.line,
                    diagnostic.column,
                    diagnostic.code.as_str(),
                    diagnostic.message
                );
            }
            process::exit(1);
        }
    }
}

fn load_runtime_relation_schema(
    filepath: &str,
    module_id: String,
    experimental_relations: bool,
) -> Result<rad_vm::relation::frontend::FrontendArtifacts, String> {
    let source = fs::File::open(filepath)
        .map_err(|error| format!("Error reading relation schema {filepath}: {error}"))?;
    let options = rad_vm::relation::frontend::FrontendOptions {
        enabled: experimental_relations,
        module_id,
        ..rad_vm::relation::frontend::FrontendOptions::default()
    };
    let artifacts = rad_vm::relation::frontend::compile_reader(source, &options).map_err(
        |diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| {
                    format!(
                        "{filepath}:{}:{} [{}] {}",
                        diagnostic.line,
                        diagnostic.column,
                        diagnostic.code.as_str(),
                        diagnostic.message
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        },
    )?;
    if !artifacts.operations.is_empty() {
        return Err(format!(
            "Relation schema {filepath} contains {} operation(s); --relation-schema accepts declarations and derivation rules only",
            artifacts.operations.len()
        ));
    }
    Ok(artifacts)
}

fn collect_rad_files(filepaths: Vec<String>) -> Vec<String> {
    let all_files = if filepaths.is_empty() {
        vec![".".to_string()]
    } else {
        filepaths
    };
    let mut rad_files = Vec::new();
    for target in all_files {
        let path = Path::new(&target);
        if path.is_file() && path.extension().is_some_and(|extension| extension == "rad") {
            rad_files.push(target);
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let mut directories = vec![path.to_path_buf()];
        while let Some(directory) = directories.pop() {
            if let Ok(entries) = fs::read_dir(directory) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        let name = entry_path.file_name().unwrap_or_default().to_string_lossy();
                        if name != "node_modules" && name != "target" {
                            directories.push(entry_path);
                        }
                    } else if entry_path
                        .extension()
                        .is_some_and(|extension| extension == "rad")
                    {
                        rad_files.push(entry_path.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    rad_files.sort();
    rad_files
}

fn run_format_command(filepaths: Vec<String>, check_only: bool) {
    let rad_files = collect_rad_files(filepaths);
    if rad_files.is_empty() {
        println!("No .rad files found");
        return;
    }
    let mut changed = 0;
    for filepath in &rad_files {
        let source = match fs::read_to_string(filepath) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("Error reading {filepath}: {error}");
                continue;
            }
        };
        let formatted = rad_vm::formatter::format_rad(&source);
        if formatted == source {
            if !check_only {
                println!("  unchanged {filepath}");
            }
            continue;
        }
        if check_only {
            println!("  needs formatting: {filepath}");
            changed += 1;
        } else if let Err(error) = fs::write(filepath, &formatted) {
            eprintln!("Error writing {filepath}: {error}");
        } else {
            println!("  formatted {filepath}");
            changed += 1;
        }
    }
    if check_only && changed > 0 {
        println!("\n{changed} file(s) need formatting. Run `rad fmt` to fix.");
        process::exit(1);
    }
}
