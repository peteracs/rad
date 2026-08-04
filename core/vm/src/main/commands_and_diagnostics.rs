

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
    let loaded = match load_program_with_source_map_and_options(new_path, parser_options) {
        Ok(r) => r,
        Err(errors) => {
            for e in errors {
                eprintln!(
                    "{}",
                    format_error(&e.source, &e.filepath, &e.message, e.line, e.col)
                );
            }
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
    let compile_result = match Compiler::new()
        .with_features(trace_features)
        .with_program_source_identity(
            loaded
                .source_layout
                .digest(&loaded.merged_source)
                .expect("module loader produced an invalid source layout"),
        )
        .compile(&loaded.program)
    {
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
        let loaded = match load_program_with_source_map_and_options(&path_str, parser_options) {
            Ok(r) => r,
            Err(errors) => {
                println!("  ERROR {}", filename);
                for e in &errors {
                    eprintln!(
                        "{}",
                        format_error(&e.source, &e.filepath, &e.message, e.line, e.col)
                    );
                }
                files_errored += 1;
                continue;
            }
        };
        let display_path = if loaded.had_imports {
            format!("<module graph from {}>", path_str)
        } else {
            path_str.clone()
        };

        let mut checker = Checker::new_with_options(CheckerOptions {
            compat_v0_5_dx: false,
            warn_compat: false,
            strict_types: false,
            features: Vec::new(),
        });
        checker.set_aliases(loaded.aliases.clone());
        let check_errors = checker.check(&loaded.program);
        let checker_output = checker.output();

        if !loaded.errors.is_empty() || !check_errors.is_empty() {
            println!("  ERROR {}", filename);
            for e in &loaded.errors {
                eprintln!(
                    "{}",
                    format_error(&e.source, &e.filepath, &e.message, e.line, e.col)
                );
            }
            for err in &check_errors {
                let (src, epath) = resolve_source_for_error(
                    err.file,
                    &loaded.source_map,
                    &loaded.merged_source,
                    &display_path,
                );
                eprintln!(
                    "{}",
                    format_error(src, epath, &err.message, err.line, err.col)
                );
            }
            files_errored += 1;
            continue;
        }

        let compiler = Compiler::new()
            .with_checker_output(checker_output)
            .with_aliases(loaded.aliases)
            .with_program_source_identity(
                loaded
                    .source_layout
                    .digest(&loaded.merged_source)
                    .expect("module loader produced an invalid source layout"),
            );
        let compile_result = match compiler.compile(&loaded.program) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_flags_are_recognized() {
        assert!(wants_help(&["rad".to_string(), "--help".to_string()]));
        assert!(wants_help(&["rad".to_string(), "-h".to_string()]));
        assert!(!wants_help(&["rad".to_string()]));
        assert!(!wants_help(&[
            "rad".to_string(),
            "script.rad".to_string(),
            "--help".to_string(),
        ]));
    }

    fn assert_parses_run_with_no_check(args: Vec<String>) {
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run {
                filepath,
                skip_check,
                ..
            } => {
                assert_eq!(filepath, "script.rad");
                assert!(skip_check);
            }
            CliCommand::Version => panic!("expected run command"),
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Replay { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_no_check_before_and_after_file() {
        let cases = vec![
            vec![
                "rad".to_string(),
                "--no-check".to_string(),
                "script.rad".to_string(),
            ],
            vec![
                "rad".to_string(),
                "script.rad".to_string(),
                "--no-check".to_string(),
            ],
        ];

        for args in cases {
            assert_parses_run_with_no_check(args);
        }
    }

    #[test]
    fn parse_cli_args_accepts_bounded_relations_check() {
        let args = vec![
            "rad".to_string(),
            "relations".to_string(),
            "check".to_string(),
            "facts.rad".to_string(),
            "--experimental-relations".to_string(),
            "--module".to_string(),
            "game::facts".to_string(),
        ];
        match parse_cli_args(&args).unwrap() {
            CliCommand::RelationsCheck {
                filepath,
                module_id,
                experimental_relations,
            } => {
                assert_eq!(filepath, "facts.rad");
                assert_eq!(module_id, "game::facts");
                assert!(experimental_relations);
            }
            _ => panic!("expected relations check command"),
        }
    }

    #[test]
    fn parse_cli_args_feature_gates_relation_lsp_support() {
        let args = vec![
            "rad".to_string(),
            "lsp".to_string(),
            "--experimental-relations".to_string(),
        ];
        assert!(matches!(
            parse_cli_args(&args).unwrap(),
            CliCommand::Lsp {
                experimental_relations: true
            }
        ));
    }

    #[test]
    fn parse_cli_args_supports_version_anywhere_without_file() {
        let args = vec![
            "rad".to_string(),
            "--no-check".to_string(),
            "--version".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        assert!(matches!(parsed, CliCommand::Version));
    }

    #[test]
    fn parse_cli_args_accepts_compat_flag() {
        let args = vec![
            "rad".to_string(),
            "--compat-v0.5-dx".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run {
                filepath,
                skip_check,
                compat_v0_5_dx,
                deny_warnings,
                warn_compat,
                strict_types,
                write_lock,
                ..
            } => {
                assert_eq!(filepath, "script.rad");
                assert!(!skip_check);
                assert!(compat_v0_5_dx);
                assert!(!deny_warnings);
                assert!(warn_compat);
                assert!(!strict_types);
                assert!(!write_lock);
            }
            CliCommand::Version => panic!("expected run command"),
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Replay { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_no_compat_flag() {
        let args = vec![
            "rad".to_string(),
            "--no-compat-v0.5-dx".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run { compat_v0_5_dx, .. } => {
                assert!(!compat_v0_5_dx);
            }
            CliCommand::Version => panic!("expected run command"),
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Replay { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_compat_last_flag_wins() {
        let args = vec![
            "rad".to_string(),
            "--compat-v0.5-dx".to_string(),
            "--no-compat-v0.5-dx".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run { compat_v0_5_dx, .. } => {
                assert!(!compat_v0_5_dx);
            }
            CliCommand::Version => panic!("expected run command"),
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Replay { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_warning_policy_flags() {
        let args = vec![
            "rad".to_string(),
            "--deny-warnings".to_string(),
            "--no-warn-compat".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run {
                filepath,
                skip_check,
                compat_v0_5_dx,
                deny_warnings,
                warn_compat,
                strict_types,
                write_lock,
                ..
            } => {
                assert_eq!(filepath, "script.rad");
                assert!(!skip_check);
                assert!(!compat_v0_5_dx);
                assert!(deny_warnings);
                assert!(!warn_compat);
                assert!(!strict_types);
                assert!(!write_lock);
            }
            CliCommand::Version => panic!("expected run command"),
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Replay { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_strict_and_lock_flags() {
        let args = vec![
            "rad".to_string(),
            "--strict-types".to_string(),
            "--write-lock".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run {
                strict_types,
                write_lock,
                ..
            } => {
                assert!(strict_types);
                assert!(write_lock);
            }
            CliCommand::Version => panic!("expected run command"),
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Replay { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_profile_copies_flag() {
        let args = vec![
            "rad".to_string(),
            "--profile-copies".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run { profile_copies, .. } => {
                assert!(profile_copies);
            }
            CliCommand::Version => panic!("expected run command"),
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Replay { .. } => {
                todo!()
            }
        }
    }

    #[test]
    fn parse_cli_args_accepts_record_flag() {
        for args in [
            vec![
                "rad".to_string(),
                "script.rad".to_string(),
                "--record".to_string(),
                "trace.radr".to_string(),
            ],
            vec![
                "rad".to_string(),
                "--record=trace.radr".to_string(),
                "script.rad".to_string(),
            ],
        ] {
            let parsed = parse_cli_args(&args).unwrap();
            match parsed {
                CliCommand::Run { record, .. } => {
                    assert_eq!(record.as_deref(), Some("trace.radr"));
                }
                other => panic!("expected run command, got {:?}", other),
            }
        }
        let missing = vec![
            "rad".to_string(),
            "script.rad".to_string(),
            "--record".to_string(),
        ];
        assert!(parse_cli_args(&missing).is_err());
    }

    /// `rad run` outside a project directory must explain itself instead of
    /// trying to open a file literally named "run".
    #[test]
    fn parse_cli_args_run_without_rad_toml_is_a_helpful_error() {
        let args: Vec<String> = ["rad", "run"].iter().map(|s| s.to_string()).collect();
        // cargo test runs in core/vm/, which has no rad.toml
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("no rad.toml"), "got: {}", err);
        assert!(err.contains("rad new"), "got: {}", err);
    }

    /// Everything after `--` belongs to the program: flags are not parsed,
    /// and sys_args() receives exactly these strings.
    #[test]
    fn parse_cli_args_passes_program_args_after_double_dash() {
        let args: Vec<String> = ["rad", "script.rad", "--", "alice", "work/dir", "--record"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        match parse_cli_args(&args).unwrap() {
            CliCommand::Run {
                filepath,
                record,
                program_args,
                ..
            } => {
                assert_eq!(filepath, "script.rad");
                // `--record` after `--` is data, not a rad flag
                assert_eq!(record, None);
                assert_eq!(program_args, vec!["alice", "work/dir", "--record"]);
            }
            other => panic!("expected run command, got {:?}", other),
        }
    }

    #[test]
    fn parse_cli_args_parses_replay_command() {
        let args = vec![
            "rad".to_string(),
            "replay".to_string(),
            "trace.radr".to_string(),
            "--to-frame".to_string(),
            "42".to_string(),
            "--force".to_string(),
        ];
        match parse_cli_args(&args).unwrap() {
            CliCommand::Replay {
                trace_path,
                to_frame,
                force,
                serve,
                with_source,
            } => {
                assert_eq!(trace_path, "trace.radr");
                assert_eq!(to_frame, Some(42));
                assert!(force);
                assert!(!serve);
                assert!(with_source.is_none());
            }
            other => panic!("expected replay command, got {:?}", other),
        }
        // Missing trace path is an error.
        let missing = vec!["rad".to_string(), "replay".to_string()];
        assert!(parse_cli_args(&missing).is_err());
        // Bad frame number is an error.
        let bad = vec![
            "rad".to_string(),
            "replay".to_string(),
            "t.radr".to_string(),
            "--to-frame=abc".to_string(),
        ];
        assert!(parse_cli_args(&bad).is_err());
    }

    #[test]
    fn parse_cli_args_parses_build_target_wasm() {
        let args = vec![
            "rad".to_string(),
            "build".to_string(),
            "--target".to_string(),
            "wasm".to_string(),
            "a.rad".to_string(),
            "out.wasm".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Build {
                input_rad,
                output_wasm,
            } => {
                assert_eq!(input_rad, "a.rad");
                assert_eq!(output_wasm, "out.wasm");
            }
            _ => panic!("expected build"),
        }
    }

    #[test]
    fn parse_cli_args_rejects_unknown_option() {
        let args = vec!["rad".to_string(), "--wat".to_string()];
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("Unknown option"));
    }

    #[test]
    fn parse_cli_args_rejects_version_with_input_file() {
        let args = vec![
            "rad".to_string(),
            "--version".to_string(),
            "script.rad".to_string(),
        ];
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("--version cannot be combined"));
    }

    #[test]
    fn parse_cli_args_requires_single_input_file() {
        let args = vec!["rad".to_string()];
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("Usage:"));
    }

    #[test]
    fn format_error_caret_aligns_with_column() {
        let out = format_error("ab\nxyz", "file.rad", "bad", 2, 2);
        assert!(out.contains(">>    2 | xyz"));
        assert!(out.contains("^"));
    }
}
