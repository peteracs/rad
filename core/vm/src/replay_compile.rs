//! Compilation boundary for self-contained replay traces.
//!
//! Current traces carry an authenticated source bundle and rebuild the
//! original module graph plus its checked semantic product. Replay cannot use
//! the unchecked compiler path: lowering metadata such as spread arity,
//! iterator kinds, and nominal redirects belongs to semantic analysis.

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
        let parsed = crate::pipeline::parse_source(source, ParserOptions);
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
            ParserOptions,
        )
        .map_err(render_load_errors)?;
        if !loaded.errors.is_empty() {
            return Err(render_load_errors(loaded.errors));
        }
        (loaded.program, loaded.aliases)
    };
    let analysis = crate::pipeline::analyze_program(
        &program,
        &aliases,
        crate::checker::CheckerOptions {
            features: features.to_vec(),
            ..crate::checker::CheckerOptions::default()
        },
    );
    if !analysis.errors().is_empty() {
        return Err(format!(
            "{description} failed semantic analysis: {}",
            analysis
                .errors()
                .iter()
                .map(|error| format!("line {}: {}", error.line, error.message))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let checked = analysis
        .into_checked()
        .expect("an error-free replay analysis produces checked semantics");
    let compile_result = crate::pipeline::compile_checked_program(
        &program,
        aliases,
        checked,
        crate::pipeline::CheckedCompileOptions {
            source_identity: Some(source_identity),
            ..crate::pipeline::CheckedCompileOptions::default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_compilation_preserves_checker_derived_spread_arity() {
        let source = r#"
fn sum_three(a: int, b: int, c: int) -> int { return a + b + c }
assert(sum_three(..(1, 2, 3)) == 6, "spread arity survives trace compilation")
"#;
        let mut vm = compile_trace_vm(source, "spread regression", &[], &SourceLayout::default())
            .expect("checked trace compilation succeeds");
        vm.run(0)
            .expect("replay compiler passes all tuple elements to the call");
    }
}
