use super::ast::*;
use super::canonical;
use super::limits::SealedPlanLimits;
use super::{DiagnosticCode, FrontendArtifacts, FrontendDiagnostic};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(crate) fn check_and_seal(
    mut raw: BoundedRawProgram,
    limits: SealedPlanLimits,
) -> Result<FrontendArtifacts, Vec<FrontendDiagnostic>> {
    let mut diagnostics = Vec::new();
    let mut schemas = BTreeMap::<String, RelationSchema>::new();
    for schema in raw.relations {
        validate_schema(&schema, &mut diagnostics);
        if schemas
            .insert(schema.identity.clone(), schema.clone())
            .is_some()
        {
            diagnostics.push(diagnostic(
                DiagnosticCode::DuplicateRelation,
                &schema.identity,
                "duplicate authoritative relation",
            ));
        }
    }

    let mut rules_by_head = BTreeMap::<String, Vec<&BoundedRawRule>>::new();
    for rule in &raw.rules {
        rules_by_head
            .entry(rule.ast.head_relation.clone())
            .or_default()
            .push(rule);
    }
    for head in rules_by_head.keys() {
        if schemas.contains_key(head) {
            diagnostics.push(diagnostic(
                DiagnosticCode::NamespaceCollision,
                head,
                "authoritative and derived relations share one namespace",
            ));
        }
    }
    if let Some(error) = diagnostics.iter().min().cloned() {
        return Err(vec![error]);
    }

    let derived_heads = rules_by_head.keys().cloned().collect::<BTreeSet<_>>();
    let unknown = raw
        .rules
        .iter()
        .flat_map(|rule| &rule.ast.atoms)
        .filter(|atom| {
            !schemas.contains_key(&atom.relation) && !derived_heads.contains(&atom.relation)
        })
        .map(|atom| {
            diagnostic(
                DiagnosticCode::UnknownRelation,
                &atom.relation,
                "rule atom names an unknown relation",
            )
        })
        .min();
    if let Some(error) = unknown {
        return Err(vec![error]);
    }
    let mut edges = BTreeSet::<(String, String)>::new();
    for (head, rules) in &rules_by_head {
        for rule in rules {
            for atom in &rule.ast.atoms {
                if derived_heads.contains(&atom.relation) {
                    edges.insert((atom.relation.clone(), head.clone()));
                }
            }
        }
    }
    raw.module_ids.sort();
    raw.module_ids.dedup();
    let order = topological_order(&derived_heads, &edges).ok_or_else(|| {
        vec![diagnostic(
            DiagnosticCode::RecursiveDerivation,
            &raw.module_ids.join(","),
            "derived relation dependencies must be nonrecursive",
        )]
    })?;

    let mut sealed = Vec::<Arc<SealedRulePlan>>::new();
    let mut seen_rule_ids = BTreeSet::new();
    for head in order {
        let mut inferred_for_head = None;
        let mut head_rules = rules_by_head.remove(&head).unwrap_or_default();
        head_rules.sort_by_key(|rule| canonical::raw_rule_bytes(&rule.ast));
        for bounded in head_rules {
            let inferred = match infer_rule(&bounded.ast, &schemas) {
                Ok(schema) => schema,
                Err(error) => {
                    diagnostics.push(error);
                    continue;
                }
            };
            if let Some(expected) = &inferred_for_head {
                if expected != &inferred {
                    diagnostics.push(diagnostic(
                        DiagnosticCode::TypeMismatch,
                        &head,
                        "all rules for one derived relation must infer the same schema",
                    ));
                    continue;
                }
            } else {
                inferred_for_head = Some(inferred.clone());
                schemas.insert(head.clone(), inferred.clone());
            }
            let raw_bytes = canonical::raw_rule_bytes(&bounded.ast);
            let raw_digest: [u8; 32] = Sha256::digest(&raw_bytes).into();
            let identity = bounded.ast.explicit_id.clone().unwrap_or_else(|| {
                format!(
                    "{}::rule::{}",
                    bounded.module_id,
                    &hex::encode(raw_digest)[..24]
                )
            });
            if !seen_rule_ids.insert(identity.clone()) {
                diagnostics.push(diagnostic(
                    DiagnosticCode::DuplicateRule,
                    &identity,
                    "rule identities must be globally unique",
                ));
                continue;
            }
            let canonical_bytes = canonical::sealed_rule_bytes(&identity, &bounded.ast, &inferred);
            let dependencies = bounded
                .ast
                .atoms
                .iter()
                .map(|atom| atom.relation.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let quote = StaticResourceQuote {
                atoms: bounded.ast.atoms.len(),
                predicates: bounded.ast.predicates.len(),
                terms: bounded.ast.head.len()
                    + bounded
                        .ast
                        .atoms
                        .iter()
                        .map(|atom| atom.terms.len())
                        .sum::<usize>(),
                canonical_bytes: canonical_bytes.len(),
            };
            let digest = canonical::digest(&canonical_bytes);
            sealed.push(Arc::new(SealedRulePlan::new(
                identity,
                canonical_bytes,
                digest,
                dependencies,
                inferred,
                quote,
            )));
        }
    }
    if let Some(error) = diagnostics.into_iter().min() {
        return Err(vec![error]);
    }
    sealed.sort_by(|left, right| {
        (
            left.inferred_head().identity.as_str(),
            left.identity(),
            left.digest(),
        )
            .cmp(&(
                right.inferred_head().identity.as_str(),
                right.identity(),
                right.digest(),
            ))
    });

    validate_operations(&raw.operations, &schemas)?;
    validate_sealed_limits(&sealed, edges.len(), limits)?;
    let mut schema_values = schemas.into_values().collect::<Vec<_>>();
    schema_values.sort();
    let edge_values = edges.into_iter().collect::<Vec<_>>();
    let mut operations = raw.operations;
    operations.sort();
    let manifest_digest = canonical::manifest_digest(
        &raw.module_ids,
        &schema_values,
        &sealed,
        &edge_values,
        &operations,
    );
    Ok(FrontendArtifacts {
        modules: raw.module_ids.into(),
        relations: RelationManifest::new(schema_values),
        rules: sealed.into(),
        dependency_dag: DerivationDependencyDag::new(edge_values),
        operations: operations.into(),
        manifest_digest: FrontendManifestDigest::new(manifest_digest),
    })
}

fn validate_schema(schema: &RelationSchema, diagnostics: &mut Vec<FrontendDiagnostic>) {
    let mut columns = BTreeSet::new();
    for column in &schema.columns {
        if !columns.insert(column.name.as_str()) {
            diagnostics.push(diagnostic(
                DiagnosticCode::DuplicateColumn,
                &format!("{}::{}", schema.identity, column.name),
                "relation column names must be unique",
            ));
        }
        if column.value_type != RelationType::Entity && column.on_delete.is_some() {
            diagnostics.push(diagnostic(
                DiagnosticCode::TypeMismatch,
                &format!("{}::{}", schema.identity, column.name),
                "on delete metadata is valid only for entity columns",
            ));
        }
    }
    let mut unique_names = BTreeSet::new();
    for unique in &schema.unique {
        if !unique_names.insert(unique.name.as_str()) {
            diagnostics.push(diagnostic(
                DiagnosticCode::DuplicateUniqueConstraint,
                &format!("{}::{}", schema.identity, unique.name),
                "unique constraint names must be unique",
            ));
        }
        let mut members = BTreeSet::new();
        for column in &unique.columns {
            if !columns.contains(column.as_str()) || !members.insert(column.as_str()) {
                diagnostics.push(diagnostic(
                    DiagnosticCode::UnknownUniqueColumn,
                    &format!("{}::{}", schema.identity, column),
                    "unique constraints name distinct declared columns",
                ));
            }
        }
    }
    if schema.symmetric {
        if schema.columns.len() != 2 || schema.columns[0].value_type != schema.columns[1].value_type
        {
            diagnostics.push(diagnostic(
                DiagnosticCode::SymmetricShape,
                &schema.identity,
                "symmetric relations require two columns of one type",
            ));
        }
        if !schema.unique.is_empty() {
            diagnostics.push(diagnostic(
                DiagnosticCode::SymmetricUnique,
                &schema.identity,
                "v0 forbids unique constraints on symmetric relations",
            ));
        }
        if schema.columns.len() == 2 && schema.columns[0].on_delete != schema.columns[1].on_delete {
            diagnostics.push(diagnostic(
                DiagnosticCode::SymmetricEndpointMetadata,
                &schema.identity,
                "symmetric endpoints require identical delete metadata",
            ));
        }
    }
}

fn infer_rule(
    rule: &RawRuleAst,
    schemas: &BTreeMap<String, RelationSchema>,
) -> Result<RelationSchema, FrontendDiagnostic> {
    let mut bindings = BTreeMap::<String, RelationType>::new();
    let mut diagnostics = Vec::new();
    let mut atoms = rule.atoms.iter().collect::<Vec<_>>();
    atoms.sort();
    for atom in atoms {
        let Some(schema) = schemas.get(&atom.relation) else {
            diagnostics.push(diagnostic(
                DiagnosticCode::UnknownRelation,
                &atom.relation,
                "rule atom names an unknown relation",
            ));
            continue;
        };
        if atom.terms.len() != schema.columns.len() {
            diagnostics.push(diagnostic(
                DiagnosticCode::Arity,
                &atom.relation,
                "rule atom arity does not match its schema",
            ));
        }
        for (term, column) in atom.terms.iter().zip(&schema.columns) {
            match term {
                RawTerm::Variable(name) => {
                    if let Err(error) = bind(&mut bindings, name, column.value_type) {
                        diagnostics.push(error);
                    }
                }
                RawTerm::Literal(value) if literal_type(value) != column.value_type => {
                    diagnostics.push(diagnostic(
                        DiagnosticCode::TypeMismatch,
                        &atom.relation,
                        "atom literal type does not match its column",
                    ));
                }
                RawTerm::Literal(_) => {}
            }
        }
    }
    let mut predicates = rule.predicates.iter().collect::<Vec<_>>();
    predicates.sort();
    for predicate in predicates {
        let RawPredicate::Greater(left, right) = predicate;
        if bindings.get(left) != Some(&RelationType::Int)
            || bindings.get(right) != Some(&RelationType::Int)
        {
            diagnostics.push(diagnostic(
                DiagnosticCode::TypeMismatch,
                &rule.head_relation,
                "greater-than operands must be positively bound ints",
            ));
        }
    }
    if let Some(error) = diagnostics.into_iter().min() {
        return Err(error);
    }

    let aggregate_output = if let Some(aggregate) = &rule.aggregate {
        if rule.atoms.is_empty() {
            return Err(diagnostic(
                DiagnosticCode::AggregateRequiresPositiveInput,
                &rule.head_relation,
                "aggregate rules require a positive relation atom",
            ));
        }
        if bindings.contains_key(&aggregate.output)
            || aggregate.input.as_ref() == Some(&aggregate.output)
            || aggregate.group_by.contains(&aggregate.output)
        {
            return Err(diagnostic(
                DiagnosticCode::AggregateOutputNotFresh,
                &rule.head_relation,
                "aggregate output must be fresh",
            ));
        }
        let output_type = match aggregate.kind {
            AggregateKind::Count if aggregate.input.is_none() => RelationType::Count,
            AggregateKind::Sum | AggregateKind::Min | AggregateKind::Max => {
                let input = aggregate.input.as_ref().ok_or_else(|| {
                    diagnostic(
                        DiagnosticCode::AggregateType,
                        &rule.head_relation,
                        "sum/min/max require one int input",
                    )
                })?;
                if bindings.get(input) != Some(&RelationType::Int) {
                    return Err(diagnostic(
                        DiagnosticCode::AggregateType,
                        &rule.head_relation,
                        "sum/min/max require one int input",
                    ));
                }
                RelationType::Int
            }
            AggregateKind::Count => {
                return Err(diagnostic(
                    DiagnosticCode::AggregateType,
                    &rule.head_relation,
                    "count has no value input",
                ));
            }
        };
        for group in &aggregate.group_by {
            if !bindings.contains_key(group) {
                return Err(diagnostic(
                    DiagnosticCode::UnboundVariable,
                    group,
                    "aggregate group variables must be positively bound",
                ));
            }
        }
        Some((aggregate.output.as_str(), output_type))
    } else {
        None
    };

    let mut columns = Vec::new();
    let mut head_variables = BTreeSet::new();
    for (index, term) in rule.head.iter().enumerate() {
        let (name, value_type) = match term {
            RawTerm::Variable(name) => {
                let value_type = aggregate_output
                    .filter(|(output, _)| output == &name.as_str())
                    .map(|(_, value_type)| value_type)
                    .or_else(|| bindings.get(name).copied())
                    .ok_or_else(|| {
                        diagnostic(
                            DiagnosticCode::UnboundVariable,
                            name,
                            "head variables must be positively bound",
                        )
                    })?;
                head_variables.insert(name.clone());
                (name.clone(), value_type)
            }
            RawTerm::Literal(value) => (format!("value_{index}"), literal_type(value)),
        };
        columns.push(RelationColumn {
            name,
            value_type,
            on_delete: (value_type == RelationType::Entity).then_some(OnDelete::Restrict),
        });
    }
    if let Some(aggregate) = &rule.aggregate {
        let expected = aggregate
            .group_by
            .iter()
            .cloned()
            .chain(std::iter::once(aggregate.output.clone()))
            .collect::<BTreeSet<_>>();
        if head_variables != expected {
            return Err(diagnostic(
                DiagnosticCode::AggregateHeadProjection,
                &rule.head_relation,
                "aggregate heads contain exactly group variables plus the output",
            ));
        }
    }
    Ok(RelationSchema {
        identity: rule.head_relation.clone(),
        columns,
        unique: Vec::new(),
        symmetric: false,
    })
}

fn bind(
    bindings: &mut BTreeMap<String, RelationType>,
    name: &str,
    value_type: RelationType,
) -> Result<(), FrontendDiagnostic> {
    if bindings
        .insert(name.to_string(), value_type)
        .is_some_and(|previous| previous != value_type)
    {
        Err(diagnostic(
            DiagnosticCode::TypeMismatch,
            name,
            "one variable cannot have two relation types",
        ))
    } else {
        Ok(())
    }
}

fn literal_type(value: &Literal) -> RelationType {
    match value {
        Literal::Int(_) => RelationType::Int,
        Literal::Count(_) => RelationType::Count,
        Literal::Text(_) => RelationType::Text,
    }
}

fn topological_order(
    heads: &BTreeSet<String>,
    edges: &BTreeSet<(String, String)>,
) -> Option<Vec<String>> {
    let mut remaining = heads.clone();
    let mut output = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|head| {
                !edges
                    .iter()
                    .any(|(source, target)| target == *head && remaining.contains(source))
            })
            .cloned()
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return None;
        }
        for head in ready {
            remaining.remove(&head);
            output.push(head);
        }
    }
    Some(output)
}

fn validate_operations(
    operations: &[RelationOperation],
    schemas: &BTreeMap<String, RelationSchema>,
) -> Result<(), Vec<FrontendDiagnostic>> {
    let mut diagnostics = Vec::new();
    for operation in operations {
        let Some(schema) = schemas.get(&operation.relation) else {
            diagnostics.push(diagnostic(
                DiagnosticCode::UnknownRelation,
                &operation.relation,
                "relation operation names an unknown relation",
            ));
            continue;
        };
        if operation.tuple.len() != schema.columns.len() {
            diagnostics.push(diagnostic(
                DiagnosticCode::Arity,
                &operation.relation,
                "relation operation tuple arity does not match its schema",
            ));
        }
        if let RelationOperationKind::ReplaceBy { constraint, key } = &operation.kind {
            let Some(unique) = schema.unique.iter().find(|item| &item.name == constraint) else {
                diagnostics.push(diagnostic(
                    DiagnosticCode::UnknownUniqueColumn,
                    constraint,
                    "ReplaceBy names one declared unique constraint",
                ));
                continue;
            };
            if key.len() != unique.columns.len() {
                diagnostics.push(diagnostic(
                    DiagnosticCode::Arity,
                    constraint,
                    "ReplaceBy key arity must match its unique constraint",
                ));
            }
            for (value, unique_column) in key.iter().zip(&unique.columns) {
                if let Some(column) = schema
                    .columns
                    .iter()
                    .find(|column| &column.name == unique_column)
                {
                    validate_operation_value(
                        value,
                        column.value_type,
                        &mut diagnostics,
                        constraint,
                    );
                }
            }
        }
        for (value, column) in operation.tuple.iter().zip(&schema.columns) {
            validate_operation_value(
                value,
                column.value_type,
                &mut diagnostics,
                &operation.relation,
            );
        }
    }
    if let Some(error) = diagnostics.into_iter().min() {
        Err(vec![error])
    } else {
        Ok(())
    }
}

fn validate_operation_value(
    value: &RawOperationValue,
    expected: RelationType,
    diagnostics: &mut Vec<FrontendDiagnostic>,
    identity: &str,
) {
    if let RawOperationValue::Literal(value) = value {
        if literal_type(value) != expected {
            diagnostics.push(diagnostic(
                DiagnosticCode::TypeMismatch,
                identity,
                "operation literal type does not match its relation column",
            ));
        }
    }
}

fn validate_sealed_limits(
    rules: &[Arc<SealedRulePlan>],
    edge_count: usize,
    limits: SealedPlanLimits,
) -> Result<(), Vec<FrontendDiagnostic>> {
    let mut diagnostics = Vec::new();
    if rules.len() > limits.max_rules {
        diagnostics.push(FrontendDiagnostic::limit(
            DiagnosticCode::SealedRuleLimit,
            rules.len(),
            limits.max_rules,
        ));
    }
    let mut total_terms = 0usize;
    let mut total_bytes = 0usize;
    for rule in rules {
        let quote = rule.resource_quote();
        total_terms = total_terms.saturating_add(quote.terms);
        total_bytes = total_bytes.saturating_add(quote.canonical_bytes);
        if quote.atoms > limits.max_atoms_per_rule {
            diagnostics.push(FrontendDiagnostic::limit(
                DiagnosticCode::SealedAtomLimit,
                quote.atoms,
                limits.max_atoms_per_rule,
            ));
        }
        if quote.predicates > limits.max_predicates_per_rule {
            diagnostics.push(FrontendDiagnostic::limit(
                DiagnosticCode::SealedPredicateLimit,
                quote.predicates,
                limits.max_predicates_per_rule,
            ));
        }
    }
    if total_terms > limits.max_terms {
        diagnostics.push(FrontendDiagnostic::limit(
            DiagnosticCode::SealedTermLimit,
            total_terms,
            limits.max_terms,
        ));
    }
    if edge_count > limits.max_dependency_edges {
        diagnostics.push(FrontendDiagnostic::limit(
            DiagnosticCode::SealedDependencyLimit,
            edge_count,
            limits.max_dependency_edges,
        ));
    }
    if total_bytes > limits.max_canonical_bytes {
        diagnostics.push(FrontendDiagnostic::limit(
            DiagnosticCode::SealedByteLimit,
            total_bytes,
            limits.max_canonical_bytes,
        ));
    }
    if let Some(error) = diagnostics.into_iter().min() {
        Err(vec![error])
    } else {
        Ok(())
    }
}

fn diagnostic(code: DiagnosticCode, identity: &str, message: &str) -> FrontendDiagnostic {
    FrontendDiagnostic::new(code, message, 0, 0, identity.as_bytes())
}
