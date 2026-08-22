//! Canonical source-to-bytecode orchestration.
//!
//! Individual lexer, parser, checker, and compiler APIs remain available for
//! layer tests. Production adapters and end-to-end tests should use this
//! module so aliases, semantic options, checker products, and source identity
//! cannot drift between entry points.

use std::collections::HashMap;

use crate::ast::{ModuleAlias, Program};
use crate::checker::{Checker, CheckerOptions, CheckerSemanticIndex, TypeError, TypeWarning};
use crate::compiler::{CompileError, CompileResult, Compiler};
use crate::lexer::{Lexer, LexerError};
use crate::parser::{ParseError, Parser, ParserOptions};
use crate::types::CheckerOutput;

#[derive(Debug)]
pub struct ParsedSource {
    pub program: Program,
    pub lexer_errors: Vec<LexerError>,
    pub parser_errors: Vec<ParseError>,
}

pub fn parse_source(source: &str, options: ParserOptions) -> ParsedSource {
    let mut lexer = Lexer::new(source);
    let (tokens, lexer_errors) = lexer.tokenize();
    let mut parser = Parser::new(tokens).with_options(options);
    let program = parser.parse();
    ParsedSource {
        program,
        lexer_errors,
        parser_errors: parser.errors().to_vec(),
    }
}

#[derive(Debug)]
pub struct SemanticAnalysis {
    errors: Vec<TypeError>,
    warnings: Vec<TypeWarning>,
    output: CheckerOutput,
}

impl SemanticAnalysis {
    pub fn errors(&self) -> &[TypeError] {
        &self.errors
    }

    pub fn warnings(&self) -> &[TypeWarning] {
        &self.warnings
    }

    pub fn output(&self) -> &CheckerOutput {
        &self.output
    }

    pub fn into_checked(self) -> Result<CheckedSemantics, Box<Self>> {
        if self.errors.is_empty() {
            Ok(CheckedSemantics {
                warnings: self.warnings,
                output: self.output,
            })
        } else {
            Err(Box::new(self))
        }
    }
}

/// A checker product that is known to have no semantic errors.
///
/// Fields are private so adapters cannot pair an arbitrary output with the
/// checked-program path. `Compiler` still verifies product provenance and
/// integrity when the product is installed.
#[derive(Debug)]
pub struct CheckedSemantics {
    warnings: Vec<TypeWarning>,
    output: CheckerOutput,
}

impl CheckedSemantics {
    pub fn warnings(&self) -> &[TypeWarning] {
        &self.warnings
    }

    pub fn output(&self) -> &CheckerOutput {
        &self.output
    }
}

/// The one place a `Checker` is constructed and run.
///
/// Deliberately stops at diagnostics and hands the checker back: building the
/// semantic product costs a fingerprint over every checker map, so each entry
/// point below decides for itself whether it needs one. A `build_output: bool`
/// parameter would hide that decision at the call sites.
fn run_checker(
    program: &Program,
    aliases: &HashMap<String, ModuleAlias>,
    options: CheckerOptions,
) -> (Checker, Diagnostics) {
    let mut checker = Checker::new_with_options(options);
    checker.set_aliases(aliases.clone());
    let errors = checker.check(program);
    let warnings = checker.warnings();
    (checker, Diagnostics { errors, warnings })
}

/// Diagnostics plus the checked semantic product the compiler installs.
pub fn analyze_program(
    program: &Program,
    aliases: &HashMap<String, ModuleAlias>,
    options: CheckerOptions,
) -> SemanticAnalysis {
    let (checker, diagnostics) = run_checker(program, aliases, options);
    SemanticAnalysis {
        errors: diagnostics.errors,
        warnings: diagnostics.warnings,
        output: checker.output(),
    }
}

/// Diagnostics for one program, without building the compiler's semantic
/// product.
///
/// `analyze_program` finishes by calling `Checker::output()`, which computes
/// the product fingerprint by debug-formatting every checker map. The compile
/// path needs that; an editor rendering squiggles on each keystroke does not,
/// and paying it per keypress is work proportional to the whole program for a
/// value that is immediately dropped.
pub fn check_program(
    program: &Program,
    aliases: &HashMap<String, ModuleAlias>,
    options: CheckerOptions,
) -> Diagnostics {
    let (_, diagnostics) = run_checker(program, aliases, options);
    diagnostics
}

/// Diagnostics plus the read-only projection editors query for hover and
/// completion, from one checker run and without the product fingerprint.
pub fn index_program(
    program: &Program,
    aliases: &HashMap<String, ModuleAlias>,
    options: CheckerOptions,
) -> (Diagnostics, CheckerSemanticIndex) {
    let (checker, diagnostics) = run_checker(program, aliases, options);
    let index = checker.semantic_index();
    (diagnostics, index)
}

#[derive(Debug, Default)]
pub struct Diagnostics {
    errors: Vec<TypeError>,
    warnings: Vec<TypeWarning>,
}

impl Diagnostics {
    pub fn errors(&self) -> &[TypeError] {
        &self.errors
    }

    pub fn warnings(&self) -> &[TypeWarning] {
        &self.warnings
    }
}

#[derive(Debug)]
pub struct SourceAnalysis {
    pub program: Program,
    pub lexer_errors: Vec<LexerError>,
    pub parser_errors: Vec<ParseError>,
    semantic: SemanticAnalysis,
}

impl SourceAnalysis {
    pub fn semantic(&self) -> &SemanticAnalysis {
        &self.semantic
    }

    pub fn into_parts(self) -> (Program, SemanticAnalysis) {
        (self.program, self.semantic)
    }
}

pub fn analyze_source(
    source: &str,
    parser_options: ParserOptions,
    aliases: &HashMap<String, ModuleAlias>,
    checker_options: CheckerOptions,
) -> SourceAnalysis {
    let parsed = parse_source(source, parser_options);
    let semantic = analyze_program(&parsed.program, aliases, checker_options);
    SourceAnalysis {
        program: parsed.program,
        lexer_errors: parsed.lexer_errors,
        parser_errors: parsed.parser_errors,
        semantic,
    }
}

#[derive(Debug, Clone, Default)]
pub struct CheckedCompileOptions {
    pub release: bool,
    pub source_identity: Option<String>,
}

pub fn compile_checked_program(
    program: &Program,
    aliases: HashMap<String, ModuleAlias>,
    checked: CheckedSemantics,
    options: CheckedCompileOptions,
) -> Result<CompileResult, CompileError> {
    let checker_options = checked
        .output
        .semantic_options
        .clone()
        .ok_or_else(|| CompileError {
            message: "checked semantic product has no semantic configuration".to_string(),
            line: 0,
            col: 0,
        })?;
    let mut compiler = Compiler::new()
        .with_release(options.release)
        .with_aliases(aliases)
        .with_checker_options(checker_options)
        .with_checker_output(checked.output);
    if let Some(identity) = options.source_identity {
        compiler = compiler.with_program_source_identity(identity);
    }
    compiler.compile(program)
}

/// Compile through the explicitly unchecked production path.
///
/// This exists for deterministic replay and the CLI's opt-in `--no-check`
/// mode. It is not a fallback from failed semantic analysis: callers must
/// choose it directly, and the compiler's own defensive validation remains
/// active. Keeping the complete configuration here prevents those exceptional
/// entry points from drifting on aliases, features, release mode, or source
/// identity.
#[derive(Debug, Clone, Default)]
pub struct UncheckedCompileOptions {
    pub features: Vec<String>,
    pub release: bool,
    pub source_identity: Option<String>,
}

pub fn compile_unchecked_program(
    program: &Program,
    aliases: HashMap<String, ModuleAlias>,
    options: UncheckedCompileOptions,
) -> Result<CompileResult, CompileError> {
    let mut compiler = Compiler::new()
        .with_release(options.release)
        .with_aliases(aliases)
        .with_features(options.features);
    if let Some(identity) = options.source_identity {
        compiler = compiler.with_program_source_identity(identity);
    }
    compiler.compile(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_pipeline_preserves_semantic_configuration_through_compilation() {
        let parsed = parse_source(
            "component Root {} system Run(root: Root) {}",
            ParserOptions::default(),
        );
        assert!(parsed.lexer_errors.is_empty());
        assert!(parsed.parser_errors.is_empty());
        let options = CheckerOptions {
            strict_types: true,
            features: vec!["causal_laws".to_string()],
            ..CheckerOptions::default()
        };
        let analysis = analyze_program(&parsed.program, &HashMap::new(), options);
        assert!(analysis.errors().is_empty(), "{:?}", analysis.errors());
        let checked = analysis.into_checked().expect("analysis is error-free");
        compile_checked_program(
            &parsed.program,
            HashMap::new(),
            checked,
            CheckedCompileOptions::default(),
        )
        .expect("checked product and compiler configuration stay provenance-compatible");
    }

    #[test]
    fn parse_source_reports_lexical_and_parser_diagnostics_together() {
        let parsed = parse_source("let x = §\ncomponent", ParserOptions::default());
        assert!(!parsed.lexer_errors.is_empty());
        assert!(!parsed.parser_errors.is_empty());
    }

    #[test]
    fn unchecked_pipeline_is_explicit_and_preserves_feature_configuration() {
        let parsed = parse_source(
            "intent Ping { key target: entity }",
            ParserOptions::default(),
        );
        assert!(parsed.lexer_errors.is_empty());
        assert!(parsed.parser_errors.is_empty());
        compile_unchecked_program(
            &parsed.program,
            HashMap::new(),
            UncheckedCompileOptions {
                features: vec!["causal_laws".to_string()],
                ..UncheckedCompileOptions::default()
            },
        )
        .expect("the explicit unchecked path must preserve feature configuration");
    }
}
