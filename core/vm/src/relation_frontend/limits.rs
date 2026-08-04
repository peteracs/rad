use super::{DiagnosticCode, FrontendDiagnostic, RawInputStats, RawRuleSummary};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawInputLimits {
    pub max_modules: usize,
    pub max_total_module_identifier_bytes: usize,
    pub max_total_source_bytes: usize,
    pub max_source_bytes: usize,
    pub max_tokens: usize,
    pub max_ast_nodes: usize,
    pub max_relations: usize,
    pub max_rules: usize,
    pub max_operations: usize,
    pub max_identifier_bytes: usize,
    pub max_columns_per_relation: usize,
    pub max_unique_constraints_per_relation: usize,
    pub max_terms_per_rule: usize,
    pub max_atoms_per_rule: usize,
    pub max_predicates_per_rule: usize,
    pub max_aggregate_groups_per_rule: usize,
    pub max_structural_cost: usize,
}

impl Default for RawInputLimits {
    fn default() -> Self {
        Self {
            max_modules: 4_096,
            max_total_module_identifier_bytes: 4 * 1024 * 1024,
            max_total_source_bytes: 64 * 1024 * 1024,
            max_source_bytes: 16 * 1024 * 1024,
            max_tokens: 1_000_000,
            max_ast_nodes: 1_000_000,
            max_relations: 8_192,
            max_rules: 2_048,
            max_operations: 65_536,
            max_identifier_bytes: 4_096,
            max_columns_per_relation: 256,
            max_unique_constraints_per_relation: 256,
            max_terms_per_rule: 65_536,
            max_atoms_per_rule: 128,
            max_predicates_per_rule: 512,
            max_aggregate_groups_per_rule: 1_024,
            max_structural_cost: 32 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SealedPlanLimits {
    pub max_rules: usize,
    pub max_atoms_per_rule: usize,
    pub max_predicates_per_rule: usize,
    pub max_terms: usize,
    pub max_dependency_edges: usize,
    pub max_canonical_bytes: usize,
}

impl Default for SealedPlanLimits {
    fn default() -> Self {
        Self {
            max_rules: 1_024,
            max_atoms_per_rule: 64,
            max_predicates_per_rule: 256,
            max_terms: 65_536,
            max_dependency_edges: 16_384,
            max_canonical_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub(crate) struct RawInputMeter {
    limits: RawInputLimits,
    stats: RawInputStats,
}

impl RawInputMeter {
    pub(crate) fn new(
        source_bytes: usize,
        limits: RawInputLimits,
    ) -> Result<Self, FrontendDiagnostic> {
        if source_bytes > limits.max_source_bytes {
            return Err(FrontendDiagnostic::limit(
                DiagnosticCode::RawSourceByteLimit,
                source_bytes,
                limits.max_source_bytes,
            ));
        }
        Ok(Self {
            limits,
            stats: RawInputStats {
                source_bytes,
                ..RawInputStats::default()
            },
        })
    }

    pub(crate) fn limits(&self) -> RawInputLimits {
        self.limits
    }

    pub(crate) fn token(&mut self) -> Result<(), FrontendDiagnostic> {
        Self::charge(
            &mut self.stats.tokens,
            1,
            self.limits.max_tokens,
            DiagnosticCode::RawTokenLimit,
        )
    }

    pub(crate) fn ast_node(&mut self) -> Result<(), FrontendDiagnostic> {
        Self::charge(
            &mut self.stats.ast_nodes,
            1,
            self.limits.max_ast_nodes,
            DiagnosticCode::RawAstNodeLimit,
        )
    }

    pub(crate) fn relation(&mut self, next: usize) -> Result<(), FrontendDiagnostic> {
        Self::check(
            next,
            self.limits.max_relations,
            DiagnosticCode::RawRelationLimit,
        )?;
        self.stats.relations = next;
        Ok(())
    }

    pub(crate) fn rule(&mut self, next: usize) -> Result<(), FrontendDiagnostic> {
        Self::check(next, self.limits.max_rules, DiagnosticCode::RawRuleLimit)?;
        self.stats.rules = next;
        Ok(())
    }

    pub(crate) fn operation(&mut self, next: usize) -> Result<(), FrontendDiagnostic> {
        Self::check(
            next,
            self.limits.max_operations,
            DiagnosticCode::RawOperationLimit,
        )?;
        self.stats.operations = next;
        Ok(())
    }

    pub(crate) fn structural(&mut self, bytes: usize) -> Result<(), FrontendDiagnostic> {
        Self::charge(
            &mut self.stats.structural_cost,
            bytes,
            self.limits.max_structural_cost,
            DiagnosticCode::RawStructuralCostLimit,
        )
    }

    pub(crate) fn finish(self) -> RawInputStats {
        self.stats
    }

    pub(crate) fn check_summary(&self, summary: RawRuleSummary) -> Vec<FrontendDiagnostic> {
        let mut diagnostics = Vec::new();
        let checks = [
            (
                DiagnosticCode::RawIdentifierByteLimit,
                summary.maximum_identifier_length,
                self.limits.max_identifier_bytes,
            ),
            (
                DiagnosticCode::RawTermLimit,
                summary.total_terms,
                self.limits.max_terms_per_rule,
            ),
            (
                DiagnosticCode::RawAtomLimit,
                summary.atoms,
                self.limits.max_atoms_per_rule,
            ),
            (
                DiagnosticCode::RawPredicateLimit,
                summary.predicates,
                self.limits.max_predicates_per_rule,
            ),
            (
                DiagnosticCode::RawAggregateGroupLimit,
                summary.aggregate_groups,
                self.limits.max_aggregate_groups_per_rule,
            ),
        ];
        for (code, actual, limit) in checks {
            if actual > limit {
                diagnostics.push(FrontendDiagnostic::limit(code, actual, limit));
            }
        }
        diagnostics
    }

    fn charge(
        counter: &mut usize,
        amount: usize,
        limit: usize,
        code: DiagnosticCode,
    ) -> Result<(), FrontendDiagnostic> {
        let next = counter.saturating_add(amount);
        Self::check(next, limit, code)?;
        *counter = next;
        Ok(())
    }

    fn check(actual: usize, limit: usize, code: DiagnosticCode) -> Result<(), FrontendDiagnostic> {
        if actual > limit {
            Err(FrontendDiagnostic::limit(code, actual, limit))
        } else {
            Ok(())
        }
    }
}

pub(crate) fn validate_stats(
    stats: RawInputStats,
    limits: RawInputLimits,
) -> Result<(), FrontendDiagnostic> {
    let checks = [
        (
            DiagnosticCode::RawSourceByteLimit,
            stats.source_bytes,
            limits.max_source_bytes,
        ),
        (
            DiagnosticCode::RawTokenLimit,
            stats.tokens,
            limits.max_tokens,
        ),
        (
            DiagnosticCode::RawRelationLimit,
            stats.relations,
            limits.max_relations,
        ),
        (DiagnosticCode::RawRuleLimit, stats.rules, limits.max_rules),
        (
            DiagnosticCode::RawOperationLimit,
            stats.operations,
            limits.max_operations,
        ),
        (
            DiagnosticCode::RawAstNodeLimit,
            stats.ast_nodes,
            limits.max_ast_nodes,
        ),
        (
            DiagnosticCode::RawStructuralCostLimit,
            stats.structural_cost,
            limits.max_structural_cost,
        ),
    ];
    checks
        .into_iter()
        .filter(|(_, actual, limit)| actual > limit)
        .map(|(code, actual, limit)| FrontendDiagnostic::limit(code, actual, limit))
        .min()
        .map_or(Ok(()), Err)
}

pub(crate) fn validate_combined_stats(
    stats: RawInputStats,
    limits: RawInputLimits,
) -> Result<(), FrontendDiagnostic> {
    let combined_limits = RawInputLimits {
        max_source_bytes: limits.max_total_source_bytes,
        ..limits
    };
    validate_stats(stats, combined_limits)
}
