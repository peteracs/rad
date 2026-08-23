//! Minimal optimized RAD script runner for runtime profiling.
//!
//! This deliberately uses the same module loader, semantic product, compiler
//! provenance, and VM entry point as the CLI. It excludes CLI/LSP/Wasm
//! adapter dependencies so a VM hot-path edit can be measured without a
//! multi-minute ThinLTO relink of unrelated adapters.

use std::path::PathBuf;
use std::time::Instant;

use rad_vm::checker::CheckerOptions;
use rad_vm::module_loader::load_program_with_source_map;
use rad_vm::pipeline::{analyze_program, compile_checked_program, CheckedCompileOptions};
use rad_vm::vm::VM;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments
        .next()
        .ok_or("usage: run_rad_script_fast [--tests] <entry.rad> [program arguments ...]")?;
    let run_tests = first == "--tests";
    let entry = if run_tests {
        arguments
            .next()
            .map(PathBuf::from)
            .ok_or("usage: run_rad_script_fast --tests <entry.rad>")?
    } else {
        PathBuf::from(first)
    };
    let program_arguments = arguments
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    let total_started = Instant::now();
    let load_started = Instant::now();
    let loaded = load_program_with_source_map(&entry.to_string_lossy())
        .map_err(|errors| format!("module loading failed: {errors:?}"))?;
    if !loaded.errors.is_empty() {
        return Err(format!("module graph contains errors: {:?}", loaded.errors).into());
    }
    let load_elapsed = load_started.elapsed();

    let check_started = Instant::now();
    let analysis = analyze_program(&loaded.program, &loaded.aliases, CheckerOptions::default());
    if !analysis.errors().is_empty() {
        return Err(format!("semantic checking failed: {:?}", analysis.errors()).into());
    }
    let checked = analysis
        .into_checked()
        .map_err(|analysis| format!("semantic checking failed: {:?}", analysis.errors()))?;
    let check_elapsed = check_started.elapsed();

    let compile_started = Instant::now();
    let source_identity = loaded
        .source_layout
        .digest(&loaded.merged_source)
        .map_err(|error| format!("invalid module source layout: {error}"))?;
    let compiled = compile_checked_program(
        &loaded.program,
        loaded.aliases,
        checked,
        CheckedCompileOptions {
            release: true,
            source_identity: Some(source_identity),
        },
    )
    .map_err(|error| {
        format!(
            "compilation failed at {}:{}: {}",
            error.line, error.col, error.message
        )
    })?;
    let compile_elapsed = compile_started.elapsed();

    let mut vm = VM::new_with_seed(1);
    vm.sys_args = program_arguments;
    vm.load_compile_result(compiled);
    let execute_started = Instant::now();
    vm.run(0)
        .map_err(|error| format!("runtime failed: {error}"))?;
    if run_tests {
        let failures = rad_vm::test_runner::run_tests(&mut vm)
            .into_iter()
            .filter_map(|outcome| {
                outcome
                    .error
                    .map(|error| format!("{}: {error}", outcome.name))
            })
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return Err(format!("test failures:\n{}", failures.join("\n")).into());
        }
    }
    let execute_elapsed = execute_started.elapsed();
    let digest_started = Instant::now();
    let digest = vm.world_digest();
    let digest_elapsed = digest_started.elapsed();

    eprintln!(
        "RAD_TIMING entry={} load_ns={} check_ns={} compile_ns={} execute_ns={} digest_ns={} total_ns={} world_digest={}",
        entry.display(),
        load_elapsed.as_nanos(),
        check_elapsed.as_nanos(),
        compile_elapsed.as_nanos(),
        execute_elapsed.as_nanos(),
        digest_elapsed.as_nanos(),
        total_started.elapsed().as_nanos(),
        digest,
    );
    Ok(())
}
