/// Options of `rad blame`, as typed; `run_blame_command` gives them meaning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BlameArguments {
    output: Option<String>,
    error: Option<String>,
    with_source: Option<String>,
    why: Option<(String, String)>,
    why_resource: Option<String>,
    plain_ddmin: bool,
    no_but_for: bool,
    jobs: Option<usize>,
    max_replays: Option<usize>,
    write_script: Option<String>,
    json: bool,
    force: bool,
}

fn parse_blame_arguments(args: &[String], program: &str) -> Result<CliCommand, String> {
    let fail = |message: String| format!("{message}\n\n{}", usage(program));
    let mut trace_path = None;
    let mut parsed = BlameArguments::default();
    let mut i = 2;
    let value = |i: &mut usize, option: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| fail(format!("Missing argument for {option}")))
    };
    let count = |text: String, option: &str| -> Result<usize, String> {
        match text.parse::<usize>() {
            Ok(n) if n > 0 => Ok(n),
            _ => Err(fail(format!(
                "{option} expects a positive number, got '{text}'"
            ))),
        }
    };
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--json" => parsed.json = true,
            "--force" => parsed.force = true,
            "--no-but-for" => parsed.no_but_for = true,
            "--output" => parsed.output = Some(value(&mut i, arg)?),
            "--error" => parsed.error = Some(value(&mut i, arg)?),
            "--with" => parsed.with_source = Some(value(&mut i, arg)?),
            "--write-script" => parsed.write_script = Some(value(&mut i, arg)?),
            "--why-resource" => parsed.why_resource = Some(value(&mut i, arg)?),
            "--why" => {
                let entity = value(&mut i, arg)?;
                let component = value(&mut i, arg)?;
                parsed.why = Some((entity, component));
            }
            "--strategy" => match value(&mut i, arg)?.as_str() {
                "guided" => parsed.plain_ddmin = false,
                "ddmin" => parsed.plain_ddmin = true,
                other => {
                    return Err(fail(format!(
                        "--strategy expects 'guided' or 'ddmin', got '{other}'"
                    )))
                }
            },
            "--jobs" => parsed.jobs = Some(count(value(&mut i, arg)?, arg)?),
            "--max-replays" => parsed.max_replays = Some(count(value(&mut i, arg)?, arg)?),
            _ if arg.starts_with('-') => return Err(fail(format!("Unknown option: {arg}"))),
            _ if trace_path.is_none() => trace_path = Some(arg.to_string()),
            _ => return Err(fail(format!("Unexpected argument: {arg}"))),
        }
        i += 1;
    }
    if parsed.output.is_some() && parsed.error.is_some() {
        return Err(fail(
            "--output and --error name two symptoms; give one".into(),
        ));
    }
    if parsed.why.is_some() && parsed.why_resource.is_some() {
        return Err(fail(
            "--why and --why-resource name two lineage targets; give one".into(),
        ));
    }
    let trace_path = trace_path.ok_or_else(|| fail("Missing trace".into()))?;
    Ok(CliCommand::Blame {
        trace_path,
        arguments: parsed,
    })
}

/// `rad blame <trace>`: which recorded inputs cause a symptom, found by
/// replaying the session with inputs withheld. The report goes to stdout.
fn run_blame_command(trace_path: &str, arguments: BlameArguments) {
    use rad_vm::blame::{BlameOptions, EditedProgram, LineageTarget, Strategy, Symptom};
    let fail = |message: String| -> ! {
        eprintln!("Error: {message}");
        process::exit(1);
    };
    let trace_bytes = fs::read(trace_path)
        .unwrap_or_else(|error| fail(format!("reading trace {trace_path}: {error}")));
    let trace_text = rad_vm::radpack::open_file(&trace_bytes)
        .unwrap_or_else(|error| fail(format!("unpacking trace {trace_path}: {error}")));

    let edited = arguments.with_source.as_deref().map(|path| {
        let loaded = match load_cli_program(path, ParserOptions) {
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
        EditedProgram {
            source: loaded.merged_source,
            layout: loaded.source_layout,
            description: path.to_string(),
        }
    });

    let defaults = BlameOptions::default();
    let options = BlameOptions {
        symptom: match (arguments.output, arguments.error) {
            (Some(text), _) => Some(Symptom::Output(text)),
            (None, Some(text)) => Some(Symptom::Error(text)),
            (None, None) => None,
        },
        lineage: match (arguments.why, arguments.why_resource) {
            (Some((entity, component)), _) => Some(LineageTarget::Component { entity, component }),
            (None, Some(resource)) => Some(LineageTarget::Resource(resource)),
            (None, None) => None,
        },
        strategy: if arguments.plain_ddmin {
            Strategy::Ddmin
        } else {
            Strategy::Guided
        },
        but_for: !arguments.no_but_for,
        jobs: arguments.jobs.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get().min(8))
                .unwrap_or(1)
        }),
        max_replays: arguments.max_replays.unwrap_or(defaults.max_replays),
        force: arguments.force,
    };

    let report =
        rad_vm::blame::blame(&trace_text, edited, &options).unwrap_or_else(|error| fail(error));

    if let Some(path) = &arguments.write_script {
        let script = report.minimal_script().unwrap_or_else(|| {
            fail("--write-script needs a minimal cause made of text lines; this one holds other values".into())
        });
        fs::write(path, script).unwrap_or_else(|error| fail(format!("writing {path}: {error}")));
        eprintln!("Minimal script written: {path}");
    }
    if arguments.json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.render_text(trace_path));
    }
}
