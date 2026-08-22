use std::collections::HashMap;
use std::path::PathBuf;

use rad_vm::checker::CheckerOptions;
use rad_vm::parser::ParserOptions;
use rad_vm::pipeline::{
    analyze_program, compile_checked_program, parse_source, CheckedCompileOptions,
};
use rad_vm::vm::VM;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .ok_or("rad-vm manifest is not under the workspace root")?
        .to_path_buf();
    let source_path = workspace.join("projects/dogfood/sovereign-grid/bench.rad");
    let output_path = workspace.join("target/profiles/sovereign-grid-indexed-dhat.json");
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let source = std::fs::read_to_string(&source_path)?;
    let parsed = parse_source(&source, ParserOptions);
    if !parsed.lexer_errors.is_empty() || !parsed.parser_errors.is_empty() {
        return Err(format!(
            "benchmark source did not parse: lexer={:?}, parser={:?}",
            parsed.lexer_errors, parsed.parser_errors
        )
        .into());
    }
    let aliases = HashMap::new();
    let analysis = analyze_program(
        &parsed.program,
        &aliases,
        CheckerOptions {
            strict_types: true,
            ..CheckerOptions::default()
        },
    );
    if !analysis.errors().is_empty() || !analysis.warnings().is_empty() {
        return Err(format!(
            "benchmark source did not pass strict checking: errors={:?}, warnings={:?}",
            analysis.errors(),
            analysis.warnings()
        )
        .into());
    }
    let checked = analysis
        .into_checked()
        .map_err(|analysis| format!("benchmark semantic analysis failed: {analysis:?}"))?;
    let compiled = compile_checked_program(
        &parsed.program,
        aliases,
        checked,
        CheckedCompileOptions {
            release: true,
            source_identity: Some("sovereign-grid/bench.rad".to_string()),
        },
    )
    .map_err(|error| {
        format!(
            "benchmark compilation failed at {}:{}: {}",
            error.line, error.col, error.message
        )
    })?;

    let mut vm = VM::new_with_seed(1);
    vm.sys_args = vec!["indexed".to_string(), "1000".to_string()];
    vm.load_compile_result(compiled);
    vm.run(0)
        .map_err(|error| format!("benchmark setup failed: {error}"))?;

    let profiler = dhat::Profiler::builder().file_name(&output_path).build();
    vm.call_global("bench_run", &[])
        .map_err(|error| format!("measured benchmark entry failed: {error}"))?;
    let stats = dhat::HeapStats::get();
    drop(profiler);

    println!("profile={}", output_path.display());
    println!("total_allocations={}", stats.total_blocks);
    println!("total_bytes={}", stats.total_bytes);
    println!("peak_live_allocations={}", stats.max_blocks);
    println!("peak_live_bytes={}", stats.max_bytes);
    println!("remaining_allocations={}", stats.curr_blocks);
    println!("remaining_bytes={}", stats.curr_bytes);
    Ok(())
}
