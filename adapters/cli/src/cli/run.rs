fn main() {
    rad_vm::allocation_meter::mark_allocator_installed();
    let args: Vec<String> = env::args().collect();
    if wants_help(&args) {
        println!(
            "{}",
            usage(args.first().map(String::as_str).unwrap_or("rad"))
        );
        return;
    }
    let command = match parse_cli_args(&args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{}", msg);
            process::exit(1);
        }
    };

    if let CliCommand::Version = command {
        println!("rad {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if let CliCommand::Authority {
        query,
        filepath,
        json,
    } = command
    {
        run_authority_command(query, filepath, json);
        return;
    }

    if let CliCommand::Operational {
        query,
        filepath,
        json,
    } = command
    {
        run_operational_command(query, filepath, json);
        return;
    }

    if let CliCommand::Bench {
        filepath,
        json,
        program_args,
    } = command
    {
        run_bench_command(filepath, json, program_args);
        return;
    }

    if let CliCommand::ModelCheck {
        filepath,
        model,
        runs,
        max_commands,
        seed,
        artifact_directory,
        json,
    } = command
    {
        run_model_check_command(
            filepath,
            model,
            runs,
            max_commands,
            seed,
            artifact_directory,
            json,
        );
        return;
    }

    if let CliCommand::ShrinkModel { artifact } = command {
        run_model_shrink_command(&artifact);
        return;
    }

    if let CliCommand::FfiVerify {
        plugin,
        contract,
        json,
    } = command
    {
        run_ffi_verify_command(&plugin, contract.as_deref(), json);
        return;
    }

    if let CliCommand::RelationsCheck {
        filepath,
        module_id,
        experimental_relations,
    } = command
    {
        run_relations_check_command(&filepath, module_id, experimental_relations);
        return;
    }

    if let CliCommand::Fmt {
        filepaths,
        check_only,
    } = command
    {
        run_format_command(filepaths, check_only);
        return;
    }

    if let CliCommand::Lint {
        filepaths,
        preset,
        boundaries,
    } = command
    {
        let rad_files = collect_rad_files(filepaths);

        if rad_files.is_empty() {
            println!("No .rad files found");
            return;
        }

        let preset_data = match rad_vm::linter::get_preset(&preset) {
            Some(p) => p,
            None => {
                eprintln!("Unknown preset: {}", preset);
                eprintln!("Available presets: standard (default), strict, enterprise, teaching");
                process::exit(1);
            }
        };

        println!(
            "Linting with preset '{}': {}",
            preset, preset_data.description
        );
        println!();

        let mut total_issues = 0;
        for filepath in &rad_files {
            let source = match fs::read_to_string(filepath) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let (issues, _) = rad_vm::linter::lint_source(&source, &preset);
            let file_issues = issues.len();

            let parser_options = ParserOptions {
                compat_v0_5_dx: false,
            };
            let mut vm_issues = Vec::new();
            let mut ast_issues = Vec::new();

            if let Ok(r) = load_program_with_source_map_and_options(filepath, parser_options) {
                // The last CLI entry point that built its own checker. The
                // lint preset still chooses the options, but the parse/check
                // flow is now the shared one.
                let analysis = rad_vm::pipeline::analyze_program(
                    &r.program,
                    &r.aliases,
                    CheckerOptions {
                        compat_v0_5_dx: false,
                        warn_compat: preset_data.vm_flags.contains(&"--warn-compat"),
                        strict_types: preset_data.vm_flags.contains(&"--strict-types"),
                        features: vec![],
                    },
                );
                let errors = analysis.errors().to_vec();
                let warnings = analysis.warnings().to_vec();

                ast_issues = rad_vm::linter::lint_ast(
                    &r.program,
                    analysis.output(),
                    &preset_data,
                    filepath,
                    &boundaries,
                );

                // The checker sees the merged import graph, so its
                // diagnostics can point into OTHER files. Report only the
                // ones belonging to the file being linted (entry file =
                // FileId(0); None = no source map) — imported modules get
                // theirs when they are linted themselves — and carry the
                // line so the output is navigable.
                let is_entry = |file: Option<rad_vm::ast::FileId>| {
                    matches!(file, None | Some(rad_vm::ast::FileId(0)))
                };
                for err in errors {
                    if is_entry(err.file) {
                        vm_issues.push(format!("L{:<4} Error: {}", err.line, err.message));
                    }
                }
                for warn in warnings {
                    if is_entry(warn.file) {
                        vm_issues.push(format!("L{:<4} Warning: {}", warn.line, warn.message));
                    }
                }
            }

            if file_issues > 0 || !vm_issues.is_empty() || !ast_issues.is_empty() {
                println!("  {}", filepath);
                for issue in &issues {
                    let sev = issue.severity.to_uppercase();
                    println!(
                        "    {:<7} L{:<4} [{}] {}",
                        sev, issue.line, issue.code, issue.message
                    );
                }
                for issue in &ast_issues {
                    let sev = issue.severity.to_uppercase();
                    println!(
                        "    {:<7} L{:<4} [{}] {}",
                        sev, issue.line, issue.code, issue.message
                    );
                }
                for vi in &vm_issues {
                    println!("    VM      {}", vi);
                }
                total_issues += file_issues + ast_issues.len() + vm_issues.len();
            } else {
                println!("  {}  OK", filepath);
            }
        }

        println!(
            "\n{} issue(s) found across {} file(s)",
            total_issues,
            rad_files.len()
        );
        if total_issues > 0 {
            process::exit(1);
        }
        return;
    }

    if let CliCommand::Test { test_dir } = command {
        execute_test_command(&test_dir);
        return;
    }

    if let CliCommand::Types {
        input_rad,
        output_typescript,
        features,
    } = command
    {
        let loaded = match load_cli_program(
            &input_rad,
            ParserOptions {
                compat_v0_5_dx: false,
            },
        ) {
            Ok(loaded) => loaded,
            Err(errors) => {
                eprintln!("{errors}");
                process::exit(1);
            }
        };
        let analysis = analyze_cli_program(
            &loaded,
            &input_rad,
            CheckerOptions {
                compat_v0_5_dx: false,
                warn_compat: false,
                strict_types: true,
                features,
            },
        );
        if analysis.has_errors() {
            for error in &analysis.errors {
                eprintln!("{error}");
            }
            process::exit(1);
        }
        let declarations =
            match rad_vm::typescript::generate(&loaded.program, analysis.semantic.output()) {
                Ok(declarations) => declarations,
                Err(error) => {
                    eprintln!("TypeScript generation failed: {error}");
                    process::exit(1);
                }
            };
        if let Some(parent) = Path::new(&output_typescript).parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("Cannot create {}: {error}", parent.display());
                process::exit(1);
            }
        }
        if let Err(error) = fs::write(&output_typescript, declarations) {
            eprintln!("Cannot write {output_typescript}: {error}");
            process::exit(1);
        }
        println!("Generated {output_typescript}");
        return;
    }

    #[cfg(not(target_arch = "wasm32"))]
    if let CliCommand::Lsp {
        experimental_relations,
    } = command
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();

            let (service, socket) = tower_lsp::LspService::new(|client| rad_lsp::LspBackend {
                client,
                documents: tokio::sync::RwLock::new(std::collections::HashMap::new()),
                experimental_relations,
            });
            tower_lsp::Server::new(stdin, stdout, socket)
                .serve(service)
                .await;
        });
        return;
    }

    if let CliCommand::New {
        project_name,
        template,
        list_templates,
    } = command
    {
        rad_vm::scaffold::execute_new(project_name, template, list_templates);
        return;
    }

    if let CliCommand::Snapshot {
        directory,
        update,
        create,
        experimental_laws,
    } = command
    {
        rad_vm::snapshot::execute_snapshot(directory, update, create, experimental_laws);
        return;
    }

    if let CliCommand::Play { port } = command {
        rad_vm::play::execute_play(port);
        return;
    }

    if let CliCommand::Build {
        input_rad,
        output_wasm,
    } = command
    {
        let loaded = match load_cli_program(&input_rad, ParserOptions::default()) {
            Ok(loaded) => loaded,
            Err(errors) => {
                eprintln!("{errors}");
                process::exit(1);
            }
        };
        let analysis = analyze_cli_program(&loaded, &input_rad, CheckerOptions::default());
        for error in &analysis.errors {
            eprintln!("{error}");
        }
        for warning in &analysis.warnings {
            eprintln!("{warning}");
        }
        if analysis.has_errors() {
            process::exit(1);
        }

        let wasm_bytes = match env::var("RAD_COMPILER_WASM") {
            Ok(p) => match fs::read(&p) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("RAD_COMPILER_WASM {}: {}", p, e);
                    process::exit(1);
                }
            },
            Err(_) => rad_vm::wasm_binary_emit::emit_compiler_reactor_stub_module(),
        };

        if let Err(e) = fs::write(&output_wasm, wasm_bytes) {
            eprintln!("Error writing {}: {}", output_wasm, e);
            process::exit(1);
        }
        return;
    }

    if let CliCommand::Replay {
        trace_path,
        to_frame,
        force,
        serve,
        with_source,
    } = command
    {
        let trace_bytes = match fs::read(&trace_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Error reading trace {}: {}", trace_path, e);
                process::exit(1);
            }
        };
        // Normalize RADPACK tapes (binary or text envelope) to raw JSONL
        // before any consumer sees them; vintage raw tapes pass through.
        let trace_text = match rad_vm::radpack::open_file(&trace_bytes) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Error unpacking trace {}: {}", trace_path, e);
                process::exit(1);
            }
        };

        if let Ok(artifact) = serde_json::from_str::<serde_json::Value>(&trace_text) {
            if artifact.get("kind").and_then(serde_json::Value::as_str)
                == Some("rad_model_failure_v1")
            {
                if to_frame.is_some() || serve || with_source.is_some() {
                    eprintln!(
                        "Error: model-failure artifacts replay one deterministic generated case and do not support --to-frame, --serve, or --with"
                    );
                    process::exit(1);
                }
                let source = artifact
                    .get("source")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let model = artifact
                    .get("model")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let features = artifact
                    .get("features")
                    .cloned()
                    .and_then(|value| serde_json::from_value::<Vec<String>>(value).ok())
                    .unwrap_or_default();
                let source_layout = artifact
                    .get("source_layout")
                    .cloned()
                    .and_then(|value| {
                        serde_json::from_value::<rad_vm::source_bundle::SourceLayout>(value).ok()
                    })
                    .unwrap_or_default();
                let mut vm = match rad_vm::replay_compile::compile_trace_vm(
                    source,
                    "model-failure artifact",
                    &features,
                    &source_layout,
                ) {
                    Ok(vm) => vm,
                    Err(error) => {
                        eprintln!("Error: {error}");
                        process::exit(1);
                    }
                };
                let seed = artifact
                    .get("seed")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default();
                let trace = artifact
                    .get("minimal_trace")
                    .cloned()
                    .and_then(|value| {
                        serde_json::from_value::<Vec<rad_vm::vm::ModelTraceStep>>(value).ok()
                    })
                    .unwrap_or_default();
                if trace.is_empty() {
                    eprintln!("Model replay artifact omits its minimal trace");
                    process::exit(1);
                }
                vm.configure_model_check(rad_vm::vm::ModelCheckConfig {
                    model: Some(model.to_string()),
                    runs: Some(1),
                    max_commands: Some(trace.len() as u32),
                    seed: Some(seed),
                    trace: Some(trace),
                });
                if let Err(error) = vm.run(0) {
                    eprintln!("Model replay fixture failed: {error}");
                    process::exit(1);
                }
                let expected_name = format!("model_{model}");
                let outcomes = rad_vm::test_runner::run_tests(&mut vm);
                let Some(outcome) = outcomes
                    .iter()
                    .find(|outcome| outcome.name == expected_name)
                else {
                    eprintln!("Model replay artifact names missing model '{model}'");
                    process::exit(1);
                };
                match &outcome.error {
                    Some(error) => {
                        eprintln!("Model failure reproduced: {model}");
                        eprintln!("{error}");
                    }
                    None => {
                        eprintln!("Model replay DIVERGED: '{model}' now passes");
                        process::exit(1);
                    }
                }
                return;
            }
        }

        if let Some(new_path) = with_source {
            retroactive_replay(&trace_text, &new_path, force);
            return;
        }

        if serve {
            // Time-travel session: one replay pass with per-frame keyframes,
            // then JSON-RPC over stdio. stdout is the protocol channel.
            let mut server =
                match rad_vm::replay_serve::ReplayServer::from_trace(&trace_text, force) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        process::exit(1);
                    }
                };
            eprintln!("rad replay --serve: timeline ready, awaiting JSON-RPC on stdin");
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            if let Err(e) = server.serve(stdin.lock(), stdout.lock()) {
                eprintln!("serve error: {}", e);
                process::exit(1);
            }
            return;
        }
        let mut replayer = match rad_vm::replay::TraceReplayer::parse(&trace_text, force) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        };
        if let Some(n) = to_frame {
            // Out-of-range requests used to be silently dropped: the whole
            // trace ran and "Replay verified" printed for a stop that never
            // happened — poison for bisecting by frame (A4 BUG 06).
            if let Err(e) = replayer.validate_stop_frame(n) {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
            if n == 0 {
                // Stop before frame 0: nothing runs.
                eprintln!("{} 0", rad_vm::replay::REPLAY_STOP_PREFIX);
                eprintln!(
                    "Replay: 0 frame(s), 0 io record(s) consumed, {} leftover",
                    replayer.io_record_count()
                );
                return;
            }
            replayer.stop_at(n);
        }

        // Traces are self-contained: compile the embedded merged source.
        // No checker pass — the program already ran once to produce this.
        let source = replayer.source().to_string();
        let trace_features = replayer.features().to_vec();
        let trace_layout = replayer.source_layout().clone();
        let mut vm = match rad_vm::replay_compile::compile_trace_vm(
            &source,
            "embedded source",
            &trace_features,
            &trace_layout,
        ) {
            Ok(vm) => vm,
            Err(error) => {
                eprintln!("Error: {error}");
                process::exit(1);
            }
        };
        vm.enable_replay(replayer);

        let run_result = vm.run(0);
        // The VM decorates propagated errors with call-site context, so the
        // stop sentinel is matched anywhere in the message.
        let stopped_early = matches!(
            &run_result,
            Err(e) if e.contains(rad_vm::replay::REPLAY_STOP_PREFIX)
        );
        let replay_error = run_result.as_ref().err().map(String::as_str);
        let report = vm
            .finish_replay_with_outcome(replay_error)
            .expect("replay report");

        match run_result {
            Ok(()) => {}
            Err(e) if stopped_early => {
                eprintln!("{}", e);
            }
            Err(e) => {
                eprintln!("Runtime error (reproduced from trace): {}", e);
            }
        }

        eprintln!(
            "Replay: {} frame(s), {} io record(s) consumed, {} leftover",
            report.frames_replayed, report.io_replayed, report.leftover_io
        );
        if !stopped_early {
            if report.end_outcome_match == Some(false) {
                eprintln!("Replay DIVERGED: terminal success/error outcome does not match the recorded run");
                process::exit(1);
            }
            match report.end_digest_match {
                Some(true) => eprintln!("Replay verified: world digest matches the recorded run"),
                Some(false) => {
                    eprintln!(
                        "Replay DIVERGED: final world digest does not match the recorded run"
                    );
                    process::exit(1);
                }
                None => eprintln!("Trace carried no end digest; skipping final verification"),
            }
        }
        return;
    }

    if let CliCommand::SandboxServe {
        host_file,
        caps_file,
    } = command
    {
        // Validate the default caps grant up front so a typo'd caps file
        // fails at startup instead of on the first propose.
        let default_caps = match caps_file {
            Some(path) => {
                let text = match fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Error reading caps file {}: {}", path, e);
                        process::exit(1);
                    }
                };
                if let Err(e) = rad_vm::sandbox::SandboxCaps::from_json(&text) {
                    eprintln!("Invalid caps file {}: {}", path, e);
                    process::exit(1);
                }
                Some(text)
            }
            None => None,
        };

        // stdout is the protocol channel: the host VM's output is buffered
        // and dumped to stderr so the JSON-RPC stream stays clean.
        let mut vm = VM::new();
        vm.suppress_output();

        if let Some(filepath) = host_file {
            let loaded = match load_cli_program(&filepath, ParserOptions::default()) {
                Ok(loaded) => loaded,
                Err(errors) => {
                    eprintln!("{errors}");
                    process::exit(1);
                }
            };
            let analysis = analyze_cli_program(&loaded, &filepath, CheckerOptions::default());
            for error in &analysis.errors {
                eprintln!("{error}");
            }
            if analysis.has_errors() {
                process::exit(1);
            }
            let checked = analysis
                .semantic
                .into_checked()
                .expect("error-free host analysis must produce checked semantics");
            let source_identity = match checked_source_identity(&loaded) {
                Ok(identity) => identity,
                Err(error) => {
                    eprintln!("{error}");
                    process::exit(1);
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
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Compile error: {}", e.message);
                    process::exit(1);
                }
            };
            vm.load_compile_result(compile_result);
            if let Err(e) = vm.run(0) {
                eprintln!("Host program failed: {}", e);
                process::exit(1);
            }
            for line in vm.print_buffer.drain(..) {
                eprintln!("[host] {}", line);
            }
            eprintln!("[rad sandbox serve] host program loaded: {}", filepath);
        } else {
            eprintln!("[rad sandbox serve] serving with an empty world");
        }

        let mut server = rad_vm::sandbox_serve::SandboxServer::new(vm, default_caps);
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        if let Err(e) = server.serve(stdin.lock(), stdout.lock()) {
            eprintln!("sandbox serve: IO error: {}", e);
            process::exit(1);
        }
        return;
    }

    let CliCommand::Run {
        filepath,
        skip_check,
        compat_v0_5_dx,
        deny_warnings,
        warn_compat,
        strict_types,
        write_lock,
        profile_copies,
        serial_schedule,
        features,
        record,
        program_args,
    } = command
    else {
        unreachable!();
    };

    let parser_options = ParserOptions { compat_v0_5_dx };
    let loaded = match load_cli_program(&filepath, parser_options) {
        Ok(loaded) => loaded,
        Err(errors) => {
            eprintln!("{errors}");
            process::exit(1);
        }
    };

    let display_path = if loaded.had_imports {
        format!("<module graph from {}>", filepath)
    } else {
        filepath.clone()
    };
    if write_lock {
        let entry = Path::new(&filepath);
        let lock_path = entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("forge.lock");
        let lock = rad_vm::module_loader::LockFile::generate(&loaded.module_fingerprints);
        if let Err(e) = rad_vm::module_loader::write_lockfile(&lock_path.to_string_lossy(), &lock) {
            eprintln!("Warning: failed to write forge.lock: {}", e);
        }
    }

    let checked_semantics = if skip_check {
        for error in &loaded.errors {
            eprintln!(
                "{}",
                format_error(
                    &error.source,
                    &error.filepath,
                    &error.message,
                    error.line,
                    error.col,
                )
            );
        }
        if !loaded.errors.is_empty() {
            process::exit(1);
        }
        None
    } else {
        let analysis = analyze_cli_program(
            &loaded,
            &filepath,
            CheckerOptions {
                compat_v0_5_dx,
                warn_compat,
                strict_types,
                features: features.clone(),
            },
        );
        for error in &analysis.errors {
            eprintln!("{error}");
        }
        for warning in &analysis.warnings {
            eprintln!("{warning}");
        }
        if analysis.has_errors() || (deny_warnings && !analysis.warnings.is_empty()) {
            process::exit(1);
        }
        Some(
            analysis
                .semantic
                .into_checked()
                .expect("error-free semantic analysis must produce checked semantics"),
        )
    };

    let source_identity = match checked_source_identity(&loaded) {
        Ok(identity) => identity,
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    };
    let compile = if let Some(checked) = checked_semantics {
        rad_vm::pipeline::compile_checked_program(
            &loaded.program,
            loaded.aliases,
            checked,
            rad_vm::pipeline::CheckedCompileOptions {
                source_identity: Some(source_identity),
                ..rad_vm::pipeline::CheckedCompileOptions::default()
            },
        )
    } else {
        rad_vm::pipeline::compile_unchecked_program(
            &loaded.program,
            loaded.aliases,
            rad_vm::pipeline::UncheckedCompileOptions {
                features: features.clone(),
                source_identity: Some(source_identity),
                ..rad_vm::pipeline::UncheckedCompileOptions::default()
            },
        )
    };
    let compile_result = match compile {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                format_error(
                    &loaded.merged_source,
                    &display_path,
                    &e.message,
                    e.line,
                    e.col,
                )
            );
            process::exit(1);
        }
    };

    let mut vm = VM::new();
    vm.sys_args = program_args;
    vm.set_profile_copies(profile_copies);
    vm.set_serial_schedule(serial_schedule);
    if record.is_some() {
        // Hash the merged source (module graph included): a trace must only
        // replay against the exact program that produced it.
        vm.enable_recording_with_source_layout(
            &loaded.merged_source,
            &features,
            &loaded.source_layout,
        );
    }
    vm.load_compile_result(compile_result);

    let run_result = vm.run(0);

    // Write the trace even when the run failed: a trace of the crash is the
    // entire point of a time-travel debugger.
    if let Some(trace_path) = &record {
        if let Some(trace) =
            vm.take_trace_with_outcome(run_result.as_ref().err().map(String::as_str))
        {
            // RADPACK (D1): tapes are highly repetitive JSONL — pack them
            // with the raw-binary file envelope (no base64 tax; a tape is a
            // file, not a line-protocol payload). `rad replay` opens packed
            // and raw vintage tapes alike.
            let packed = rad_vm::radpack::seal_file("RADTRACE", &trace);
            if let Err(e) = std::fs::write(trace_path, packed) {
                eprintln!("Warning: failed to write trace to {}: {}", trace_path, e);
            } else {
                eprintln!("Recorded trace: {}", trace_path);
            }
        }
    }

    match run_result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            process::exit(1);
        }
    }
}
