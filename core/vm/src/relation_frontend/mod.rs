//! Bounded RFC-0003 language front end.
//!
//! This module stops at immutable schemas and sealed rule plans. It does not
//! mutate the VM, install a relation store, or evaluate derivations.

mod ast;
mod canonical;
mod checker;
mod lexer;
mod limits;
mod parser;
mod tooling;

pub use ast::{
    AggregateKind, BoundedRawProgram, BoundedRawRule, DerivationDependencyDag, FrontendArtifacts,
    FrontendManifestDigest, Literal, OnDelete, RawInputStats, RawOperationValue, RawRuleSummary,
    RelationColumn, RelationKind, RelationManifest, RelationOperation, RelationOperationKind,
    RelationSchema, RelationType, RuleAggregate, RuleAtom, RulePredicate, RuleTerm, SealedRulePlan,
    SourceSpan, StaticResourceQuote, TypedRulePlan, UniqueConstraint,
};
pub use limits::{RawInputLimits, SealedPlanLimits};

use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticCode {
    FeatureDisabled,
    RawModuleLimit,
    RawTotalModuleIdentifierByteLimit,
    RawTotalSourceByteLimit,
    RawSourceByteLimit,
    RawTokenLimit,
    RawRelationLimit,
    RawRuleLimit,
    RawOperationLimit,
    RawColumnLimit,
    RawUniqueConstraintLimit,
    RawTupleLimit,
    EmptyIdentifier,
    UnqualifiedModule,
    RawIdentifierByteLimit,
    RawTermLimit,
    RawAtomLimit,
    RawPredicateLimit,
    RawAggregateGroupLimit,
    RawAstNodeLimit,
    RawStructuralCostLimit,
    Syntax,
    DuplicateModule,
    ForeignRelationDeclaration,
    ForeignDerivedDeclaration,
    DuplicateRelation,
    DuplicateColumn,
    DuplicateUniqueConstraint,
    UnknownUniqueColumn,
    SymmetricShape,
    SymmetricUnique,
    SymmetricEndpointMetadata,
    NamespaceCollision,
    OperationTargetsDerived,
    UnknownRelation,
    Arity,
    TypeMismatch,
    UnboundVariable,
    DuplicateHeadColumn,
    DuplicateGroupVariable,
    AggregateRequiresPositiveInput,
    AggregateOutputNotFresh,
    AggregateHeadProjection,
    AggregateType,
    RecursiveDerivation,
    DuplicateRule,
    SealedRuleLimit,
    SealedAtomLimit,
    SealedPredicateLimit,
    SealedTermLimit,
    SealedDependencyLimit,
    SealedByteLimit,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FeatureDisabled => "relations.feature_disabled",
            Self::RawModuleLimit => "relations.raw_module_limit",
            Self::RawTotalModuleIdentifierByteLimit => {
                "relations.raw_total_module_identifier_byte_limit"
            }
            Self::RawTotalSourceByteLimit => "relations.raw_total_source_byte_limit",
            Self::RawSourceByteLimit => "relations.raw_source_byte_limit",
            Self::RawTokenLimit => "relations.raw_token_limit",
            Self::RawRelationLimit => "relations.raw_relation_limit",
            Self::RawRuleLimit => "relations.raw_rule_limit",
            Self::RawOperationLimit => "relations.raw_operation_limit",
            Self::RawColumnLimit => "relations.raw_column_limit",
            Self::RawUniqueConstraintLimit => "relations.raw_unique_constraint_limit",
            Self::RawTupleLimit => "relations.raw_tuple_limit",
            Self::EmptyIdentifier => "relations.empty_identifier",
            Self::UnqualifiedModule => "relations.unqualified_module",
            Self::RawIdentifierByteLimit => "relations.raw_identifier_byte_limit",
            Self::RawTermLimit => "relations.raw_term_limit",
            Self::RawAtomLimit => "relations.raw_atom_limit",
            Self::RawPredicateLimit => "relations.raw_predicate_limit",
            Self::RawAggregateGroupLimit => "relations.raw_aggregate_group_limit",
            Self::RawAstNodeLimit => "relations.raw_ast_node_limit",
            Self::RawStructuralCostLimit => "relations.raw_structural_cost_limit",
            Self::Syntax => "relations.syntax",
            Self::DuplicateModule => "relations.duplicate_module",
            Self::ForeignRelationDeclaration => "relations.module_foreign_relation_declaration",
            Self::ForeignDerivedDeclaration => "relations.module_foreign_derived_declaration",
            Self::DuplicateRelation => "relations.duplicate_relation",
            Self::DuplicateColumn => "relations.duplicate_column",
            Self::DuplicateUniqueConstraint => "relations.duplicate_unique_constraint",
            Self::UnknownUniqueColumn => "relations.unknown_unique_column",
            Self::SymmetricShape => "relations.symmetric_shape",
            Self::SymmetricUnique => "relations.symmetric_unique_forbidden",
            Self::SymmetricEndpointMetadata => "relations.symmetric_endpoint_metadata",
            Self::NamespaceCollision => "relations.namespace_collision",
            Self::OperationTargetsDerived => "relations.operation_targets_derived",
            Self::UnknownRelation => "relations.unknown_relation",
            Self::Arity => "relations.arity",
            Self::TypeMismatch => "relations.type_mismatch",
            Self::UnboundVariable => "relations.unbound_variable",
            Self::DuplicateHeadColumn => "relations.derivation_duplicate_head_column",
            Self::DuplicateGroupVariable => "relations.derivation_duplicate_group_variable",
            Self::AggregateRequiresPositiveInput => "relations.aggregate_requires_positive_input",
            Self::AggregateOutputNotFresh => "relations.aggregate_output_not_fresh",
            Self::AggregateHeadProjection => "relations.aggregate_head_projection",
            Self::AggregateType => "relations.aggregate_type",
            Self::RecursiveDerivation => "relations.recursive_derivation",
            Self::DuplicateRule => "relations.duplicate_rule",
            Self::SealedRuleLimit => "relations.sealed_rule_limit",
            Self::SealedAtomLimit => "relations.sealed_atom_limit",
            Self::SealedPredicateLimit => "relations.sealed_predicate_limit",
            Self::SealedTermLimit => "relations.sealed_term_limit",
            Self::SealedDependencyLimit => "relations.sealed_dependency_limit",
            Self::SealedByteLimit => "relations.sealed_byte_limit",
        }
    }

    pub const fn priority(self) -> u8 {
        match self {
            Self::FeatureDisabled => 0,
            Self::RawModuleLimit => 1,
            Self::RawTotalModuleIdentifierByteLimit => 2,
            Self::RawTotalSourceByteLimit => 3,
            Self::RawSourceByteLimit => 4,
            Self::RawTokenLimit => 5,
            Self::RawRelationLimit => 6,
            Self::RawRuleLimit => 7,
            Self::RawOperationLimit => 8,
            Self::RawColumnLimit => 9,
            Self::RawUniqueConstraintLimit => 10,
            Self::RawTupleLimit => 11,
            Self::EmptyIdentifier => 12,
            Self::UnqualifiedModule => 13,
            Self::RawIdentifierByteLimit => 14,
            Self::RawTermLimit => 15,
            Self::RawAtomLimit => 16,
            Self::RawPredicateLimit => 17,
            Self::RawAggregateGroupLimit => 18,
            Self::RawAstNodeLimit => 19,
            Self::RawStructuralCostLimit => 20,
            Self::Syntax => 21,
            Self::DuplicateModule => 22,
            Self::ForeignRelationDeclaration => 23,
            Self::ForeignDerivedDeclaration => 24,
            Self::DuplicateRelation => 25,
            Self::DuplicateColumn => 26,
            Self::DuplicateUniqueConstraint => 27,
            Self::UnknownUniqueColumn => 28,
            Self::SymmetricShape => 29,
            Self::SymmetricUnique => 30,
            Self::SymmetricEndpointMetadata => 31,
            Self::NamespaceCollision => 32,
            Self::OperationTargetsDerived => 33,
            Self::UnknownRelation => 34,
            Self::Arity => 35,
            Self::TypeMismatch => 36,
            Self::UnboundVariable => 37,
            Self::DuplicateGroupVariable => 38,
            Self::DuplicateHeadColumn => 39,
            Self::AggregateRequiresPositiveInput => 40,
            Self::AggregateOutputNotFresh => 41,
            Self::AggregateHeadProjection => 42,
            Self::AggregateType => 43,
            Self::RecursiveDerivation => 44,
            Self::DuplicateRule => 45,
            Self::SealedRuleLimit => 46,
            Self::SealedAtomLimit => 47,
            Self::SealedPredicateLimit => 48,
            Self::SealedTermLimit => 49,
            Self::SealedDependencyLimit => 50,
            Self::SealedByteLimit => 51,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RawDiagnosticWitness([u8; 32]);

impl RawDiagnosticWitness {
    pub fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendDiagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub witness: RawDiagnosticWitness,
    pub owner: Option<String>,
}

impl FrontendDiagnostic {
    pub(crate) fn new(
        code: DiagnosticCode,
        message: impl Into<String>,
        line: u32,
        column: u32,
        detail: &[u8],
    ) -> Self {
        let message = message.into();
        let mut hasher = Sha256::new();
        hasher.update(b"rad.relation-frontend-diagnostic.v1");
        hasher.update([code.priority()]);
        hasher.update((detail.len() as u64).to_be_bytes());
        hasher.update(detail);
        Self {
            code,
            message,
            line,
            column,
            witness: RawDiagnosticWitness(hasher.finalize().into()),
            owner: None,
        }
    }

    pub(crate) fn owned(mut self, owner: &str) -> Self {
        if self.owner.as_deref() == Some(owner) {
            return self;
        }
        let mut hasher = Sha256::new();
        hasher.update(b"rad.relation-frontend-owned-diagnostic.v1");
        hasher.update(self.witness.0);
        hasher.update((owner.len() as u64).to_be_bytes());
        hasher.update(owner.as_bytes());
        self.witness = RawDiagnosticWitness(hasher.finalize().into());
        self.owner = Some(owner.to_string());
        self
    }

    pub(crate) fn at(mut self, span: SourceSpan) -> Self {
        self.line = span.line;
        self.column = span.column;
        self
    }

    pub(crate) fn limit(code: DiagnosticCode, actual: usize, limit: usize) -> Self {
        let mut detail = Vec::with_capacity(16);
        detail.extend_from_slice(&(actual as u64).to_be_bytes());
        detail.extend_from_slice(&(limit as u64).to_be_bytes());
        Self::new(
            code,
            format!("{}: {actual} exceeds {limit}", code.as_str()),
            0,
            0,
            &detail,
        )
    }
}

impl Ord for FrontendDiagnostic {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.code.priority(), self.witness, self.line, self.column).cmp(&(
            other.code.priority(),
            other.witness,
            other.line,
            other.column,
        ))
    }
}

impl PartialOrd for FrontendDiagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug)]
pub struct FrontendOptions {
    pub enabled: bool,
    pub module_id: String,
    pub raw_limits: RawInputLimits,
    pub sealed_limits: SealedPlanLimits,
}

#[derive(Clone, Copy, Debug)]
pub struct SourceModule<'a> {
    pub module_id: &'a str,
    pub source: &'a str,
}

impl Default for FrontendOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            module_id: "main".to_string(),
            raw_limits: RawInputLimits::default(),
            sealed_limits: SealedPlanLimits::default(),
        }
    }
}

pub fn parse_bounded(
    source: &str,
    options: &FrontendOptions,
) -> Result<BoundedRawProgram, Vec<FrontendDiagnostic>> {
    if !options.enabled {
        return Err(vec![FrontendDiagnostic::new(
            DiagnosticCode::FeatureDisabled,
            "RFC-0003 syntax requires --experimental-relations",
            0,
            0,
            b"experimental-relations",
        )]);
    }
    if let Some(error) =
        single_module_envelope(source.len(), &options.module_id, options.raw_limits)
    {
        return Err(vec![error.owned(&options.module_id)]);
    }
    if !parser::valid_module_identity(&options.module_id) {
        return Err(vec![invalid_module_identity(&options.module_id)]);
    }
    parser::parse(source, &options.module_id, options.raw_limits).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| error.owned(&options.module_id))
            .collect()
    })
}

pub fn compile(
    source: &str,
    options: &FrontendOptions,
) -> Result<FrontendArtifacts, Vec<FrontendDiagnostic>> {
    let raw = parse_bounded(source, options)?;
    checker::check_and_seal(raw, options.sealed_limits)
}

pub fn compile_modules(
    modules: &[SourceModule<'_>],
    options: &FrontendOptions,
) -> Result<FrontendArtifacts, Vec<FrontendDiagnostic>> {
    if !options.enabled {
        return Err(vec![FrontendDiagnostic::new(
            DiagnosticCode::FeatureDisabled,
            "RFC-0003 syntax requires --experimental-relations",
            0,
            0,
            b"experimental-relations",
        )]);
    }
    let limits = options.raw_limits;
    if modules.len() > limits.max_modules {
        return Err(vec![FrontendDiagnostic::limit(
            DiagnosticCode::RawModuleLimit,
            modules.len(),
            limits.max_modules,
        )]);
    }
    let total_module_identifier_bytes = modules
        .iter()
        .map(|module| module.module_id.len())
        .fold(0usize, usize::saturating_add);
    let total_source_bytes = modules
        .iter()
        .map(|module| module.source.len())
        .fold(0usize, usize::saturating_add);
    let mut admission = Vec::new();
    if total_module_identifier_bytes > limits.max_total_module_identifier_bytes {
        admission.push(FrontendDiagnostic::limit(
            DiagnosticCode::RawTotalModuleIdentifierByteLimit,
            total_module_identifier_bytes,
            limits.max_total_module_identifier_bytes,
        ));
    }
    if total_source_bytes > limits.max_total_source_bytes {
        admission.push(FrontendDiagnostic::limit(
            DiagnosticCode::RawTotalSourceByteLimit,
            total_source_bytes,
            limits.max_total_source_bytes,
        ));
    }
    let mut module_sources = std::collections::BTreeMap::<&str, Vec<&str>>::new();
    for module in modules {
        if module.source.len() > limits.max_source_bytes {
            admission.push(
                FrontendDiagnostic::limit(
                    DiagnosticCode::RawSourceByteLimit,
                    module.source.len(),
                    limits.max_source_bytes,
                )
                .owned(module.module_id),
            );
        }
        if module.module_id.len() > limits.max_identifier_bytes {
            admission.push(
                FrontendDiagnostic::limit(
                    DiagnosticCode::RawIdentifierByteLimit,
                    module.module_id.len(),
                    limits.max_identifier_bytes,
                )
                .owned(module.module_id),
            );
        } else if !parser::valid_module_identity(module.module_id) {
            admission.push(
                FrontendDiagnostic::new(
                    DiagnosticCode::UnqualifiedModule,
                    "relation module identity must contain nonempty path segments",
                    0,
                    0,
                    module.module_id.as_bytes(),
                )
                .owned(module.module_id),
            );
        }
        module_sources
            .entry(module.module_id)
            .or_default()
            .push(module.source);
    }
    if let Some(error) = admission.into_iter().min() {
        return Err(vec![error]);
    }

    let mut duplicate_diagnostics = Vec::new();
    for (module_id, sources) in &module_sources {
        if sources.len() < 2 {
            continue;
        }
        let mut source_digests = sources
            .iter()
            .map(|source| <[u8; 32]>::from(Sha256::digest(source.as_bytes())))
            .collect::<Vec<_>>();
        source_digests.sort();
        let mut detail = Vec::new();
        detail.extend_from_slice(&(module_id.len() as u64).to_be_bytes());
        detail.extend_from_slice(module_id.as_bytes());
        detail.extend_from_slice(&(source_digests.len() as u64).to_be_bytes());
        for digest in &source_digests {
            detail.extend_from_slice(digest);
        }
        duplicate_diagnostics.push(
            FrontendDiagnostic::new(
                DiagnosticCode::DuplicateModule,
                "duplicate relation module identity",
                0,
                0,
                &detail,
            )
            .owned(module_id),
        );
    }
    if let Some(error) = duplicate_diagnostics.into_iter().min() {
        return Err(vec![error]);
    }

    let mut canonical_modules = modules.to_vec();
    canonical_modules.sort_by_key(|module| module.module_id);
    let mut relations = Vec::new();
    let mut relation_spans = Vec::new();
    let mut rules = Vec::new();
    let mut operations = Vec::new();
    let mut operation_spans = Vec::new();
    let mut stats = RawInputStats::default();
    let mut parse_diagnostics = Vec::new();
    for module in canonical_modules {
        let mut module_options = options.clone();
        module_options.module_id = module.module_id.to_string();
        let parsed = match parse_bounded(module.source, &module_options) {
            Ok(parsed) => parsed,
            Err(errors) => {
                parse_diagnostics.extend(
                    errors
                        .into_iter()
                        .map(|error| error.owned(module.module_id)),
                );
                continue;
            }
        };
        let module_stats = parsed.input_stats();
        stats.source_bytes = stats.source_bytes.saturating_add(module_stats.source_bytes);
        stats.tokens = stats.tokens.saturating_add(module_stats.tokens);
        stats.ast_nodes = stats.ast_nodes.saturating_add(module_stats.ast_nodes);
        stats.relations = stats.relations.saturating_add(module_stats.relations);
        stats.rules = stats.rules.saturating_add(module_stats.rules);
        stats.operations = stats.operations.saturating_add(module_stats.operations);
        stats.structural_cost = stats
            .structural_cost
            .saturating_add(module_stats.structural_cost);
        relations.extend(parsed.relations);
        relation_spans.extend(parsed.relation_spans);
        rules.extend(parsed.rules);
        operations.extend(parsed.operations);
        operation_spans.extend(parsed.operation_spans);
    }
    if let Some(error) = parse_diagnostics.into_iter().min() {
        return Err(vec![error]);
    }
    limits::validate_combined_stats(stats, limits).map_err(|error| vec![error])?;
    let raw = BoundedRawProgram::new(
        module_sources
            .keys()
            .map(|module| str::to_string(module))
            .collect(),
        relations,
        relation_spans,
        rules,
        operations,
        operation_spans,
        stats,
    );
    checker::check_and_seal(raw, options.sealed_limits)
}

/// Read and compile a source stream without first retaining unbounded input.
pub fn compile_reader<R: std::io::Read>(
    reader: R,
    options: &FrontendOptions,
) -> Result<FrontendArtifacts, Vec<FrontendDiagnostic>> {
    if !options.enabled {
        return Err(vec![FrontendDiagnostic::new(
            DiagnosticCode::FeatureDisabled,
            "RFC-0003 syntax requires --experimental-relations",
            0,
            0,
            b"experimental-relations",
        )]);
    }
    if let Some(error) = single_module_envelope(0, &options.module_id, options.raw_limits) {
        return Err(vec![error.owned(&options.module_id)]);
    }
    if !parser::valid_module_identity(&options.module_id) {
        return Err(vec![invalid_module_identity(&options.module_id)]);
    }
    let limit = options
        .raw_limits
        .max_source_bytes
        .min(options.raw_limits.max_total_source_bytes);
    let read_limit = limit.saturating_add(1) as u64;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            vec![FrontendDiagnostic::new(
                DiagnosticCode::Syntax,
                format!("failed to read relation source: {error}"),
                0,
                0,
                b"source-read",
            )]
        })?;
    if bytes.len() > limit {
        let error = single_module_envelope(bytes.len(), &options.module_id, options.raw_limits)
            .unwrap_or_else(|| {
                FrontendDiagnostic::limit(DiagnosticCode::RawSourceByteLimit, bytes.len(), limit)
            });
        return Err(vec![error.owned(&options.module_id)]);
    }
    let source = String::from_utf8(bytes).map_err(|_| {
        vec![FrontendDiagnostic::new(
            DiagnosticCode::Syntax,
            "relation source is not valid UTF-8",
            0,
            0,
            b"source-utf8",
        )]
    })?;
    compile(&source, options)
}

fn single_module_envelope(
    source_bytes: usize,
    module_id: &str,
    limits: RawInputLimits,
) -> Option<FrontendDiagnostic> {
    let checks = [
        (DiagnosticCode::RawModuleLimit, 1, limits.max_modules),
        (
            DiagnosticCode::RawTotalModuleIdentifierByteLimit,
            module_id.len(),
            limits.max_total_module_identifier_bytes,
        ),
        (
            DiagnosticCode::RawTotalSourceByteLimit,
            source_bytes,
            limits.max_total_source_bytes,
        ),
        (
            DiagnosticCode::RawSourceByteLimit,
            source_bytes,
            limits.max_source_bytes,
        ),
        (
            DiagnosticCode::RawIdentifierByteLimit,
            module_id.len(),
            limits.max_identifier_bytes,
        ),
    ];
    checks
        .into_iter()
        .filter(|(_, actual, limit)| actual > limit)
        .map(|(code, actual, limit)| FrontendDiagnostic::limit(code, actual, limit))
        .min()
}

fn invalid_module_identity(module_id: &str) -> FrontendDiagnostic {
    FrontendDiagnostic::new(
        DiagnosticCode::UnqualifiedModule,
        "relation module identity must contain nonempty path segments",
        0,
        0,
        module_id.as_bytes(),
    )
    .owned(module_id)
}

pub fn format_source(
    source: &str,
    options: &FrontendOptions,
) -> Result<String, Vec<FrontendDiagnostic>> {
    let raw = parse_bounded(source, options)?;
    Ok(tooling::format_program(&raw))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendSymbolKind {
    AuthoritativeRelation,
    DerivedRelation,
    Rule,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendSymbol {
    pub identity: String,
    pub kind: FrontendSymbolKind,
}

pub fn symbols(
    source: &str,
    options: &FrontendOptions,
) -> Result<Vec<FrontendSymbol>, Vec<FrontendDiagnostic>> {
    let artifacts = compile(source, options)?;
    Ok(tooling::symbols(&artifacts))
}

#[cfg(test)]
mod tests;
