use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process;

use rad_vm::ast::SourceMap;
use rad_vm::checker::{Checker, CheckerOptions};
use rad_vm::compiler::Compiler;
use rad_vm::module_loader::load_program_with_source_map_and_options;
use rad_vm::parser::ParserOptions;
use rad_vm::vm::VM;

#[derive(Debug)]
enum CliCommand {
    Run {
        filepath: String,
        skip_check: bool,
        compat_v0_5_dx: bool,
        deny_warnings: bool,
        warn_compat: bool,
        strict_types: bool,
        write_lock: bool,
        profile_copies: bool,
        serial_schedule: bool,
        features: Vec<String>,
        record: Option<String>,
        program_args: Vec<String>,
    },
    Fmt {
        filepaths: Vec<String>,
        check_only: bool,
    },
    Lint {
        filepaths: Vec<String>,
        preset: String,
        boundaries: HashMap<String, Vec<String>>,
    },
    Lsp,
    Test {
        test_dir: String,
    },
    New {
        project_name: Option<String>,
        template: Option<String>,
        list_templates: bool,
    },
    Snapshot {
        directory: Option<String>,
        update: bool,
        create: bool,
        experimental_laws: bool,
    },
    Play {
        port: u16,
    },
    Build {
        input_rad: String,
        output_wasm: String,
    },
    SandboxServe {
        host_file: Option<String>,
        caps_file: Option<String>,
    },
    Replay {
        trace_path: String,
        to_frame: Option<u64>,
        force: bool,
        serve: bool,
        with_source: Option<String>,
    },
    Version,
}

/// Entry point of the project in the current directory, per `rad.toml`
/// (`[build] entry = "..."`); defaults to src/main.rad when the key is
/// absent. The single-purpose line scan avoids a toml dependency for one key.
fn project_entry_from_rad_toml() -> Result<String, String> {
    let toml = fs::read_to_string("rad.toml").map_err(|_| {
        "rad run: no rad.toml in this directory (create a project with `rad new <name>` \
         or run a file directly: rad <file.rad>)"
            .to_string()
    })?;
    for line in toml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("entry") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let v = rest.trim().trim_matches('"');
                if !v.is_empty() {
                    return Ok(v.to_string());
                }
            }
        }
    }
    Ok("src/main.rad".to_string())
}

fn usage(program: &str) -> String {
    let mut output = format!(
        "Rad v{} — Bytecode Compiler & Virtual Machine\n\nUsage: {} <file.rad> [--no-check] [--compat-v0.5-dx|--no-compat-v0.5-dx] [--experimental-laws] [--deny-warnings] [--warn-compat|--no-warn-compat] [--strict-types] [--write-lock] [--profile-copies] [--serial-schedule] [--record <trace.radr>] [-- <program args>]\n       {} run [run options] [-- <program args>]   (entry from ./rad.toml)\n       {} new <name> [--template <template>]\n       {} snapshot [--update] [--create] [--experimental-laws] [dir]\n       {} play [--port <port>]\n       {} build [--target wasm] <input.rad> <output.wasm>\n       {} sandbox serve [host.rad] [--caps <caps.json>]\n       {} replay <trace.radr> [--to-frame <n>] [--serve] [--with <fixed.rad>] [--force]\n       {} fmt [--check] [file.rad...]\n       {} lint [--preset=strict] [file.rad...]\n       {} test [dir]\n       {} lsp\n       {} --version",
        env!("CARGO_PKG_VERSION"), program, program, program, program, program, program, program, program, program, program, program, program, program
    );
    output.push_str(&format!("\n       {} --help", program));
    output
}

fn wants_help(args: &[String]) -> bool {
    args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h")
}

fn parse_cli_args(args: &[String]) -> Result<CliCommand, String> {
    let program = args.first().map(String::as_str).unwrap_or("rad");

    if args.len() > 1 && args[1] == "fmt" {
        let mut check_only = false;
        let mut filepaths = Vec::new();
        for arg in args.iter().skip(2) {
            if arg == "--check" {
                check_only = true;
            } else if arg.starts_with('-') {
                return Err(format!(
                    "Unknown option for fmt: {}\n\n{}",
                    arg,
                    usage(program)
                ));
            } else {
                filepaths.push(arg.to_string());
            }
        }
        return Ok(CliCommand::Fmt {
            filepaths,
            check_only,
        });
    }

    if args.len() > 1 && args[1] == "lint" {
        let mut preset = "standard".to_string();
        let mut filepaths = Vec::new();
        let mut boundaries = HashMap::new();
        let mut i = 2;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--preset" && i + 1 < args.len() {
                preset = args[i + 1].clone();
                i += 2;
            } else if arg.starts_with("--preset=") {
                preset = arg.split('=').nth(1).unwrap_or("standard").to_string();
                i += 1;
            } else if arg == "--boundary" && i + 1 < args.len() {
                let parts: Vec<&str> = args[i + 1].split('=').collect();
                if parts.len() == 2 {
                    let module = parts[0].to_string();
                    let deps = parts[1].split(',').map(|s| s.to_string()).collect();
                    boundaries.insert(module, deps);
                }
                i += 2;
            } else if arg.starts_with('-') {
                return Err(format!(
                    "Unknown option for lint: {}\n\n{}",
                    arg,
                    usage(program)
                ));
            } else {
                filepaths.push(arg.to_string());
                i += 1;
            }
        }
        return Ok(CliCommand::Lint {
            filepaths,
            preset,
            boundaries,
        });
    }

    if args.len() > 1 && args[1] == "test" {
        let test_dir = if args.len() > 2 {
            args[2].clone()
        } else {
            "tests".to_string()
        };
        return Ok(CliCommand::Test { test_dir });
    }

    if args.len() > 1 && args[1] == "lsp" {
        return Ok(CliCommand::Lsp);
    }

    if args.len() > 1 && args[1] == "new" {
        let mut project_name = None;
        let mut template = None;
        let mut list_templates = false;
        let mut i = 2;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--template" && i + 1 < args.len() {
                template = Some(args[i + 1].clone());
                i += 2;
            } else if arg == "--list-templates" {
                list_templates = true;
                i += 1;
            } else if arg.starts_with('-') {
                return Err(format!(
                    "Unknown option for new: {}\n\n{}",
                    arg,
                    usage(program)
                ));
            } else {
                project_name = Some(arg.to_string());
                i += 1;
            }
        }
        return Ok(CliCommand::New {
            project_name,
            template,
            list_templates,
        });
    }

    if args.len() > 1 && args[1] == "snapshot" {
        let mut directory = None;
        let mut update = false;
        let mut create = false;
        let mut experimental_laws = false;
        for arg in args.iter().skip(2) {
            if arg == "--update" {
                update = true;
            } else if arg == "--create" {
                create = true;
            } else if arg == "--experimental-laws" {
                experimental_laws = true;
            } else if arg.starts_with('-') {
                return Err(format!(
                    "Unknown option for snapshot: {}\n\n{}",
                    arg,
                    usage(program)
                ));
            } else {
                directory = Some(arg.to_string());
            }
        }
        return Ok(CliCommand::Snapshot {
            directory,
            update,
            create,
            experimental_laws,
        });
    }

    if args.len() > 1 && args[1] == "play" {
        let mut port = 8080;
        let mut i = 2;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--port" && i + 1 < args.len() {
                if let Ok(p) = args[i + 1].parse::<u16>() {
                    port = p;
                }
                i += 2;
            } else if arg.starts_with('-') {
                return Err(format!(
                    "Unknown option for play: {}\n\n{}",
                    arg,
                    usage(program)
                ));
            } else {
                i += 1;
            }
        }
        return Ok(CliCommand::Play { port });
    }

    if args.len() > 1 && args[1] == "sandbox" {
        if args.len() < 3 || args[2] != "serve" {
            return Err(format!(
                "Expected: {} sandbox serve [host.rad] [--caps <caps.json>]\n\n{}",
                program,
                usage(program)
            ));
        }
        let mut host_file = None;
        let mut caps_file = None;
        let mut i = 3;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--caps" && i + 1 < args.len() {
                caps_file = Some(args[i + 1].clone());
                i += 2;
            } else if let Some(rest) = arg.strip_prefix("--caps=") {
                caps_file = Some(rest.to_string());
                i += 1;
            } else if arg.starts_with('-') {
                return Err(format!(
                    "Unknown option for sandbox serve: {}\n\n{}",
                    arg,
                    usage(program)
                ));
            } else {
                host_file = Some(arg.to_string());
                i += 1;
            }
        }
        return Ok(CliCommand::SandboxServe {
            host_file,
            caps_file,
        });
    }

    if args.len() > 1 && args[1] == "replay" {
        let mut trace_path = None;
        let mut to_frame = None;
        let mut force = false;
        let mut serve = false;
        let mut with_source = None;
        let mut i = 2;
        while i < args.len() {
            let arg = &args[i];
            match arg.as_str() {
                "--force" => force = true,
                "--serve" => serve = true,
                "--with" => {
                    i += 1;
                    let p = args.get(i).ok_or_else(|| {
                        format!("Missing argument for --with\n\n{}", usage(program))
                    })?;
                    with_source = Some(p.to_string());
                }
                "--to-frame" => {
                    i += 1;
                    let n = args.get(i).ok_or_else(|| {
                        format!("Missing argument for --to-frame\n\n{}", usage(program))
                    })?;
                    to_frame = Some(n.parse::<u64>().map_err(|_| {
                        format!(
                            "--to-frame expects a number, got '{}'\n\n{}",
                            n,
                            usage(program)
                        )
                    })?);
                }
                _ if arg.starts_with("--to-frame=") => {
                    let n = &arg["--to-frame=".len()..];
                    to_frame = Some(n.parse::<u64>().map_err(|_| {
                        format!(
                            "--to-frame expects a number, got '{}'\n\n{}",
                            n,
                            usage(program)
                        )
                    })?);
                }
                _ if arg.starts_with("--with=") => {
                    with_source = Some(arg["--with=".len()..].to_string());
                }
                _ if arg.starts_with('-') => {
                    return Err(format!("Unknown option: {}\n\n{}", arg, usage(program)));
                }
                _ if trace_path.is_none() => trace_path = Some(arg.to_string()),
                _ => {
                    return Err(format!(
                        "Unexpected argument: {}\n\n{}",
                        arg,
                        usage(program)
                    ));
                }
            }
            i += 1;
        }
        let trace_path = trace_path.ok_or_else(|| {
            format!(
                "Expected: {} replay <trace.radr> [--to-frame <n>] [--serve] [--with <fixed.rad>] [--force]\n\n{}",
                program,
                usage(program)
            )
        })?;
        return Ok(CliCommand::Replay {
            trace_path,
            to_frame,
            force,
            serve,
            with_source,
        });
    }

    if args.len() > 1 && args[1] == "build" {
        let mut target = "wasm".to_string();
        let mut rest = Vec::new();
        let mut i = 2;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--target" && i + 1 < args.len() {
                target = args[i + 1].clone();
                i += 2;
            } else if arg.starts_with('-') {
                return Err(format!(
                    "Unknown option for build: {}\n\n{}",
                    arg,
                    usage(program)
                ));
            } else {
                rest.push(arg.clone());
                i += 1;
            }
        }
        if rest.len() != 2 {
            return Err(format!(
                "Expected: {} build [--target wasm] <input.rad> <output.wasm>\n\n{}",
                program,
                usage(program)
            ));
        }
        if target != "wasm" {
            return Err(format!(
                "only --target wasm is supported (got {:?})",
                target
            ));
        }
        return Ok(CliCommand::Build {
            input_rad: rest[0].clone(),
            output_wasm: rest[1].clone(),
        });
    }

    // `rad run` — what `rad new` tells people to type. Resolves the entry
    // point from ./rad.toml and falls through to ordinary run parsing, so
    // every flag (and `-- <program args>`) works unchanged.
    if args.len() > 1 && args[1] == "run" {
        let entry = project_entry_from_rad_toml()?;
        let mut rewritten = vec![args[0].clone(), entry];
        rewritten.extend(args.iter().skip(2).cloned());
        return parse_cli_args(&rewritten);
    }

    let mut skip_check = false;
    let mut compat_v0_5_dx = false;
    let mut deny_warnings = false;
    let mut warn_compat = true;
    let mut strict_types = false;
    let mut write_lock = false;
    let mut profile_copies = false;
    let mut serial_schedule = false;
    let mut want_version = false;
    let mut features = Vec::new();
    let mut record: Option<String> = None;
    let mut positional: Vec<&str> = Vec::new();
    let mut program_args: Vec<String> = Vec::new();

    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            // Everything after `--` belongs to the program (sys_args()).
            "--" => {
                program_args = it.map(String::clone).collect();
                break;
            }
            "--no-check" => skip_check = true,
            "--compat-v0.5-dx" => compat_v0_5_dx = true,
            "--no-compat-v0.5-dx" => compat_v0_5_dx = false,
            "--deny-warnings" => deny_warnings = true,
            "--warn-compat" => warn_compat = true,
            "--no-warn-compat" => warn_compat = false,
            "--strict-types" => strict_types = true,
            "--write-lock" => write_lock = true,
            "--profile-copies" => profile_copies = true,
            "--serial-schedule" => serial_schedule = true,
            "--experimental-laws" => features.push("causal_laws".to_string()),
            "--version" => want_version = true,
            "--feature" => {
                if let Some(f) = it.next() {
                    features.push(f.clone());
                } else {
                    return Err(format!(
                        "Missing argument for --feature\n\n{}",
                        usage(program)
                    ));
                }
            }
            "--record" => {
                if let Some(p) = it.next() {
                    record = Some(p.clone());
                } else {
                    return Err(format!(
                        "Missing argument for --record\n\n{}",
                        usage(program)
                    ));
                }
            }
            _ if arg.starts_with("--record=") => {
                record = Some(arg["--record=".len()..].to_string());
            }
            _ if arg.starts_with("--feature=") => {
                features.push(arg.split('=').nth(1).unwrap_or("").to_string());
            }
            _ if arg.starts_with('-') => {
                return Err(format!("Unknown option: {}\n\n{}", arg, usage(program)));
            }
            _ => positional.push(arg),
        }
    }

    if want_version && positional.is_empty() {
        return Ok(CliCommand::Version);
    }
    if positional.len() != 1 {
        return Err(usage(program));
    }
    if want_version {
        return Err(format!(
            "--version cannot be combined with input files\n\n{}",
            usage(program)
        ));
    }

    Ok(CliCommand::Run {
        filepath: positional[0].to_string(),
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
    })
}

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
    if let CliCommand::Lsp = command {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();

            let (service, socket) = tower_lsp::LspService::new(|client| rad_vm::lsp::LspBackend {
                client,
                documents: tokio::sync::RwLock::new(std::collections::HashMap::new()),
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
        let mut lexer = rad_vm::lexer::Lexer::new(&source);
        let tokens = lexer.tokenize().0;
        let mut parser = rad_vm::parser::Parser::new(tokens);
        let program = parser.parse();
        if !parser.errors().is_empty() {
            for e in parser.errors() {
                eprintln!("Error parsing embedded source: {}", e.message);
            }
            process::exit(1);
        }
        let compile_result = match Compiler::new().compile(&program) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error compiling embedded source: {}", e.message);
                process::exit(1);
            }
        };

        let mut vm = VM::new();
        vm.enable_replay(replayer);
        vm.load_compile_result(compile_result);

        let run_result = vm.run(0);
        // The VM decorates propagated errors with call-site context, so the
        // stop sentinel is matched anywhere in the message.
        let stopped_early = matches!(
            &run_result,
            Err(e) if e.contains(rad_vm::replay::REPLAY_STOP_PREFIX)
        );
        let report = vm.finish_replay().expect("replay report");

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
    let (program, source, had_imports, source_map, module_fingerprints, aliases, parse_errors) =
        match load_program_with_source_map_and_options(&filepath, parser_options) {
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
        .with_features(features);
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
        vm.enable_recording(&source);
    }
    vm.load_compile_result(compile_result);

    let run_result = vm.run(0);

    // Write the trace even when the run failed: a trace of the crash is the
    // entire point of a time-travel debugger.
    if let Some(trace_path) = &record {
        if let Some(trace) = vm.take_trace() {
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
    let mut vm_a = match compile_into_vm(&original_source, "embedded source") {
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
    let compile_result = match Compiler::new().compile(&loaded.program) {
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
    let report = vm_b.finish_replay().expect("retro report");

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
            .with_aliases(loaded.aliases);
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

fn compile_into_vm(source: &str, what: &str) -> Result<VM, String> {
    let mut lexer = rad_vm::lexer::Lexer::new(source);
    let tokens = lexer.tokenize().0;
    let mut parser = rad_vm::parser::Parser::new(tokens);
    let program = parser.parse();
    if let Some(e) = parser.errors().first() {
        return Err(format!("{} failed to parse: {}", what, e.message));
    }
    let compile_result = Compiler::new()
        .compile(&program)
        .map_err(|e| format!("{} failed to compile: {}", what, e.message))?;
    let mut vm = VM::new();
    vm.load_compile_result(compile_result);
    Ok(vm)
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
            CliCommand::Lsp => panic!("expected run command"),
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
            CliCommand::Lsp => panic!("expected run command"),
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
            CliCommand::Lsp => panic!("expected run command"),
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
            CliCommand::Lsp => panic!("expected run command"),
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
            CliCommand::Lsp => panic!("expected run command"),
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
            CliCommand::Lsp => panic!("expected run command"),
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
            CliCommand::Lsp => panic!("expected run command"),
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
