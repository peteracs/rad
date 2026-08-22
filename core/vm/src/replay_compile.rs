//! Compilation boundary for self-contained replay traces.
//!
//! Current traces carry an authenticated source bundle and must rebuild the
//! original module graph. Vintage single-source traces retain their flat
//! compilation path. Keeping this policy here prevents the CLI and replay
//! server from drifting apart.

use crate::parser::ParserOptions;
use crate::source_bundle::SourceLayout;
use crate::vm::VM;

pub fn compile_trace_vm(
    source: &str,
    description: &str,
    features: &[String],
    source_layout: &SourceLayout,
) -> Result<VM, String> {
    let source_identity = source_layout
        .digest(source)
        .map_err(|error| format!("{description} has an invalid source layout: {error}"))?;
    let (program, aliases) = if source_layout.sections.is_empty() {
        let parsed = crate::pipeline::parse_source(source, ParserOptions::default());
        if let Some(error) = parsed.lexer_errors.first() {
            return Err(format!("{description} failed to lex: {}", error.message));
        }
        if let Some(error) = parsed.parser_errors.first() {
            return Err(format!("{description} failed to parse: {}", error.message));
        }
        (parsed.program, std::collections::HashMap::new())
    } else {
        let loaded = crate::module_loader::load_program_from_source_bundle(
            source,
            source_layout,
            ParserOptions {
                compat_v0_5_dx: false,
            },
        )
        .map_err(render_load_errors)?;
        if !loaded.errors.is_empty() {
            return Err(render_load_errors(loaded.errors));
        }
        (loaded.program, loaded.aliases)
    };
    let compile_result = crate::pipeline::compile_unchecked_program(
        &program,
        aliases,
        crate::pipeline::UncheckedCompileOptions {
            features: features.to_vec(),
            source_identity: Some(source_identity),
            ..crate::pipeline::UncheckedCompileOptions::default()
        },
    )
    .map_err(|error| format!("{description} failed to compile: {}", error.message))?;

    let mut vm = VM::new();
    vm.load_compile_result(compile_result);
    Ok(vm)
}

fn render_load_errors(errors: Vec<crate::module_loader::ModuleLoadError>) -> String {
    errors
        .into_iter()
        .map(|error| error.message)
        .collect::<Vec<_>>()
        .join("\n")
}
