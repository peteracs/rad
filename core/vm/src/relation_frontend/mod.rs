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
    RelationColumn, RelationManifest, RelationOperation, RelationOperationKind, RelationSchema,
    RelationType, SealedRulePlan, StaticResourceQuote, UniqueConstraint,
};
pub use limits::{RawInputLimits, SealedPlanLimits};

use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticCode {
    FeatureDisabled,
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
    DuplicateRelation,
    DuplicateColumn,
    DuplicateUniqueConstraint,
    UnknownUniqueColumn,
    SymmetricShape,
    SymmetricUnique,
    SymmetricEndpointMetadata,
    NamespaceCollision,
    UnknownRelation,
    Arity,
    TypeMismatch,
    UnboundVariable,
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
            Self::DuplicateRelation => "relations.duplicate_relation",
            Self::DuplicateColumn => "relations.duplicate_column",
            Self::DuplicateUniqueConstraint => "relations.duplicate_unique_constraint",
            Self::UnknownUniqueColumn => "relations.unknown_unique_column",
            Self::SymmetricShape => "relations.symmetric_shape",
            Self::SymmetricUnique => "relations.symmetric_unique_forbidden",
            Self::SymmetricEndpointMetadata => "relations.symmetric_endpoint_metadata",
            Self::NamespaceCollision => "relations.namespace_collision",
            Self::UnknownRelation => "relations.unknown_relation",
            Self::Arity => "relations.arity",
            Self::TypeMismatch => "relations.type_mismatch",
            Self::UnboundVariable => "relations.unbound_variable",
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
            Self::RawSourceByteLimit => 1,
            Self::RawTokenLimit => 2,
            Self::RawRelationLimit => 3,
            Self::RawRuleLimit => 4,
            Self::RawOperationLimit => 5,
            Self::RawColumnLimit => 6,
            Self::RawUniqueConstraintLimit => 7,
            Self::RawTupleLimit => 8,
            Self::EmptyIdentifier => 9,
            Self::UnqualifiedModule => 10,
            Self::RawIdentifierByteLimit => 11,
            Self::RawTermLimit => 12,
            Self::RawAtomLimit => 13,
            Self::RawPredicateLimit => 14,
            Self::RawAggregateGroupLimit => 15,
            Self::RawAstNodeLimit => 16,
            Self::RawStructuralCostLimit => 17,
            Self::Syntax => 20,
            Self::DuplicateModule => 21,
            Self::DuplicateRelation => 22,
            Self::DuplicateColumn => 23,
            Self::DuplicateUniqueConstraint => 24,
            Self::UnknownUniqueColumn => 25,
            Self::SymmetricShape => 26,
            Self::SymmetricUnique => 27,
            Self::SymmetricEndpointMetadata => 28,
            Self::NamespaceCollision => 29,
            Self::UnknownRelation => 30,
            Self::Arity => 31,
            Self::TypeMismatch => 32,
            Self::UnboundVariable => 33,
            Self::AggregateRequiresPositiveInput => 34,
            Self::AggregateOutputNotFresh => 35,
            Self::AggregateHeadProjection => 36,
            Self::AggregateType => 37,
            Self::RecursiveDerivation => 38,
            Self::DuplicateRule => 39,
            Self::SealedRuleLimit => 40,
            Self::SealedAtomLimit => 41,
            Self::SealedPredicateLimit => 42,
            Self::SealedTermLimit => 43,
            Self::SealedDependencyLimit => 44,
            Self::SealedByteLimit => 45,
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
        }
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
    parser::parse(source, &options.module_id, options.raw_limits)
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
    let mut module_ids = std::collections::BTreeSet::new();
    let mut relations = Vec::new();
    let mut rules = Vec::new();
    let mut operations = Vec::new();
    let mut stats = RawInputStats::default();
    for module in modules {
        if module.module_id.len() > options.raw_limits.max_identifier_bytes {
            return Err(vec![FrontendDiagnostic::limit(
                DiagnosticCode::RawIdentifierByteLimit,
                module.module_id.len(),
                options.raw_limits.max_identifier_bytes,
            )]);
        }
        if !module_ids.insert(module.module_id) {
            return Err(vec![FrontendDiagnostic::new(
                DiagnosticCode::DuplicateModule,
                "duplicate relation module identity",
                0,
                0,
                module.module_id.as_bytes(),
            )]);
        }
        let mut module_options = options.clone();
        module_options.module_id = module.module_id.to_string();
        let parsed = parse_bounded(module.source, &module_options)?;
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
        limits::validate_stats(stats, options.raw_limits).map_err(|error| vec![error])?;
        relations.extend(parsed.relations);
        rules.extend(parsed.rules);
        operations.extend(parsed.operations);
    }
    let raw = BoundedRawProgram::new(
        module_ids.into_iter().map(str::to_string).collect(),
        relations,
        rules,
        operations,
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
    let limit = options.raw_limits.max_source_bytes;
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
        return Err(vec![FrontendDiagnostic::limit(
            DiagnosticCode::RawSourceByteLimit,
            bytes.len(),
            limit,
        )]);
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
