fn run_authority_command(query: AuthorityQuery, filepath: String, json: bool) {
    let loaded = match load_program_with_source_map_and_options(&filepath, ParserOptions::default()) {
        Ok(loaded) => loaded,
        Err(errors) => {
            for error in errors {
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
            process::exit(1);
        }
    };
    let mut had_errors = false;
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
        had_errors = true;
    }
    let mut checker = Checker::new();
    checker.set_aliases(loaded.aliases);
    for error in checker.check(&loaded.program) {
        let (source, path) = resolve_source_for_error(
            error.file,
            &loaded.source_map,
            &loaded.merged_source,
            &filepath,
        );
        eprintln!(
            "{}",
            format_error(source, path, &error.message, error.line, error.col)
        );
        if let Some(hint) = error.hint {
            eprintln!("  hint: {hint}");
        }
        had_errors = true;
    }
    let report = checker.output().authority;
    let query_result = match query {
        AuthorityQuery::Effects { symbol } => report.resolve(&symbol).map(|callable| {
            if json {
                serde_json::to_string_pretty(callable).expect("authority report is serializable")
            } else {
                render_callable_effects(callable, &report)
            }
        }),
        AuthorityQuery::Writers { authority } => {
            let names = report.writers_of(&authority);
            Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "authority": authority,
                    "writers": names,
                }))
                .expect("writer report is serializable")
            } else {
                render_reverse_query("writers", &authority, &names)
            })
        }
        AuthorityQuery::Readers { authority } => {
            let names = report.readers_of(&authority);
            Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "authority": authority,
                    "readers": names,
                }))
                .expect("reader report is serializable")
            } else {
                render_reverse_query("readers", &authority, &names)
            })
        }
        AuthorityQuery::Path { from, to } => report.path(&from, &to).map(|path| {
            if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "from": from,
                    "to": to,
                    "path": path,
                }))
                .expect("path report is serializable")
            } else {
                path.map_or_else(
                    || format!("no authority path from {from} to {to}"),
                    |nodes| nodes.join(" -> "),
                )
            }
        }),
    };
    match query_result {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("authority query: {error}");
            process::exit(1);
        }
    }
    if had_errors {
        process::exit(1);
    }
}

fn render_callable_effects(
    callable: &rad_vm::types::CallableAuthority,
    report: &rad_vm::types::AuthorityReport,
) -> String {
    let calls = callable
        .calls
        .iter()
        .filter_map(|name| report.callables.get(name))
        .map(|target| target.display_name.as_str())
        .collect::<Vec<_>>();
    let deferred = callable
        .deferred_calls
        .iter()
        .filter_map(|name| report.callables.get(name))
        .map(|target| target.display_name.as_str())
        .collect::<Vec<_>>();
    format!(
        "effect {} {{\n    kind: {}\n    direct:      {}\n    synchronous: {}\n    transitive:  {}\n    calls:       {}\n    deferred:    {}\n}}",
        callable.display_name,
        callable.kind,
        render_effect_set(&callable.direct),
        render_effect_set(&callable.synchronous),
        render_effect_set(&callable.transitive),
        render_names(&calls),
        render_names(&deferred),
    )
}

fn render_effect_set(effects: &rad_vm::types::AuthorityEffects) -> String {
    format!(
        "{{ reads: {}, writes: {}, emits: {}, io: {}, async: {}, unknown: {} }}",
        render_names(&effects.reads),
        render_names(&effects.writes),
        render_names(&effects.emits),
        effects.io,
        effects.async_effect,
        effects.unknown,
    )
}

fn render_reverse_query(kind: &str, authority: &str, names: &[String]) -> String {
    if names.is_empty() {
        return format!("{kind} {authority}: []");
    }
    format!("{kind} {authority}:\n{}", names.join("\n"))
}

fn render_names<T: AsRef<str>>(names: &[T]) -> String {
    format!(
        "[{}]",
        names
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ")
    )
}
