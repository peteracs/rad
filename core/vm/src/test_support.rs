use std::collections::HashMap;

use crate::ast::Program;
use crate::checker::{Checker, CheckerOptions, TypeError};
use crate::compiler::CompileResult;
use crate::parser::ParserOptions;
use crate::vm::VM;

pub(crate) fn parse_program(
    source: &str,
    parser_options: ParserOptions,
) -> Result<Program, String> {
    let parsed = crate::pipeline::parse_source(source, parser_options);
    let mut diagnostics = parsed
        .lexer_errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    diagnostics.extend(parsed.parser_errors.iter().map(ToString::to_string));
    if diagnostics.is_empty() {
        Ok(parsed.program)
    } else {
        Err(diagnostics.join("\n"))
    }
}

pub(crate) fn compile_source(
    source: &str,
    parser_options: ParserOptions,
) -> Result<CompileResult, String> {
    let program = parse_program(source, parser_options)?;
    crate::pipeline::compile_unchecked_program(
        &program,
        HashMap::new(),
        crate::pipeline::UncheckedCompileOptions::default(),
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn compile_checked_source(
    source: &str,
    parser_options: ParserOptions,
    checker_options: CheckerOptions,
) -> Result<CompileResult, String> {
    let program = parse_program(source, parser_options)?;
    let analysis = crate::pipeline::analyze_program(&program, &HashMap::new(), checker_options);
    if !analysis.errors().is_empty() {
        return Err(analysis
            .errors()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let checked = analysis
        .into_checked()
        .map_err(|_| "error-free analysis did not produce checked semantics".to_string())?;
    crate::pipeline::compile_checked_program(
        &program,
        HashMap::new(),
        checked,
        crate::pipeline::CheckedCompileOptions::default(),
    )
    .map_err(|error| error.to_string())
}

fn run_compile_result(result: CompileResult, serial_schedule: bool) -> Result<Vec<String>, String> {
    let mut vm = VM::new();
    vm.set_serial_schedule(serial_schedule);
    vm.load_compile_result(result);
    vm.run(0)?;
    Ok(vm.print_buffer.clone())
}

pub(crate) fn run_source(
    source: &str,
    parser_options: ParserOptions,
) -> Result<Vec<String>, String> {
    run_compile_result(compile_source(source, parser_options)?, false)
}

pub(crate) fn run_checked_source(
    source: &str,
    parser_options: ParserOptions,
    checker_options: CheckerOptions,
) -> Result<Vec<String>, String> {
    run_compile_result(
        compile_checked_source(source, parser_options, checker_options)?,
        false,
    )
}

pub(crate) fn run_source_serial(
    source: &str,
    parser_options: ParserOptions,
) -> Result<Vec<String>, String> {
    run_compile_result(compile_source(source, parser_options)?, true)
}

/// A checked program plus the checker that produced it.
///
/// Checker tests that start from source text each hand-rolled the same
/// lexer -> parser -> checker sequence, so how those tests parsed could drift
/// from `pipeline`. Tests that construct an AST directly still use `Checker`
/// on its own: the checker is the unit under test there, and routing them
/// through parsing would only obscure what they assert.
pub(crate) struct SourceCheck {
    pub(crate) checker: Checker,
    pub(crate) errors: Vec<TypeError>,
}

pub(crate) fn check_source(source: &str) -> SourceCheck {
    check_source_with(source, ParserOptions, CheckerOptions::default())
}

pub(crate) fn check_source_with(
    source: &str,
    parser_options: ParserOptions,
    checker_options: CheckerOptions,
) -> SourceCheck {
    let program = parse_program(source, parser_options).expect("checker test source must parse");
    let mut checker = Checker::new_with_options(checker_options);
    let errors = checker.check(&program);
    SourceCheck { checker, errors }
}
