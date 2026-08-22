/// `rad replay <trace> --with <fixed.rad>` — retroactive edits (list item
/// #6): replay the recorded session's *inputs* against *modified* source,
/// then report the blast radius of the edit by diffing the two final worlds.
fn retroactive_replay(trace_text: &str, new_path: &str, force: bool) {
    // Pass 1: faithful replay of the embedded (original) source.
    let baseline = match rad_vm::replay::TraceReplayer::parse(trace_text, force) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };
    let original_source = baseline.source().to_string();
    let trace_features = baseline.features().to_vec();
    let trace_layout = baseline.source_layout().clone();
    let mut vm_a = match rad_vm::replay_compile::compile_trace_vm(
        &original_source,
        "embedded source",
        &trace_features,
        &trace_layout,
    ) {
        Ok(vm) => vm,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };
    vm_a.suppress_output();
    vm_a.enable_replay(baseline);
    let baseline_err = vm_a.run(0).err();
    let world_a = vm_a.world_snapshot();
    let digest_a = vm_a.world_digest();
    if let Some(e) = &baseline_err {
        eprintln!("Note: the recorded run ended in an error: {}", e);
    }

    // Pass 2: retroactive replay — the edited source against the same
    // recorded inputs, served from the args-keyed oracle.
    let retro = rad_vm::replay::TraceReplayer::parse(trace_text, force)
        .expect("trace parsed once already")
        .into_retro();
    let parser_options = ParserOptions {
        compat_v0_5_dx: false,
    };
    let loaded = match load_cli_program(new_path, parser_options) {
        Ok(loaded) => loaded,
        Err(errors) => {
            eprintln!("{errors}");
            process::exit(1);
        }
    };
    if !loaded.errors.is_empty() {
        for e in &loaded.errors {
            eprintln!(
                "{}",
                format_error(&e.source, &e.filepath, &e.message, e.line, e.col)
            );
        }
        process::exit(1);
    }
    let source_identity = loaded
        .source_layout
        .digest(&loaded.merged_source)
        .expect("module loader produced an invalid source layout");
    let compile_result = match rad_vm::pipeline::compile_unchecked_program(
        &loaded.program,
        loaded.aliases,
        rad_vm::pipeline::UncheckedCompileOptions {
            features: trace_features,
            source_identity: Some(source_identity),
            ..rad_vm::pipeline::UncheckedCompileOptions::default()
        },
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                format_error(&loaded.merged_source, new_path, &e.message, e.line, e.col)
            );
            process::exit(1);
        }
    };
    let mut vm_b = VM::new();
    vm_b.enable_replay(retro);
    vm_b.load_compile_result(compile_result);
    let retro_err = vm_b.run(0).err();
    let world_b = vm_b.world_snapshot();
    let digest_b = vm_b.world_digest();
    let report = vm_b
        .finish_replay_with_outcome(retro_err.as_deref())
        .expect("retro report");

    eprintln!();
    eprintln!(
        "=== Retroactive replay: {} against the recorded session ===",
        new_path
    );
    if let Some(e) = &retro_err {
        eprintln!("Edited run halted: {}", e);
    }
    eprintln!(
        "Recorded io: {} consumed, {} repeated reads, {} unused",
        report.io_replayed, report.reused_reads, report.leftover_io
    );
    if report.virtual_writes > 0 {
        eprintln!(
            "Virtualized writes: {} write call(s) the recording never performed \
             replayed as no-ops (no real io was done)",
            report.virtual_writes
        );
    }
    let diff = rad_vm::world::WorldSnapshot::diff_summary(&world_a, &world_b);
    if digest_a == digest_b {
        eprintln!("The edit changes NOTHING: final worlds are content-identical");
    } else {
        let parts: Vec<String> = diff
            .iter()
            .map(|(name, rows)| format!("{}: {}", name, rows))
            .collect();
        eprintln!("The edit's blast radius (original vs edited final world):");
        eprintln!("  {{{}}}", parts.join(", "));
        eprintln!("  original digest: {}", digest_a);
        eprintln!("  edited digest:   {}", digest_b);
    }
}

/// Compile a self-contained merged source into a fresh VM (no checker: the
/// program already ran once to produce the trace).
/// `rad test <dir|file.rad>` — run every `test` declaration in each file.
///
/// Each file is compiled and run in-process: its top-level code executes
/// first (the fixture), then every compiled `test` block is invoked via
/// `rad_vm::test_runner::run_tests`, which reports each test by name and
/// catches per-test failures. A failing test — or a file that cannot
/// compile or whose top-level code errors — fails the run with a
/// non-zero exit.
///
/// This command used to spawn `rad <file>` per file and count each clean
/// exit as one passed "test": `test` blocks never executed, so a suite
/// asserting `1 == 2` reported PASS (A4 BUG 08, seq 73).
fn execute_test_command(test_dir: &str) {
    let path = Path::new(test_dir);
    let mut test_files = Vec::new();
    if path.is_file() {
        if path.extension().is_some_and(|ext| ext == "rad") {
            test_files.push(path.to_path_buf());
        } else {
            eprintln!("Error: test path '{}' is not a .rad file", test_dir);
            process::exit(1);
        }
    } else if path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_file() && entry_path.extension().is_some_and(|ext| ext == "rad") {
                    test_files.push(entry_path);
                }
            }
        }
        test_files.sort();
    } else {
        eprintln!("Error: test path '{}' not found", test_dir);
        process::exit(1);
    }

    if test_files.is_empty() {
        println!("No test files found in '{}'", test_dir);
        return;
    }

    let mut tests_passed = 0usize;
    let mut tests_failed = 0usize;
    let mut files_errored = 0usize;
    let mut files_without_tests = 0usize;

    for filepath in test_files {
        let filename = filepath
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let path_str = filepath.to_string_lossy().to_string();

        // Same pipeline as `rad <file>` (module graph + checker), but
        // in-process so the compiled `__test_*` functions can be called
        // after top-level code has run.
        let parser_options = ParserOptions {
            compat_v0_5_dx: false,
        };
        let loaded = match load_cli_program(&path_str, parser_options) {
            Ok(loaded) => loaded,
            Err(errors) => {
                println!("  ERROR {}", filename);
                eprintln!("{errors}");
                files_errored += 1;
                continue;
            }
        };
        let display_path = if loaded.had_imports {
            format!("<module graph from {}>", path_str)
        } else {
            path_str.clone()
        };

        let analysis = analyze_cli_program(
            &loaded,
            &path_str,
            CheckerOptions {
                warn_compat: false,
                ..CheckerOptions::default()
            },
        );

        if analysis.has_errors() {
            println!("  ERROR {}", filename);
            for error in &analysis.errors {
                eprintln!("{error}");
            }
            files_errored += 1;
            continue;
        }

        let checked = analysis
            .semantic
            .into_checked()
            .expect("error-free test analysis must produce checked semantics");
        let source_identity = match checked_source_identity(&loaded) {
            Ok(identity) => identity,
            Err(error) => {
                println!("  ERROR {}", filename);
                eprintln!("{error}");
                files_errored += 1;
                continue;
            }
        };
        let compile_result = match rad_vm::pipeline::compile_checked_program(
            &loaded.program,
            loaded.aliases,
            checked,
            rad_vm::pipeline::CheckedCompileOptions {
                source_identity: Some(source_identity),
                ..rad_vm::pipeline::CheckedCompileOptions::default()
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                println!("  ERROR {}", filename);
                eprintln!(
                    "{}",
                    format_error(
                        &loaded.merged_source,
                        &display_path,
                        &e.message,
                        e.line,
                        e.col
                    )
                );
                files_errored += 1;
                continue;
            }
        };

        let mut vm = VM::new();
        vm.load_compile_result(compile_result);
        if let Err(e) = vm.run(0) {
            // Top-level code is the fixture: if it cannot run, the file
            // fails loudly rather than silently skipping its tests.
            println!("  FAIL  {} (top-level code)", filename);
            println!("        Runtime error: {}", e);
            files_errored += 1;
            continue;
        }

        let outcomes = rad_vm::test_runner::run_tests(&mut vm);
        if outcomes.is_empty() {
            println!("  none  {} (no test declarations)", filename);
            files_without_tests += 1;
            continue;
        }
        for outcome in &outcomes {
            match &outcome.error {
                None => {
                    println!("  PASS  {} :: {}", filename, outcome.name);
                    tests_passed += 1;
                }
                Some(e) => {
                    println!("  FAIL  {} :: {}", filename, outcome.name);
                    println!("        {}", e);
                    if let Some(artifact) = write_model_failure_artifact(
                        e,
                        &filepath,
                        &loaded.merged_source,
                        &loaded.source_layout,
                        None,
                    ) {
                        println!("        replay: rad replay {}", artifact.display());
                    }
                    tests_failed += 1;
                }
            }
        }
    }

    let mut summary = format!(
        "\nResults: {} passed, {} failed, {} total",
        tests_passed,
        tests_failed,
        tests_passed + tests_failed
    );
    if files_without_tests > 0 {
        summary.push_str(&format!(" ({} file(s) with no tests)", files_without_tests));
    }
    if files_errored > 0 {
        summary.push_str(&format!(" ({} file(s) failed to run)", files_errored));
    }
    println!("{}", summary);
    if tests_failed > 0 || files_errored > 0 {
        process::exit(1);
    }
}

fn write_model_failure_artifact(
    error: &str,
    source_path: &Path,
    source: &str,
    source_layout: &rad_vm::source_bundle::SourceLayout,
    artifact_directory: Option<&Path>,
) -> Option<std::path::PathBuf> {
    let mut artifact = model_failure_json(error)?;
    let object = artifact.as_object_mut()?;
    object.insert(
        "source".to_string(),
        serde_json::Value::String(source.to_string()),
    );
    object.insert(
        "source_layout".to_string(),
        serde_json::to_value(source_layout).ok()?,
    );
    object.insert("features".to_string(), serde_json::json!([]));
    let model = object.get("model")?.as_str()?;
    let safe_model = model
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let seed = object
        .get("seed")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let stem = source_path.file_stem()?.to_string_lossy();
    let filename = format!("{stem}.{safe_model}.{seed}.radr");
    let artifact_path = if let Some(directory) = artifact_directory {
        fs::create_dir_all(directory).ok()?;
        directory.join(filename)
    } else {
        source_path.with_file_name(filename)
    };
    let bytes = serde_json::to_vec_pretty(&artifact).ok()?;
    fs::write(&artifact_path, bytes).ok()?;
    Some(artifact_path)
}

fn model_failure_json(error: &str) -> Option<serde_json::Value> {
    let encoded = error
        .lines()
        .find_map(|line| line.strip_prefix("RAD_MODEL_FAILURE="))?;
    serde_json::from_str(encoded).ok()
}

fn resolve_source_for_error<'a>(
    file: Option<rad_vm::ast::FileId>,
    source_map: &'a SourceMap,
    fallback_source: &'a str,
    fallback_path: &'a str,
) -> (&'a str, &'a str) {
    if let Some(fid) = file {
        if let Some(sf) = source_map.get_file(fid) {
            return (&sf.source, &sf.path);
        }
    }
    (fallback_source, fallback_path)
}

fn format_error(source: &str, filepath: &str, message: &str, line: u32, col: u32) -> String {
    format_diagnostic("Error", source, filepath, message, line, col)
}

fn format_warning(source: &str, filepath: &str, message: &str, line: u32, col: u32) -> String {
    format_diagnostic("Warning", source, filepath, message, line, col)
}

fn format_diagnostic(
    kind: &str,
    source: &str,
    filepath: &str,
    message: &str,
    line: u32,
    col: u32,
) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let lineno = line as usize;

    let mut parts = vec![format!("  {}: {}\n", kind, message)];
    parts.push(format!("  --> {}:{}:{}", filepath, line, col));
    parts.push("   |".to_string());

    if lineno == 0 || lines.is_empty() {
        parts.push(String::new());
        return parts.join("\n");
    }

    let start = lineno.saturating_sub(2);
    let end = std::cmp::min(lines.len(), lineno + 1);
    for (i, line_text) in lines.iter().enumerate().take(end).skip(start) {
        let prefix = if i + 1 == lineno { ">> " } else { "   " };
        let line_prefix = format!("{}{:4} | ", prefix, i + 1);
        parts.push(format!("{}{}", line_prefix, line_text));
        if i + 1 == lineno && col > 0 {
            let caret_indent = line_prefix.chars().count() + (col - 1) as usize;
            parts.push(format!("{}^", " ".repeat(caret_indent)));
        }
    }
    parts.push(String::new());
    parts.join("\n")
}
