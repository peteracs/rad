//! Compilation boundary for self-contained replay traces.
//!
//! Current traces carry an authenticated source bundle and must rebuild the
//! original module graph. Vintage single-source traces retain their flat
//! compilation path. Keeping this policy here prevents the CLI and replay
//! server from drifting apart.

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::{Parser, ParserOptions};
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
    let compile_result = if source_layout.sections.is_empty() {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        if let Some(error) = parser.errors().first() {
            return Err(format!("{description} failed to parse: {}", error.message));
        }
        Compiler::new()
            .with_features(features.to_vec())
            .with_program_source_identity(source_identity.clone())
            .compile(&program)
            .map_err(|error| format!("{description} failed to compile: {}", error.message))?
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
        Compiler::new()
            .with_aliases(loaded.aliases)
            .with_features(features.to_vec())
            .with_program_source_identity(source_identity)
            .compile(&loaded.program)
            .map_err(|error| format!("{description} failed to compile: {}", error.message))?
    };

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
