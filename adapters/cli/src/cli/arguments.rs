

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
    format!(
        "Rad v{} — Bytecode Compiler & Virtual Machine

Usage: {program} <file.rad> [--no-check] [--compat-v0.5-dx|--no-compat-v0.5-dx] [--experimental-laws] [--deny-warnings] [--warn-compat|--no-warn-compat] [--strict-types] [--write-lock] [--profile-copies] [--serial-schedule] [--record <trace.radr>] [-- <program args>]
       {program} run [run options] [-- <program args>]   (entry from ./rad.toml)
       {program} relations check <file.rad> --experimental-relations [--module <id>]
       {program} new <name> [--template <template>]
       {program} snapshot [--update] [--create] [--experimental-laws] [dir]
       {program} play [--port <port>]
       {program} build [--target wasm] <input.rad> <output.wasm>
       {program} types <input.rad> <output.d.ts> [--feature <name>]
       {program} sandbox serve [host.rad] [--caps <caps.json>]
       {program} replay <trace.radr> [--to-frame <n>] [--serve] [--with <fixed.rad>] [--force]
       {program} fmt [--check] [file.rad...]
       {program} lint [--preset=strict] [file.rad...]
       {program} effects <callable> [--json] [--file <file.rad>]
       {program} writers <component|resource> [--json] [--file <file.rad>]
       {program} readers <component|resource> [--json] [--file <file.rad>]
       {program} path <from> -> <to> [--json] [--file <file.rad>]
       {program} query-plan <view|callable> [--json] [--file <file.rad>]
       {program} cost-path <callable> [--json] [--file <file.rad>]
       {program} why <entity> <component> [--json] [--file <file.rad>]
       {program} why-field <entity> <component> <field> [--json] [--file <file.rad>]
       {program} why-removed <entity> <component> [--json] [--file <file.rad>]
       {program} why-not-in-view <view> <entity> [--json] [--file <file.rad>]
       {program} bench <file.rad> [--json] [-- <program args>]
       {program} model-check <file.rad> [--model <name>] [--runs <n>] [--max-commands <n>] [--seed <n>] [--artifact-dir <dir>] [--json]
       {program} shrink <model-failure.radr>
       {program} ffi verify <plugin> [--contract <contract.json>] [--json]
       {program} test [dir]
       {program} lsp [--experimental-relations]
       {program} --version
       {program} --help",
        env!("CARGO_PKG_VERSION")
    )
}

fn wants_help(args: &[String]) -> bool {
    args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h")
}

fn parse_cli_args(args: &[String]) -> Result<CliCommand, String> {
    let program = args.first().map(String::as_str).unwrap_or("rad");

    if args.get(1).map(String::as_str) == Some("ffi") {
        if args.get(2).map(String::as_str) != Some("verify") {
            return Err(format!("Expected: {program} ffi verify <plugin> [--contract <contract.json>] [--json]"));
        }
        let mut plugin = None;
        let mut contract = None;
        let mut json = false;
        let mut index = 3;
        while index < args.len() {
            match args[index].as_str() {
                "--json" => {
                    json = true;
                    index += 1;
                }
                "--contract" if index + 1 < args.len() => {
                    contract = Some(args[index + 1].clone());
                    index += 2;
                }
                option if option.starts_with('-') => {
                    return Err(format!("Unknown option for ffi verify: {option}"));
                }
                argument => {
                    if plugin.replace(argument.to_string()).is_some() {
                        return Err(format!("Expected: {program} ffi verify <plugin> [--contract <contract.json>] [--json]"));
                    }
                    index += 1;
                }
            }
        }
        return Ok(CliCommand::FfiVerify {
            plugin: plugin
                .ok_or_else(|| format!("Expected: {program} ffi verify <plugin> [--contract <contract.json>] [--json]"))?,
            contract,
            json,
        });
    }

    if args.get(1).map(String::as_str) == Some("bench") {
        let mut json = false;
        let mut filepath = None;
        let mut program_args = Vec::new();
        let mut index = 2;
        while index < args.len() {
            let argument = &args[index];
            if argument == "--" {
                program_args.extend_from_slice(&args[index + 1..]);
                break;
            }
            if argument == "--json" {
                json = true;
            } else if argument.starts_with('-') {
                return Err(format!("Unknown option for bench: {argument}"));
            } else if filepath.replace(argument.clone()).is_some() {
                return Err(format!("Expected: {program} bench <file.rad> [--json] [-- <program args>]"));
            }
            index += 1;
        }
        let filepath = filepath
            .ok_or_else(|| format!("Expected: {program} bench <file.rad> [--json] [-- <program args>]"))?;
        return Ok(CliCommand::Bench {
            filepath,
            json,
            program_args,
        });
    }

    if args.get(1).map(String::as_str) == Some("model-check") {
        let mut filepath = None;
        let mut model = None;
        let mut runs = 10_000_u32;
        let mut max_commands = 200_u32;
        let mut seed = 1_u64;
        let mut artifact_directory = None;
        let mut json = false;
        let mut index = 2;
        while index < args.len() {
            let argument = &args[index];
            let mut take_value = |name: &str| -> Result<String, String> {
                let value = args.get(index + 1).ok_or_else(|| {
                    format!("{name} requires a value for model-check")
                })?;
                index += 2;
                Ok(value.clone())
            };
            match argument.as_str() {
                "--json" => {
                    json = true;
                    index += 1;
                }
                "--model" => model = Some(take_value("--model")?),
                "--runs" => {
                    runs = take_value("--runs")?
                        .parse()
                        .map_err(|_| "--runs requires a positive u32".to_string())?;
                }
                "--max-commands" => {
                    max_commands = take_value("--max-commands")?
                        .parse()
                        .map_err(|_| "--max-commands requires a positive u32".to_string())?;
                }
                "--seed" => {
                    seed = take_value("--seed")?
                        .parse()
                        .map_err(|_| "--seed requires a u64".to_string())?;
                }
                "--artifact-dir" => artifact_directory = Some(take_value("--artifact-dir")?),
                option if option.starts_with('-') => {
                    return Err(format!("Unknown option for model-check: {option}"));
                }
                value => {
                    if filepath.replace(value.to_string()).is_some() {
                        return Err(format!("Expected one model-check input file, got '{value}'"));
                    }
                    index += 1;
                }
            }
        }
        if runs == 0 || max_commands == 0 {
            return Err("--runs and --max-commands must be positive".to_string());
        }
        let filepath = filepath.ok_or_else(|| {
            format!("Expected: {program} model-check <file.rad> [options]")
        })?;
        return Ok(CliCommand::ModelCheck {
            filepath,
            model,
            runs,
            max_commands,
            seed,
            artifact_directory,
            json,
        });
    }

    if args.get(1).map(String::as_str) == Some("shrink") {
        if args.len() != 3 {
            return Err(format!("Expected: {program} shrink <model-failure.radr>"));
        }
        return Ok(CliCommand::ShrinkModel {
            artifact: args[2].clone(),
        });
    }

    if args.len() > 1
        && matches!(
            args[1].as_str(),
            "effects" | "writers" | "readers" | "path"
        )
    {
        let command = args[1].as_str();
        let mut json = false;
        let mut filepath = None;
        let mut positional = Vec::new();
        let mut index = 2;
        while index < args.len() {
            match args[index].as_str() {
                "--json" => {
                    json = true;
                    index += 1;
                }
                "--file" if index + 1 < args.len() => {
                    filepath = Some(args[index + 1].clone());
                    index += 2;
                }
                value if value.starts_with("--file=") => {
                    filepath = Some(value["--file=".len()..].to_string());
                    index += 1;
                }
                option if option.starts_with('-') && option != "->" => {
                    return Err(format!("Unknown option for {command}: {option}"));
                }
                value => {
                    positional.push(value.to_string());
                    index += 1;
                }
            }
        }
        if filepath.is_none()
            && positional
                .first()
                .is_some_and(|value| value.ends_with(".rad"))
        {
            filepath = Some(positional.remove(0));
        }
        let query = match command {
            "effects" if positional.len() == 1 => AuthorityQuery::Effects {
                symbol: positional.remove(0),
            },
            "writers" if positional.len() == 1 => AuthorityQuery::Writers {
                authority: positional.remove(0),
            },
            "readers" if positional.len() == 1 => AuthorityQuery::Readers {
                authority: positional.remove(0),
            },
            "path" => {
                positional.retain(|value| value != "->");
                if positional.len() != 2 {
                    return Err(format!(
                        "Expected: {program} path <from> -> <to> [--file <file.rad>]"
                    ));
                }
                AuthorityQuery::Path {
                    from: positional.remove(0),
                    to: positional.remove(0),
                }
            }
            _ => {
                return Err(format!(
                    "Expected: {program} {command} <name> [--json] [--file <file.rad>]"
                ));
            }
        };
        let filepath = match filepath {
            Some(filepath) => filepath,
            None => project_entry_from_rad_toml().map_err(|error| {
                format!(
                    "{error}\nUse `--file <file.rad>` outside a RAD project."
                )
            })?,
        };
        return Ok(CliCommand::Authority {
            query,
            filepath,
            json,
        });
    }

    if args.len() > 1
        && matches!(
            args[1].as_str(),
            "query-plan" | "cost-path" | "why" | "why-field" | "why-removed" | "why-not-in-view"
        )
    {
        let command = args[1].as_str();
        let mut json = false;
        let mut filepath = None;
        let mut positional = Vec::new();
        let mut index = 2;
        while index < args.len() {
            match args[index].as_str() {
                "--json" => {
                    json = true;
                    index += 1;
                }
                "--file" if index + 1 < args.len() => {
                    filepath = Some(args[index + 1].clone());
                    index += 2;
                }
                value if value.starts_with("--file=") => {
                    filepath = Some(value["--file=".len()..].to_string());
                    index += 1;
                }
                option if option.starts_with('-') => {
                    return Err(format!("Unknown option for {command}: {option}"));
                }
                value => {
                    positional.push(value.to_string());
                    index += 1;
                }
            }
        }
        let query = match command {
            "query-plan" if positional.len() == 1 => OperationalQuery::QueryPlan {
                target: positional.remove(0),
            },
            "cost-path" if positional.len() == 1 => OperationalQuery::CostPath {
                target: positional.remove(0),
            },
            "why" if positional.len() == 2 => OperationalQuery::Why {
                entity: positional.remove(0),
                component: positional.remove(0),
            },
            "why-field" if positional.len() == 3 => OperationalQuery::WhyField {
                entity: positional.remove(0),
                component: positional.remove(0),
                field: positional.remove(0),
            },
            "why-removed" if positional.len() == 2 => OperationalQuery::WhyRemoved {
                entity: positional.remove(0),
                component: positional.remove(0),
            },
            "why-not-in-view" if positional.len() == 2 => OperationalQuery::WhyNotInView {
                view: positional.remove(0),
                entity: positional.remove(0),
            },
            _ => {
                return Err(format!(
                    "Invalid arguments for {command}. See `{program} --help`."
                ));
            }
        };
        let filepath = match filepath {
            Some(filepath) => filepath,
            None => project_entry_from_rad_toml().map_err(|error| {
                format!("{error}\nUse `--file <file.rad>` outside a RAD project.")
            })?,
        };
        return Ok(CliCommand::Operational {
            query,
            filepath,
            json,
        });
    }

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

    if args.len() > 1 && args[1] == "types" {
        let mut positional = Vec::new();
        let mut features = Vec::new();
        let mut index = 2;
        while index < args.len() {
            match args[index].as_str() {
                "--feature" if index + 1 < args.len() => {
                    features.push(args[index + 1].clone());
                    index += 2;
                }
                value if value.starts_with("--feature=") => {
                    features.push(value["--feature=".len()..].to_string());
                    index += 1;
                }
                option if option.starts_with('-') => {
                    return Err(format!("Unknown option for types: {option}"));
                }
                value => {
                    positional.push(value.to_string());
                    index += 1;
                }
            }
        }
        if positional.len() != 2 || !positional[1].ends_with(".d.ts") {
            return Err(format!(
                "Expected: {program} types <input.rad> <output.d.ts> [--feature <name>]"
            ));
        }
        features.sort();
        features.dedup();
        return Ok(CliCommand::Types {
            input_rad: positional.remove(0),
            output_typescript: positional.remove(0),
            features,
        });
    }

    if args.len() > 1 && args[1] == "lsp" {
        let experimental_relations = args[2..]
            .iter()
            .any(|argument| argument == "--experimental-relations");
        if args[2..]
            .iter()
            .any(|argument| argument != "--experimental-relations")
        {
            return Err(format!("Unknown lsp option\n\n{}", usage(program)));
        }
        return Ok(CliCommand::Lsp {
            experimental_relations,
        });
    }

    if args.len() > 1 && args[1] == "relations" {
        if args.get(2).map(String::as_str) != Some("check") {
            return Err(format!(
                "Expected: {} relations check <file.rad> --experimental-relations [--module <id>]\n\n{}",
                program,
                usage(program)
            ));
        }
        let mut filepath = None;
        let mut module_id = "main".to_string();
        let mut experimental_relations = false;
        let mut index = 3;
        while index < args.len() {
            match args[index].as_str() {
                "--experimental-relations" => {
                    experimental_relations = true;
                    index += 1;
                }
                "--module" if index + 1 < args.len() => {
                    module_id = args[index + 1].clone();
                    index += 2;
                }
                option if option.starts_with('-') => {
                    return Err(format!("Unknown option for relations check: {option}"));
                }
                value if filepath.is_none() => {
                    filepath = Some(value.to_string());
                    index += 1;
                }
                value => return Err(format!("Unexpected relations check argument: {value}")),
            }
        }
        let filepath =
            filepath.ok_or_else(|| "relations check requires a .rad file".to_string())?;
        return Ok(CliCommand::RelationsCheck {
            filepath,
            module_id,
            experimental_relations,
        });
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
