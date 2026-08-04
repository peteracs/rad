

fn main() {
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

    if let CliCommand::RelationsCheck {
        filepath,
        module_id,
        experimental_relations,
    } = command
    {
        let source = match fs::File::open(&filepath) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("Error reading {filepath}: {error}");
                process::exit(1);
            }
        };
        let options = rad_vm::relation_frontend::FrontendOptions {
            enabled: experimental_relations,
            module_id,
            ..rad_vm::relation_frontend::FrontendOptions::default()
        };
        match rad_vm::relation_frontend::compile_reader(source, &options) {
            Ok(artifacts) => {
                println!(
                    "relations: {} schemas, {} rules, manifest {}",
                    artifacts.relations.schemas().len(),
                    artifacts.rules.len(),
                    hex::encode(artifacts.manifest_digest.as_bytes())
                );
            }
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
        return;
    }

    if let CliCommand::Fmt {
        filepaths,
        check_only,
    } = command
    {
        let mut changed = 0;

        let mut all_files = Vec::new();
        if filepaths.is_empty() {
            all_files.push(".".to_string());
        } else {
            all_files.extend(filepaths);
        }

        let mut rad_files = Vec::new();
        for target in all_files {
            let path = Path::new(&target);
            if path.is_file() && path.extension().is_some_and(|ext| ext == "rad") {
                rad_files.push(target);
            } else if path.is_dir() {
                let mut dirs = vec![path.to_path_buf()];
                while let Some(dir) = dirs.pop() {
                    if let Ok(entries) = fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            if entry_path.is_dir() {
                                let name =
                                    entry_path.file_name().unwrap_or_default().to_string_lossy();
                                if name != "node_modules" && name != "target" {
                                    dirs.push(entry_path);
                                }
                            } else if entry_path.is_file()
                                && entry_path.extension().is_some_and(|ext| ext == "rad")
                            {
                                rad_files.push(entry_path.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }

        if rad_files.is_empty() {
            println!("No .rad files found");
            return;
        }

        for filepath in &rad_files {
            let source = match fs::read_to_string(filepath) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading {}: {}", filepath, e);
                    continue;
                }
            };
            let formatted = rad_vm::formatter::format_rad(&source);
            if formatted != source {
                if check_only {
                    println!("  needs formatting: {}", filepath);
                    changed += 1;
                } else {
                    if let Err(e) = fs::write(filepath, &formatted) {
                        eprintln!("Error writing {}: {}", filepath, e);
                    } else {
                        println!("  formatted {}", filepath);
                        changed += 1;
                    }
                }
            } else if !check_only {
                println!("  unchanged {}", filepath);
            }
        }
        if check_only && changed > 0 {
            println!(
                "\n{} file(s) need formatting. Run `rad fmt` to fix.",
                changed
            );
            process::exit(1);
        }
        return;
    }

    if let CliCommand::Lint {
        filepaths,
        preset,
        boundaries,
    } = command
    {
        let mut all_files = Vec::new();
        if filepaths.is_empty() {
            all_files.push(".".to_string());
        } else {
            all_files.extend(filepaths);
        }

        let mut rad_files = Vec::new();
        for target in all_files {
            let path = Path::new(&target);
            if path.is_file() && path.extension().is_some_and(|ext| ext == "rad") {
                rad_files.push(target);
            } else if path.is_dir() {
                let mut dirs = vec![path.to_path_buf()];
                while let Some(dir) = dirs.pop() {
                    if let Ok(entries) = fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            if entry_path.is_dir() {
                                let name =
                                    entry_path.file_name().unwrap_or_default().to_string_lossy();
                                if name != "node_modules" && name != "target" {
                                    dirs.push(entry_path);
                                }
                            } else if entry_path.is_file()
                                && entry_path.extension().is_some_and(|ext| ext == "rad")
                            {
                                rad_files.push(entry_path.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }

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
                let mut checker = Checker::new_with_options(CheckerOptions {
                    compat_v0_5_dx: false,
                    warn_compat: preset_data.vm_flags.contains(&"--warn-compat"),
                    strict_types: preset_data.vm_flags.contains(&"--strict-types"),
                    features: vec![],
                });
                let errors = checker.check(&r.program);
                let warnings = checker.warnings();

                ast_issues = rad_vm::linter::lint_ast(
                    &r.program,
                    &checker,
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

    #[cfg(not(target_arch = "wasm32"))]
    if let CliCommand::Lsp {
        experimental_relations,
    } = command
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();

            let (service, socket) = tower_lsp::LspService::new(|client| rad_vm::lsp::LspBackend {
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
        let parser_options = ParserOptions {
            compat_v0_5_dx: false,
        };
        let (program, source, had_imports, source_map, _module_fingerprints, aliases, parse_errors) =
            match load_program_with_source_map_and_options(&input_rad, parser_options) {
                Ok(r) => (
                    r.program,
                    r.merged_source,
                    r.had_imports,
                    r.source_map,
                    r.module_fingerprints,
                    r.aliases,
                    r.errors,
                ),
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

        let mut has_errors = false;
        for e in &parse_errors {
            eprintln!(
                "{}",
                format_error(&e.source, &e.filepath, &e.message, e.line, e.col)
            );
            has_errors = true;
        }

        let display_path = if had_imports {
            format!("<module graph from {}>", input_rad)
        } else {
            input_rad.clone()
        };

        let mut checker = Checker::new_with_options(CheckerOptions {
            compat_v0_5_dx: false,
            warn_compat: true,
            strict_types: false,
            features: vec![],
        });
        checker.set_aliases(aliases.clone());
        let errors = checker.check(&program);
        if !errors.is_empty() {
            for err in &errors {
                let (src, path) =
                    resolve_source_for_error(err.file, &source_map, &source, &display_path);
                eprintln!(
                    "{}",
                    format_error(src, path, &err.message, err.line, err.col)
                );
                if let Some(hint) = &err.hint {
                    eprintln!("  hint: {}", hint);
                }
            }
            has_errors = true;
        }
        let warnings = checker.warnings();
        for warning in &warnings {
            let (src, path) =
                resolve_source_for_error(warning.file, &source_map, &source, &display_path);
            eprintln!(
                "{}",
                format_warning(src, path, &warning.message, warning.line, warning.col)
            );
            if let Some(hint) = &warning.hint {
                eprintln!("  hint: {}", hint);
            }
        }

        if has_errors {
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
            let parser_options = ParserOptions {
                compat_v0_5_dx: false,
            };
            let loaded = match load_program_with_source_map_and_options(&filepath, parser_options) {
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
            let mut has_errors = false;
            for e in &loaded.errors {
                eprintln!(
                    "{}",
                    format_error(&e.source, &e.filepath, &e.message, e.line, e.col)
                );
                has_errors = true;
            }
            let mut checker = Checker::new_with_options(CheckerOptions {
                compat_v0_5_dx: false,
                warn_compat: true,
                strict_types: false,
                features: vec![],
            });
            checker.set_aliases(loaded.aliases.clone());
            for err in checker.check(&loaded.program) {
                eprintln!("Error: {}", err.message);
                has_errors = true;
            }
            if has_errors {
                process::exit(1);
            }
            let compile_result = match Compiler::new()
                .with_checker_output(checker.output())
                .with_program_source_identity(
                    loaded
                        .source_layout
                        .digest(&loaded.merged_source)
                        .expect("module loader produced an invalid source layout"),
                )
                .compile(&loaded.program)
            {
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
    let (
        program,
        source,
        source_layout,
        had_imports,
        source_map,
        module_fingerprints,
        aliases,
        parse_errors,
    ) = match load_program_with_source_map_and_options(&filepath, parser_options) {
        Ok(r) => (
            r.program,
            r.merged_source,
            r.source_layout,
            r.had_imports,
            r.source_map,
            r.module_fingerprints,
            r.aliases,
            r.errors,
        ),
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

    let mut has_errors = false;
    for e in &parse_errors {
        eprintln!(
            "{}",
            format_error(&e.source, &e.filepath, &e.message, e.line, e.col)
        );
        has_errors = true;
    }

    let display_path = if had_imports {
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
        let lock = rad_vm::module_loader::LockFile::generate(&module_fingerprints);
        if let Err(e) = rad_vm::module_loader::write_lockfile(&lock_path.to_string_lossy(), &lock) {
            eprintln!("Warning: failed to write forge.lock: {}", e);
        }
    }

    let mut checker_output = rad_vm::types::CheckerOutput::default();
    if !skip_check {
        let mut checker = Checker::new_with_options(CheckerOptions {
            compat_v0_5_dx,
            warn_compat,
            strict_types,
            features: features.clone(),
        });
        checker.set_aliases(aliases.clone());
        let errors = checker.check(&program);
        checker_output = checker.output();
        if !errors.is_empty() {
            for err in &errors {
                let (src, path) =
                    resolve_source_for_error(err.file, &source_map, &source, &display_path);
                eprintln!(
                    "{}",
                    format_error(src, path, &err.message, err.line, err.col)
                );
                if let Some(hint) = &err.hint {
                    eprintln!("  hint: {}", hint);
                }
            }
            has_errors = true;
        }
        let warnings = checker.warnings();
        for warning in &warnings {
            let (src, path) =
                resolve_source_for_error(warning.file, &source_map, &source, &display_path);
            eprintln!(
                "{}",
                format_warning(src, path, &warning.message, warning.line, warning.col)
            );
            if let Some(hint) = &warning.hint {
                eprintln!("  hint: {}", hint);
            }
        }
        if deny_warnings && !warnings.is_empty() {
            process::exit(1);
        }
    }

    if has_errors {
        process::exit(1);
    }

    let compiler = Compiler::new()
        .with_checker_output(checker_output)
        .with_aliases(aliases)
        .with_features(features.clone())
        .with_program_source_identity(
            source_layout
                .digest(&source)
                .expect("module loader produced an invalid source layout"),
        );
    let compile_result = match compiler.compile(&program) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                format_error(&source, &display_path, &e.message, e.line, e.col)
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
        vm.enable_recording_with_source_layout(&source, &features, &source_layout);
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