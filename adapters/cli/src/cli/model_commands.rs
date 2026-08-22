fn run_model_check_command(
    filepath: String,
    model: Option<String>,
    runs: u32,
    max_commands: u32,
    seed: u64,
    artifact_directory: Option<String>,
    json: bool,
) {
    let checked = match load_checked_cli_program(&filepath) {
        Ok(checked) => checked,
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    };
    let mut vm = VM::new_with_seed(seed);
    if json {
        vm.suppress_output();
    }
    vm.configure_model_check(rad_vm::vm::ModelCheckConfig {
        model: model.clone(),
        runs: Some(runs),
        max_commands: Some(max_commands),
        seed: Some(seed),
        trace: None,
    });
    vm.load_compile_result(checked.compiled);
    if let Err(error) = vm.run(0) {
        eprintln!("Model fixture failed before checking: {error}");
        process::exit(1);
    }

    let started = std::time::Instant::now();
    let outcomes = rad_vm::test_runner::run_tests(&mut vm);
    let elapsed_ns = started.elapsed().as_nanos();
    let model_outcomes = outcomes
        .iter()
        .filter(|outcome| outcome.name.starts_with("model_"))
        .filter(|outcome| {
            model
                .as_ref()
                .is_none_or(|selected| outcome.name == format!("model_{selected}"))
        })
        .collect::<Vec<_>>();
    if model_outcomes.is_empty() {
        eprintln!(
            "No matching model declaration found{}",
            model
                .as_ref()
                .map_or_else(String::new, |name| format!(" for '{name}'"))
        );
        process::exit(1);
    }

    let failures = model_outcomes
        .iter()
        .filter_map(|outcome| {
            outcome.error.as_ref().map(|error| {
                let artifact = write_model_failure_artifact(
                    error,
                    Path::new(&filepath),
                    &checked.loaded.merged_source,
                    &checked.loaded.source_layout,
                    artifact_directory.as_deref().map(Path::new),
                );
                serde_json::json!({
                    "model": outcome.name.trim_start_matches("model_"),
                    "error": error,
                    "artifact": artifact.map(|path| path.to_string_lossy().into_owned()),
                })
            })
        })
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "file": filepath,
                "elapsed_ns": elapsed_ns,
                "reports": vm.model_check_reports(),
                "failures": failures,
            }))
            .expect("model report is serializable")
        );
    } else {
        for report in vm.model_check_reports() {
            println!(
                "model {}: histories {}/{}, commands {}, counterexamples {}, minimum trace {}, shrink {:.3} ms, history median {:.3} ms, p95 {:.3} ms",
                report.model,
                report.histories_executed,
                report.histories_requested,
                report.commands_executed,
                report.counterexamples,
                report
                    .minimum_trace_length
                    .map_or_else(|| "none".to_string(), |length| length.to_string()),
                report.shrink_ns as f64 / 1_000_000.0,
                report.median_history_ns as f64 / 1_000_000.0,
                report.p95_history_ns as f64 / 1_000_000.0,
            );
        }
        println!("elapsed: {:.3} s", elapsed_ns as f64 / 1_000_000_000.0);
        for failure in &failures {
            eprintln!(
                "model {} failed; replay artifact: {}",
                failure["model"].as_str().unwrap_or("unknown"),
                failure["artifact"].as_str().unwrap_or("unavailable")
            );
        }
    }
    if !failures.is_empty() {
        process::exit(1);
    }
}

fn run_model_shrink_command(artifact_path: &str) {
    let bytes = match fs::read(artifact_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("Error reading {artifact_path}: {error}");
            process::exit(1);
        }
    };
    let mut artifact: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Invalid model-failure artifact: {error}");
            process::exit(1);
        }
    };
    if artifact.get("kind").and_then(serde_json::Value::as_str)
        != Some("rad_model_failure_v1")
    {
        eprintln!("{artifact_path} is not a model-failure artifact");
        process::exit(1);
    }
    let source = artifact
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let model = artifact
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let seed = artifact
        .get("seed")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let trace = artifact
        .get("generated_trace")
        .cloned()
        .and_then(|value| {
            serde_json::from_value::<Vec<rad_vm::vm::ModelTraceStep>>(value).ok()
        })
        .unwrap_or_default();
    if source.is_empty() || model.is_empty() || trace.is_empty() {
        eprintln!("Model artifact omits source, model, or generated_trace");
        process::exit(1);
    }
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
        "model-shrink artifact",
        &features,
        &source_layout,
    ) {
        Ok(vm) => vm,
        Err(error) => {
            eprintln!("Error: {error}");
            process::exit(1);
        }
    };
    vm.configure_model_check(rad_vm::vm::ModelCheckConfig {
        model: Some(model.clone()),
        runs: Some(1),
        max_commands: Some(trace.len() as u32),
        seed: Some(seed),
        trace: Some(trace.clone()),
    });
    if let Err(error) = vm.run(0) {
        eprintln!("Model shrink fixture failed: {error}");
        process::exit(1);
    }
    let outcomes = rad_vm::test_runner::run_tests(&mut vm);
    let outcome = outcomes
        .iter()
        .find(|outcome| outcome.name == format!("model_{model}"));
    let Some(error) = outcome.and_then(|outcome| outcome.error.as_deref()) else {
        eprintln!("Model shrink DIVERGED: '{model}' no longer fails");
        process::exit(1);
    };
    let Some(reduced) = model_failure_json(error) else {
        eprintln!("Model shrink did not produce a replayable failure marker");
        process::exit(1);
    };
    let old_length = trace.len();
    let new_length = reduced["minimal_trace"].as_array().map_or(0, Vec::len);
    let object = artifact
        .as_object_mut()
        .expect("validated model artifact is an object");
    for key in [
        "campaign_seed",
        "seed",
        "run_index",
        "histories_requested",
        "max_commands",
        "generated_trace",
        "minimal_trace",
        "reason",
    ] {
        if let Some(value) = reduced.get(key) {
            object.insert(key.to_string(), value.clone());
        }
    }
    if let Err(error) = fs::write(
        artifact_path,
        serde_json::to_vec_pretty(&artifact).expect("model artifact serializes"),
    ) {
        eprintln!("Error writing {artifact_path}: {error}");
        process::exit(1);
    }
    println!(
        "model {model}: deterministically shrunk {old_length} command(s) to {new_length}; updated {artifact_path}"
    );
}
