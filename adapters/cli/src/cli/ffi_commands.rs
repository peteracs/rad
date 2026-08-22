fn run_ffi_verify_command(plugin: &str, contract: Option<&str>, json: bool) {
    if std::env::var_os("RAD_FFI_VERIFY_CHILD").is_none() {
        let executable = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("FFI verification failed: cannot locate verifier executable: {error}");
                process::exit(1);
            }
        };
        let mut command = std::process::Command::new(executable);
        command.args(["ffi", "verify", plugin]);
        if let Some(contract) = contract {
            command.args(["--contract", contract]);
        }
        if json {
            command.arg("--json");
        }
        let mut child = match command
            .env("RAD_FFI_VERIFY_CHILD", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("FFI verification failed: cannot start isolated verifier: {error}");
                process::exit(1);
            }
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!(
                        "FFI verification failed: plugin exceeded the isolated 10-second verification timeout"
                    );
                    process::exit(1);
                }
                Err(error) => {
                    let _ = child.kill();
                    eprintln!("FFI verification failed: verifier process error: {error}");
                    process::exit(1);
                }
            }
        }
        let output = child
            .wait_with_output()
            .expect("completed isolated verifier output remains readable");
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            eprintln!(
                "FFI verification failed safely in isolated process ({})",
                output.status
            );
            process::exit(1);
        }
        return;
    }

    let report = match rad_vm::ffi::verify_plugin(plugin) {
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
