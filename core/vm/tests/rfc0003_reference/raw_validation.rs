// Bounded raw-rule admission and canonical invalid-plan diagnostics.
//
// This is the trust boundary in front of typed plan sealing. Collection
// lengths and identifier sizes are checked before any body is hashed; only a
// raw rule set admitted by this envelope receives complete fingerprints.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RawRuleInputStats {
    source_bytes: usize,
    tokens: usize,
    rules_seen: usize,
}

impl RawRuleInputStats {
    fn for_rules(rules: &[RulePlan]) -> Self {
        Self {
            rules_seen: rules.len(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawRuleInputLimits {
    max_source_bytes: usize,
    max_tokens: usize,
    max_ast_nodes: usize,
    max_rules: usize,
    max_identifier_bytes: usize,
    max_terms_per_rule: usize,
    max_atoms_per_rule: usize,
    max_predicates_per_rule: usize,
    max_aggregate_groups_per_rule: usize,
    max_total_structural_cost: usize,
    max_body_node_visits: usize,
}

impl RawRuleInputLimits {
    fn generous() -> Self {
        Self {
            max_source_bytes: 16 * 1024 * 1024,
            max_tokens: 1_000_000,
            max_ast_nodes: 1_000_000,
            max_rules: 2_048,
            max_identifier_bytes: 4_096,
            max_terms_per_rule: 65_536,
            max_atoms_per_rule: 128,
            max_predicates_per_rule: 512,
            max_aggregate_groups_per_rule: 1_024,
            max_total_structural_cost: 32 * 1024 * 1024,
            max_body_node_visits: 2_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RawValidationUsage {
    header_visits: usize,
    shape_visits: usize,
    body_node_visits: usize,
    ast_nodes: usize,
    structural_cost: usize,
    complete_fingerprints: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RulePlanLimits {
    max_rules: usize,
    max_atoms_per_rule: usize,
    max_predicates_per_rule: usize,
    max_terms: usize,
    max_canonical_plan_bytes: usize,
    max_dependency_edges: usize,
}

impl RulePlanLimits {
    fn generous() -> Self {
        Self {
            max_rules: 1_024,
            max_atoms_per_rule: 64,
            max_predicates_per_rule: 256,
            max_terms: 65_536,
            max_canonical_plan_bytes: 16 * 1024 * 1024,
            max_dependency_edges: 16_384,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuleDiagnosticCode {
    RawSourceByteLimit,
    RawTokenLimit,
    RawRuleLimit,
    EmptyId,
    UnqualifiedId,
    RawIdentifierByteLimit,
    RawTermLimit,
    RawAtomLimit,
    RawPredicateLimit,
    RawAggregateGroupLimit,
    RawAstNodeLimit,
    RawStructuralCostLimit,
    RawValidationWorkLimit,
    RuleLimit,
    DuplicateId,
    AtomLimit,
    PredicateLimit,
    TermLimit,
    CanonicalByteLimit,
    DependencyEdgeLimit,
}

impl RuleDiagnosticCode {
    const fn priority(self) -> u8 {
        match self {
            Self::RawSourceByteLimit => 0,
            Self::RawTokenLimit => 1,
            Self::RawRuleLimit => 2,
            Self::EmptyId => 3,
            Self::UnqualifiedId => 4,
            Self::RawIdentifierByteLimit => 5,
            Self::RawTermLimit => 6,
            Self::RawAtomLimit => 7,
            Self::RawPredicateLimit => 8,
            Self::RawAggregateGroupLimit => 9,
            Self::RawAstNodeLimit => 10,
            Self::RawStructuralCostLimit => 11,
            Self::RawValidationWorkLimit => 12,
            Self::RuleLimit => 20,
            Self::DuplicateId => 21,
            Self::AtomLimit => 22,
            Self::PredicateLimit => 23,
            Self::TermLimit => 24,
            Self::CanonicalByteLimit => 25,
            Self::DependencyEdgeLimit => 26,
        }
    }

    fn error(self) -> &'static str {
        match self {
            Self::RawSourceByteLimit => "derivation.raw_source_byte_limit",
            Self::RawTokenLimit => "derivation.raw_token_limit",
            Self::RawRuleLimit => "derivation.raw_rule_limit",
            Self::EmptyId => "derivation.empty_rule_id",
            Self::UnqualifiedId => "derivation.unqualified_rule_id",
            Self::RawIdentifierByteLimit => "derivation.raw_identifier_byte_limit",
            Self::RawTermLimit => "derivation.raw_term_limit",
            Self::RawAtomLimit => "derivation.raw_atom_limit",
            Self::RawPredicateLimit => "derivation.raw_predicate_limit",
            Self::RawAggregateGroupLimit => "derivation.raw_aggregate_group_limit",
            Self::RawAstNodeLimit => "derivation.raw_ast_node_limit",
            Self::RawStructuralCostLimit => "derivation.raw_structural_cost_limit",
            Self::RawValidationWorkLimit => "derivation.raw_validation_work_limit",
            Self::RuleLimit => "derivation.rule_limit",
            Self::DuplicateId => "derivation.duplicate_rule_id",
            Self::AtomLimit => "derivation.atom_limit",
            Self::PredicateLimit => "derivation.predicate_limit",
            Self::TermLimit => "derivation.term_limit",
            Self::CanonicalByteLimit => "derivation.rule_plan_byte_limit",
            Self::DependencyEdgeLimit => "derivation.dependency_edge_limit",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuleDiagnostic {
    code: RuleDiagnosticCode,
    rule_fingerprint: [u8; 32],
    detail_key: [u8; 32],
}

impl Ord for RuleDiagnostic {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.code.priority(), self.rule_fingerprint, self.detail_key).cmp(&(
            other.code.priority(),
            other.rule_fingerprint,
            other.detail_key,
        ))
    }
}

impl PartialOrd for RuleDiagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuleDiagnosticSelection {
    diagnostic: Option<RuleDiagnostic>,
    usage: RawValidationUsage,
}

#[derive(Clone, Copy, Debug)]
struct RawRuleSummary {
    fingerprint: [u8; 32],
    atoms: usize,
    predicates: usize,
    terms: usize,
    canonical_bytes: usize,
}

fn checked_raw_add(left: usize, right: usize) -> usize {
    left.saturating_add(right)
}

fn hash_text(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn hash_value(hasher: &mut Sha256, value: &FactValue) {
    match value {
        FactValue::Int(value) => {
            hasher.update(b"i");
            hasher.update(value.to_be_bytes());
        }
        FactValue::Count(value) => {
            hasher.update(b"c");
            hasher.update(value.to_be_bytes());
        }
        FactValue::Entity(entity) => {
            hasher.update(b"e");
            hasher.update(entity.slot.to_be_bytes());
            hasher.update(entity.generation.to_be_bytes());
        }
        FactValue::Text(value) => {
            hasher.update(b"t");
            hash_text(hasher, value);
        }
    }
}

fn hash_term(hasher: &mut Sha256, term: &Term) {
    match term {
        Term::Variable(name) => {
            hasher.update(b"v");
            hash_text(hasher, name);
        }
        Term::Constant(value) => {
            hasher.update(b"c");
            hash_value(hasher, value);
        }
    }
}

fn atom_fingerprint(atom: &Atom) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rfc0003.raw-atom.v2");
    hash_text(&mut hasher, &atom.relation);
    hasher.update((atom.terms.len() as u64).to_be_bytes());
    for term in &atom.terms {
        hash_term(&mut hasher, term);
    }
    hasher.finalize().into()
}

fn predicate_fingerprint(predicate: &Predicate) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rfc0003.raw-predicate.v2");
    match predicate {
        Predicate::Greater(left, right) => {
            hasher.update(b"g");
            hash_text(&mut hasher, left);
            hash_text(&mut hasher, right);
        }
    }
    hasher.finalize().into()
}

fn exact_digest_multiset(domain: &[u8], mut digests: Vec<[u8; 32]>) -> [u8; 32] {
    digests.sort();
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update((digests.len() as u64).to_be_bytes());
    for digest in digests {
        hasher.update(32_u64.to_be_bytes());
        hasher.update(digest);
    }
    hasher.finalize().into()
}

fn raw_rule_fingerprint(rule: &RulePlan) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rfc0003.raw-rule-plan.v2");
    hash_text(&mut hasher, &rule.id);
    hash_text(&mut hasher, &rule.head_relation);
    hasher.update((rule.head.len() as u64).to_be_bytes());
    for term in &rule.head {
        hash_term(&mut hasher, term);
    }
    hasher.update(exact_digest_multiset(
        b"rfc0003.raw-rule-atoms.v2",
        rule.atoms.iter().map(atom_fingerprint).collect(),
    ));
    hasher.update(exact_digest_multiset(
        b"rfc0003.raw-rule-predicates.v2",
        rule.predicates.iter().map(predicate_fingerprint).collect(),
    ));
    match &rule.aggregate {
        None => hasher.update([0]),
        Some(aggregate) => {
            hasher.update([1]);
            hasher.update([match aggregate.kind {
                AggregateKind::Count => b'c',
                AggregateKind::Sum => b's',
                AggregateKind::Min => b'n',
                AggregateKind::Max => b'x',
            }]);
            match &aggregate.input {
                None => hasher.update([0]),
                Some(input) => {
                    hasher.update([1]);
                    hash_text(&mut hasher, input);
                }
            }
            hash_text(&mut hasher, &aggregate.output);
            let groups = aggregate
                .group_by
                .iter()
                .map(|group| {
                    let mut group_hasher = Sha256::new();
                    hash_text(&mut group_hasher, group);
                    group_hasher.finalize().into()
                })
                .collect();
            hasher.update(exact_digest_multiset(b"rfc0003.raw-rule-groups.v2", groups));
        }
    }
    hasher.finalize().into()
}

fn bounded_header_fingerprint(rule: &RulePlan) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rfc0003.bounded-rule-header.v1");
    hash_text(&mut hasher, &rule.id);
    hash_text(&mut hasher, &rule.head_relation);
    hasher.update((rule.head.len() as u64).to_be_bytes());
    hasher.update((rule.atoms.len() as u64).to_be_bytes());
    hasher.update((rule.predicates.len() as u64).to_be_bytes());
    hasher.update(
        (rule
            .aggregate
            .as_ref()
            .map_or(0, |aggregate| aggregate.group_by.len()) as u64)
            .to_be_bytes(),
    );
    hasher.finalize().into()
}

fn set_limit_witness(code: RuleDiagnosticCode) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rfc0003.bounded-limit-witness.v1");
    hasher.update([code.priority()]);
    hasher.finalize().into()
}

fn diagnostic_detail(code: RuleDiagnosticCode, actual: usize, limit: usize) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rfc0003.rule-diagnostic-detail.v2");
    hasher.update([code.priority()]);
    hasher.update((actual as u64).to_be_bytes());
    hasher.update((limit as u64).to_be_bytes());
    hasher.finalize().into()
}

fn diagnostic(
    code: RuleDiagnosticCode,
    rule_fingerprint: [u8; 32],
    actual: usize,
    limit: usize,
) -> RuleDiagnostic {
    RuleDiagnostic {
        code,
        rule_fingerprint,
        detail_key: diagnostic_detail(code, actual, limit),
    }
}

fn set_limit_diagnostic(code: RuleDiagnosticCode, actual: usize, limit: usize) -> RuleDiagnostic {
    diagnostic(code, set_limit_witness(code), actual, limit)
}

fn offer(best: &mut Option<RuleDiagnostic>, candidate: RuleDiagnostic) {
    if best.as_ref().is_none_or(|current| candidate < *current) {
        *best = Some(candidate);
    }
}

fn measure_term(term: &Term, identifier_max: &mut usize) -> usize {
    match term {
        Term::Variable(name) => {
            *identifier_max = (*identifier_max).max(name.len());
            checked_raw_add(1, checked_raw_add(8, name.len()))
        }
        Term::Constant(FactValue::Text(value)) => {
            checked_raw_add(1, checked_raw_add(9, value.len()))
        }
        Term::Constant(_) => 10,
    }
}

fn measure_rule(rule: &RulePlan) -> (usize, usize) {
    let mut identifier_max = rule.id.len().max(rule.head_relation.len());
    let mut bytes = checked_raw_add(
        b"rfc0003.rule-plan.v1".len(),
        checked_raw_add(
            checked_raw_add(8, rule.id.len()),
            checked_raw_add(8, rule.head_relation.len()),
        ),
    );
    bytes = checked_raw_add(bytes, 8);
    for term in &rule.head {
        bytes = checked_raw_add(bytes, measure_term(term, &mut identifier_max));
    }
    bytes = checked_raw_add(bytes, 8);
    for atom in &rule.atoms {
        identifier_max = identifier_max.max(atom.relation.len());
        // Framed atom + relation text + term count.
        bytes = checked_raw_add(bytes, checked_raw_add(24, atom.relation.len()));
        for term in &atom.terms {
            bytes = checked_raw_add(bytes, measure_term(term, &mut identifier_max));
        }
    }
    bytes = checked_raw_add(bytes, 8);
    for predicate in &rule.predicates {
        match predicate {
            Predicate::Greater(left, right) => {
                identifier_max = identifier_max.max(left.len()).max(right.len());
                // Framed predicate + tag + two framed identifiers.
                bytes = checked_raw_add(
                    bytes,
                    checked_raw_add(25, checked_raw_add(left.len(), right.len())),
                );
            }
        }
    }
    bytes = checked_raw_add(bytes, 1);
    if let Some(aggregate) = &rule.aggregate {
        // The aggregate-presence tag is above; add kind and input presence.
        bytes = checked_raw_add(bytes, 2);
        if let Some(input) = &aggregate.input {
            identifier_max = identifier_max.max(input.len());
            bytes = checked_raw_add(bytes, checked_raw_add(8, input.len()));
        }
        identifier_max = identifier_max.max(aggregate.output.len());
        bytes = checked_raw_add(bytes, checked_raw_add(16, aggregate.output.len()));
        for group in &aggregate.group_by {
            identifier_max = identifier_max.max(group.len());
            bytes = checked_raw_add(bytes, checked_raw_add(8, group.len()));
        }
    }
    (bytes, identifier_max)
}

fn duplicate_group_witness(id: &str, fingerprints: &[[u8; 32]]) -> [u8; 32] {
    let mut fingerprints = fingerprints.to_vec();
    fingerprints.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"rfc0003.duplicate-rule-group.v1");
    hash_text(&mut hasher, id);
    hasher.update((fingerprints.len() as u64).to_be_bytes());
    for fingerprint in fingerprints {
        hasher.update(32_u64.to_be_bytes());
        hasher.update(fingerprint);
    }
    hasher.finalize().into()
}

fn analyze_rule_diagnostics(
    rules: &[RulePlan],
    input: RawRuleInputStats,
    raw_limits: RawRuleInputLimits,
    plan_limits: RulePlanLimits,
) -> RuleDiagnosticSelection {
    let mut usage = RawValidationUsage::default();
    let immediate = [
        (
            RuleDiagnosticCode::RawSourceByteLimit,
            input.source_bytes,
            raw_limits.max_source_bytes,
        ),
        (
            RuleDiagnosticCode::RawTokenLimit,
            input.tokens,
            raw_limits.max_tokens,
        ),
        (
            RuleDiagnosticCode::RawRuleLimit,
            input.rules_seen.max(rules.len()),
            raw_limits.max_rules,
        ),
    ];
    for (code, actual, limit) in immediate {
        if actual > limit {
            return RuleDiagnosticSelection {
                diagnostic: Some(set_limit_diagnostic(code, actual, limit)),
                usage,
            };
        }
    }

    // Raw rule-count admission makes this complete pass inherently bounded.
    // It is deliberately not charged against the later body-work budget:
    // every admitted header must participate in canonical diagnostic choice.
    let mut header_diagnostic = None;
    for rule in rules {
        usage.header_visits += 1;
        let id_bounded = rule.id.len() <= raw_limits.max_identifier_bytes;
        let head_bounded = rule.head_relation.len() <= raw_limits.max_identifier_bytes;
        let header = if id_bounded && head_bounded {
            bounded_header_fingerprint(rule)
        } else {
            set_limit_witness(RuleDiagnosticCode::RawIdentifierByteLimit)
        };
        if rule.id.is_empty() {
            offer(
                &mut header_diagnostic,
                diagnostic(RuleDiagnosticCode::EmptyId, header, 0, 1),
            );
        } else if id_bounded && !rule.id.contains('.') && !rule.id.contains("::") {
            offer(
                &mut header_diagnostic,
                diagnostic(RuleDiagnosticCode::UnqualifiedId, header, rule.id.len(), 0),
            );
        }
        if !id_bounded || !head_bounded {
            offer(
                &mut header_diagnostic,
                set_limit_diagnostic(
                    RuleDiagnosticCode::RawIdentifierByteLimit,
                    rule.id.len().max(rule.head_relation.len()),
                    raw_limits.max_identifier_bytes,
                ),
            );
        }
    }
    if let Some(diagnostic) = header_diagnostic {
        return RuleDiagnosticSelection {
            diagnostic: Some(diagnostic),
            usage,
        };
    }

    // Shape inspection is also complete. Its maximum work is derived from
    // max_rules * (1 + max_atoms_per_rule), so no independent profile knob can
    // truncate it and make a raw shape diagnostic registration-order dependent.
    let mut shape_diagnostic = None;
    let mut ast_nodes = 0usize;
    for rule in rules {
        usage.shape_visits = checked_raw_add(usage.shape_visits, 1);
        let header = bounded_header_fingerprint(rule);
        if rule.atoms.len() > raw_limits.max_atoms_per_rule {
            offer(
                &mut shape_diagnostic,
                diagnostic(
                    RuleDiagnosticCode::RawAtomLimit,
                    header,
                    rule.atoms.len(),
                    raw_limits.max_atoms_per_rule,
                ),
            );
        }
        if rule.predicates.len() > raw_limits.max_predicates_per_rule {
            offer(
                &mut shape_diagnostic,
                diagnostic(
                    RuleDiagnosticCode::RawPredicateLimit,
                    header,
                    rule.predicates.len(),
                    raw_limits.max_predicates_per_rule,
                ),
            );
        }
        let groups = rule
            .aggregate
            .as_ref()
            .map_or(0, |aggregate| aggregate.group_by.len());
        if groups > raw_limits.max_aggregate_groups_per_rule {
            offer(
                &mut shape_diagnostic,
                diagnostic(
                    RuleDiagnosticCode::RawAggregateGroupLimit,
                    header,
                    groups,
                    raw_limits.max_aggregate_groups_per_rule,
                ),
            );
        }
        if rule.atoms.len() <= raw_limits.max_atoms_per_rule {
            usage.shape_visits = checked_raw_add(usage.shape_visits, rule.atoms.len());
            let terms = rule.atoms.iter().fold(rule.head.len(), |total, atom| {
                checked_raw_add(total, atom.terms.len())
            });
            if terms > raw_limits.max_terms_per_rule {
                offer(
                    &mut shape_diagnostic,
                    diagnostic(
                        RuleDiagnosticCode::RawTermLimit,
                        header,
                        terms,
                        raw_limits.max_terms_per_rule,
                    ),
                );
            }
            let rule_nodes = [
                1,
                terms,
                rule.atoms.len(),
                rule.predicates.len(),
                groups,
                usize::from(rule.aggregate.is_some()),
            ]
            .into_iter()
            .fold(0usize, checked_raw_add);
            ast_nodes = checked_raw_add(ast_nodes, rule_nodes);
        }
    }
    if let Some(diagnostic) = shape_diagnostic {
        return RuleDiagnosticSelection {
            diagnostic: Some(diagnostic),
            usage,
        };
    }
    usage.ast_nodes = ast_nodes;
    if ast_nodes > raw_limits.max_ast_nodes {
        return RuleDiagnosticSelection {
            diagnostic: Some(set_limit_diagnostic(
                RuleDiagnosticCode::RawAstNodeLimit,
                ast_nodes,
                raw_limits.max_ast_nodes,
            )),
            usage,
        };
    }
    let predicted_visits = ast_nodes.saturating_mul(2);
    if predicted_visits > raw_limits.max_body_node_visits {
        return RuleDiagnosticSelection {
            diagnostic: Some(set_limit_diagnostic(
                RuleDiagnosticCode::RawValidationWorkLimit,
                predicted_visits,
                raw_limits.max_body_node_visits,
            )),
            usage,
        };
    }

    let mut canonical_bytes = 0usize;
    let mut identifier_max = 0usize;
    let mut measurements = Vec::with_capacity(rules.len());
    for rule in rules {
        let (bytes, rule_identifier_max) = measure_rule(rule);
        canonical_bytes = checked_raw_add(canonical_bytes, bytes);
        identifier_max = identifier_max.max(rule_identifier_max);
        measurements.push(bytes);
    }
    usage.body_node_visits = ast_nodes;
    usage.structural_cost = checked_raw_add(canonical_bytes, ast_nodes.saturating_mul(8));
    if identifier_max > raw_limits.max_identifier_bytes {
        return RuleDiagnosticSelection {
            diagnostic: Some(set_limit_diagnostic(
                RuleDiagnosticCode::RawIdentifierByteLimit,
                identifier_max,
                raw_limits.max_identifier_bytes,
            )),
            usage,
        };
    }
    if usage.structural_cost > raw_limits.max_total_structural_cost {
        return RuleDiagnosticSelection {
            diagnostic: Some(set_limit_diagnostic(
                RuleDiagnosticCode::RawStructuralCostLimit,
                usage.structural_cost,
                raw_limits.max_total_structural_cost,
            )),
            usage,
        };
    }

    let mut summaries = Vec::with_capacity(rules.len());
    let mut ids = BTreeMap::<&str, Vec<[u8; 32]>>::new();
    let mut dependency_edges = BTreeSet::new();
    for (rule, canonical_bytes) in rules.iter().zip(measurements) {
        let fingerprint = raw_rule_fingerprint(rule);
        let terms = rule.atoms.iter().fold(rule.head.len(), |total, atom| {
            checked_raw_add(total, atom.terms.len())
        });
        for atom in &rule.atoms {
            dependency_edges.insert((rule.head_relation.as_str(), atom.relation.as_str()));
        }
        ids.entry(&rule.id).or_default().push(fingerprint);
        summaries.push(RawRuleSummary {
            fingerprint,
            atoms: rule.atoms.len(),
            predicates: rule.predicates.len(),
            terms,
            canonical_bytes,
        });
    }
    usage.body_node_visits = predicted_visits;
    usage.complete_fingerprints = summaries.len();
    let rule_set_fingerprint = exact_digest_multiset(
        b"rfc0003.raw-rule-plan-multiset.v2",
        summaries
            .iter()
            .map(|summary| summary.fingerprint)
            .collect(),
    );
    let mut best = None;
    if rules.len() > plan_limits.max_rules {
        offer(
            &mut best,
            diagnostic(
                RuleDiagnosticCode::RuleLimit,
                rule_set_fingerprint,
                rules.len(),
                plan_limits.max_rules,
            ),
        );
    }
    for (id, fingerprints) in ids {
        if fingerprints.len() > 1 {
            offer(
                &mut best,
                diagnostic(
                    RuleDiagnosticCode::DuplicateId,
                    duplicate_group_witness(id, &fingerprints),
                    fingerprints.len(),
                    1,
                ),
            );
        }
    }
    for summary in &summaries {
        if summary.atoms > plan_limits.max_atoms_per_rule {
            offer(
                &mut best,
                diagnostic(
                    RuleDiagnosticCode::AtomLimit,
                    summary.fingerprint,
                    summary.atoms,
                    plan_limits.max_atoms_per_rule,
                ),
            );
        }
        if summary.predicates > plan_limits.max_predicates_per_rule {
            offer(
                &mut best,
                diagnostic(
                    RuleDiagnosticCode::PredicateLimit,
                    summary.fingerprint,
                    summary.predicates,
                    plan_limits.max_predicates_per_rule,
                ),
            );
        }
    }
    let total_terms = summaries.iter().fold(0usize, |total, summary| {
        checked_raw_add(total, summary.terms)
    });
    if total_terms > plan_limits.max_terms {
        offer(
            &mut best,
            diagnostic(
                RuleDiagnosticCode::TermLimit,
                rule_set_fingerprint,
                total_terms,
                plan_limits.max_terms,
            ),
        );
    }
    let total_canonical_bytes = summaries.iter().fold(0usize, |total, summary| {
        checked_raw_add(total, summary.canonical_bytes)
    });
    if total_canonical_bytes > plan_limits.max_canonical_plan_bytes {
        offer(
            &mut best,
            diagnostic(
                RuleDiagnosticCode::CanonicalByteLimit,
                rule_set_fingerprint,
                total_canonical_bytes,
                plan_limits.max_canonical_plan_bytes,
            ),
        );
    }
    if dependency_edges.len() > plan_limits.max_dependency_edges {
        offer(
            &mut best,
            diagnostic(
                RuleDiagnosticCode::DependencyEdgeLimit,
                rule_set_fingerprint,
                dependency_edges.len(),
                plan_limits.max_dependency_edges,
            ),
        );
    }
    RuleDiagnosticSelection {
        diagnostic: best,
        usage,
    }
}

fn select_rule_diagnostic(
    rules: &[RulePlan],
    raw_limits: RawRuleInputLimits,
    plan_limits: RulePlanLimits,
) -> Option<RuleDiagnostic> {
    analyze_rule_diagnostics(
        rules,
        RawRuleInputStats::for_rules(rules),
        raw_limits,
        plan_limits,
    )
    .diagnostic
}
