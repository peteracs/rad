fn run_ffi_verify_command(plugin: &str, contract: Option<&str>, json: bool) {
    let report = match verify_in_isolated_worker(plugin) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("FFI verification failed: {error}");
            process::exit(1);
        }
    };
    if let Some(contract) = contract {
        let encoded = match fs::read_to_string(contract) {
            Ok(encoded) => encoded,
            Err(error) => {
                eprintln!("FFI verification failed: cannot read contract '{contract}': {error}");
                process::exit(1);
            }
        };
        if let Err(error) = rad_vm::ffi::verify_plugin_contract(&report, &encoded) {
            eprintln!("FFI verification failed: {error}");
            process::exit(1);
        }
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("FFI verification report serializes")
        );
        return;
    }

    println!("Plugin: {}", report.plugin);
    println!("Extension: {} {}", report.extension_id, report.extension_version);
    println!("RAD extension ABI: compatible ({})", report.abi_version);
    println!("Verification isolation: out-of-process");
    println!("Runtime execution: persistent isolated generation worker");
    println!("Calling convention: {}", report.calling_convention);
    println!("Content digest: {}", report.content_digest);
    println!();
    for export in &report.exports {
        println!("Export: {}{}", export.name, export.signature);
        println!("  effects: {}", export.effect_class);
        println!("  deterministic: {}", export.deterministic);
        println!("  replayable: {}", export.replayable);
    }
    for layout in &report.layouts {
        println!();
        println!(
            "Layout: {} size={} alignment={}",
            layout.name, layout.size, layout.alignment
        );
        for field in &layout.fields {
            println!(
                "  offset {:>3} size {:>3}  {}: {}",
                field.offset, field.size, field.name, field.type_name
            );
        }
    }
    println!();
    println!(
        "Layout agreement: {}",
        if report.layout_agreement { "PASS" } else { "FAIL" }
    );
    println!(
        "Opaque-type agreement: {}",
        if report.opaque_identity { "PASS" } else { "N/A" }
    );
    println!(
        "Determinism probe: {}",
        if report.determinism_probe { "PASS" } else { "NOT DECLARED" }
    );
    println!(
        "Replay support: {}",
        if report.replay_support { "PASS" } else { "FAIL" }
    );
}

fn verify_in_isolated_worker(
    plugin: &str,
) -> Result<rad_vm::ffi::NativeVerificationReport, String> {
    let executable = rad_vm::ffi::native_worker_executable()?;
    let mut child = std::process::Command::new(&executable)
        .args(["--verify", plugin])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "cannot start isolated verifier '{}': {error}",
                executable.display()
            )
        })?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(700);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("plugin exceeded the isolated 700 ms verification timeout".into());
            }
            Err(error) => {
                let _ = child.kill();
                return Err(format!("isolated verifier process error: {error}"));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot read isolated verifier output: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} (isolated verifier {})",
            detail.trim(),
            output.status
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("isolated verifier returned malformed JSON: {error}"))
}
