use super::encoding;
use super::meter::DerivationMeter;
use super::model::{
    BindingState, DerivationError, DerivationLimits, DerivationResult, DerivedRelationState,
    LogicalRow, ProofAlternative, SupportRef,
};
use crate::relation_frontend::{
    AggregateKind, Literal, RelationKind, RelationSchema, RuleAggregate, RuleAtom, RulePredicate,
    RuleTerm, SealedRulePlan,
};
use crate::relation_runtime::{
    AuthoritativeRelationState, FactKey, FactValue, RelationRuntimeManifest,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn derive_all(
    authoritative: &AuthoritativeRelationState,
    manifest: &RelationRuntimeManifest,
    limits: DerivationLimits,
) -> DerivationResult<DerivedRelationState> {
    validate_installation(authoritative, manifest)?;
    let schemas = manifest
        .schemas()
        .iter()
        .map(|schema| (schema.schema().identity.clone(), schema.schema()))
        .collect::<BTreeMap<_, _>>();
    let mut meter = DerivationMeter::new(limits);
    let mut result = BTreeMap::new();
    for head in canonical_head_order(manifest)? {
        for rule in manifest
            .rules()
            .iter()
            .filter(|rule| rule.typed_plan().head_relation == head)
        {
            evaluate_rule(rule, authoritative, &schemas, &mut result, &mut meter)?;
        }
    }

    Ok(DerivedRelationState::new(result, meter.stats()))
}

pub(super) fn canonical_head_order(
    manifest: &RelationRuntimeManifest,
) -> DerivationResult<Vec<String>> {
    let heads = manifest
        .rules()
        .iter()
        .map(|rule| rule.typed_plan().head_relation.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = heads.clone();
    let mut completed = BTreeSet::new();
    let mut order = Vec::with_capacity(heads.len());

    while !pending.is_empty() {
        let ready = pending.iter().find(|head| {
            manifest
                .rules()
                .iter()
                .filter(|rule| rule.typed_plan().head_relation == **head)
                .flat_map(|rule| rule.dependencies())
                .filter(|dependency| heads.contains(*dependency))
                .all(|dependency| completed.contains(dependency))
        });
        let Some(head) = ready.cloned() else {
            return Err(DerivationError::new(
                "derivation.cycle",
                "sealed dependency graph has no ready head",
            ));
        };
        pending.remove(&head);
        completed.insert(head.clone());
        order.push(head);
    }
    Ok(order)
}

pub(super) fn validate_installation(
    authoritative: &AuthoritativeRelationState,
    expected: &RelationRuntimeManifest,
) -> DerivationResult<()> {
    let installed = authoritative.manifest().ok_or_else(|| {
        DerivationError::new(
            "derivation.manifest_not_installed",
            "authoritative state has no relation manifest",
        )
    })?;
    if installed.digest() != expected.digest() {
        return Err(DerivationError::new(
            "derivation.frontend_manifest_mismatch",
            "authoritative and derivation artifacts come from different programs",
        ));
    }
    Ok(())
}

fn evaluate_rule(
    rule: &SealedRulePlan,
    authoritative: &AuthoritativeRelationState,
    schemas: &BTreeMap<String, &RelationSchema>,
    derived: &mut BTreeMap<FactKey, BTreeSet<ProofAlternative>>,
    meter: &mut DerivationMeter,
) -> DerivationResult<()> {
    meter.intermediate_state(16)?;
    let mut states = BTreeSet::from([BindingState {
        bindings: BTreeMap::new(),
        supports: BTreeSet::new(),
    }]);
    for atom in &rule.typed_plan().atoms {
        let rows = logical_rows(atom, authoritative, schemas, derived, meter)?;
        let mut next = BTreeSet::new();
        for state in &states {
            for row in &rows {
                meter.row()?;
                meter.join()?;
                if let Some(joined) = unify(state, &atom.terms, row, meter)? {
                    if !next.contains(&joined) && next.len() >= meter.limits().max_bindings {
                        return Err(DerivationError::new(
                            "derivation.binding_limit",
                            format!("limit {}", meter.limits().max_bindings),
                        ));
                    }
                    next.insert(joined);
                }
            }
        }
        states = next;
    }
    states.retain(|state| {
        rule.typed_plan()
            .predicates
            .iter()
            .all(|predicate| predicate_matches(predicate, &state.bindings))
    });
    match &rule.typed_plan().aggregate {
        Some(aggregate) => evaluate_aggregate(rule, aggregate, states, schemas, derived, meter),
        None => evaluate_plain(rule, states, schemas, derived, meter),
    }
}

pub(super) fn logical_rows(
    atom: &RuleAtom,
    authoritative: &AuthoritativeRelationState,
    schemas: &BTreeMap<String, &RelationSchema>,
    derived: &BTreeMap<FactKey, BTreeSet<ProofAlternative>>,
    meter: &mut DerivationMeter,
) -> DerivationResult<Vec<LogicalRow>> {
    let schema = schemas
        .get(&atom.relation)
        .ok_or_else(|| DerivationError::new("derivation.invalid_sealed_rule", &atom.relation))?;
    let mut rows = Vec::new();
    match schema.kind {
        RelationKind::Authoritative => {
            for assertion in authoritative
                .assertions()
                .values()
                .filter(|assertion| assertion.fact_key.relation == atom.relation)
            {
                let bytes = encoding::authoritative_row_len(
                    &assertion.fact_key,
                    &assertion.required_capabilities,
                )?;
                meter.intermediate_bytes(bytes)?;
                let support = SupportRef::Authoritative {
                    key: assertion.fact_key.clone(),
                    assertion_id: assertion.assertion_id,
                    required_capabilities: assertion.required_capabilities.clone(),
                };
                rows.push(LogicalRow {
                    tuple: assertion.fact_key.tuple.clone(),
                    support: support.clone(),
                });
                if schema.symmetric && assertion.fact_key.tuple[0] != assertion.fact_key.tuple[1] {
                    meter.intermediate_bytes(bytes)?;
                    let mut tuple = assertion.fact_key.tuple.clone();
                    tuple.swap(0, 1);
                    rows.push(LogicalRow { tuple, support });
                }
            }
        }
        RelationKind::Derived => {
            for (fact, proofs) in derived
                .iter()
                .filter(|(fact, _)| fact.relation == atom.relation)
            {
                for proof in proofs {
                    let bytes = encoding::derived_row_len(fact, proof)?;
                    meter.intermediate_bytes(bytes)?;
                    let support = SupportRef::Derived {
                        key: fact.clone(),
                        proof_id: proof.identity(),
                        required_capabilities: proof.required_capabilities.clone(),
                        proof_depth: proof.depth,
                    };
                    rows.push(LogicalRow {
                        tuple: fact.tuple.clone(),
                        support: support.clone(),
                    });
                    if schema.symmetric && fact.tuple[0] != fact.tuple[1] {
                        meter.intermediate_bytes(bytes)?;
                        let mut tuple = fact.tuple.clone();
                        tuple.swap(0, 1);
                        rows.push(LogicalRow { tuple, support });
                    }
                }
            }
        }
    }
    rows.sort();
    Ok(rows)
}

pub(super) fn unify(
    state: &BindingState,
    terms: &[RuleTerm],
    row: &LogicalRow,
    meter: &mut DerivationMeter,
) -> DerivationResult<Option<BindingState>> {
    if terms.len() != row.tuple.len() {
        return Ok(None);
    }
    for (index, (term, value)) in terms.iter().zip(&row.tuple).enumerate() {
        match term {
            RuleTerm::Literal(expected) if literal_value(expected) != *value => return Ok(None),
            RuleTerm::Literal(_) => {}
            RuleTerm::Variable(name) => match state.bindings.get(name) {
                Some(existing) if existing != value => return Ok(None),
                Some(_) => {}
                None => {
                    for (prior, prior_value) in terms[..index].iter().zip(&row.tuple[..index]) {
                        if matches!(prior, RuleTerm::Variable(prior_name) if prior_name == name)
                            && prior_value != value
                        {
                            return Ok(None);
                        }
                    }
                }
            },
        }
    }

    let mut bytes = encoding::binding_state_len(state)?;
    for (index, (term, value)) in terms.iter().zip(&row.tuple).enumerate() {
        let RuleTerm::Variable(name) = term else {
            continue;
        };
        let first = !state.bindings.contains_key(name)
            && !terms[..index]
                .iter()
                .any(|prior| matches!(prior, RuleTerm::Variable(prior) if prior == name));
        if first {
            bytes = encoding::checked_add(bytes, encoding::text_len(name)?)?;
            bytes = encoding::checked_add(bytes, encoding::value_len(value)?)?;
        }
    }
    if !state.supports.contains(&row.support) {
        bytes = encoding::checked_add(bytes, encoding::support_len(&row.support)?)?;
    }
    meter.intermediate_state(bytes)?;
    let mut next = state.clone();
    for (term, value) in terms.iter().zip(&row.tuple) {
        if let RuleTerm::Variable(name) = term {
            next.bindings
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
    }
    next.supports.insert(row.support.clone());
    Ok(Some(next))
}

pub(super) fn predicate_matches(
    predicate: &RulePredicate,
    bindings: &BTreeMap<String, FactValue>,
) -> bool {
    match predicate {
        RulePredicate::Greater(left, right) => {
            matches!(
                (bindings.get(left), bindings.get(right)),
                (Some(FactValue::Int(left)), Some(FactValue::Int(right))) if left > right
            )
        }
    }
}

pub(super) fn evaluate_plain(
    rule: &SealedRulePlan,
    states: BTreeSet<BindingState>,
    schemas: &BTreeMap<String, &RelationSchema>,
    result: &mut BTreeMap<FactKey, BTreeSet<ProofAlternative>>,
    meter: &mut DerivationMeter,
) -> DerivationResult<()> {
    for state in states {
        let key = build_head(rule, &state.bindings, schemas)?;
        meter.intermediate_bytes(encoding::fact_len(&key)?)?;
        meter.intermediate_bytes(encoding::combined_capabilities_len(&state.supports)?)?;
        let required_capabilities = combined_capabilities(&state.supports);
        let depth = 1 + state
            .supports
            .iter()
            .map(SupportRef::depth)
            .max()
            .unwrap_or(0);
        meter.retain(
            result,
            key,
            ProofAlternative {
                rule: rule.identity().to_string(),
                bindings: state.bindings,
                supports: state.supports,
                aggregate_group: None,
                required_capabilities,
                depth,
            },
        )?;
    }
    Ok(())
}

pub(super) fn evaluate_aggregate(
    rule: &SealedRulePlan,
    aggregate: &RuleAggregate,
    states: BTreeSet<BindingState>,
    schemas: &BTreeMap<String, &RelationSchema>,
    result: &mut BTreeMap<FactKey, BTreeSet<ProofAlternative>>,
    meter: &mut DerivationMeter,
) -> DerivationResult<()> {
    let group_names = &aggregate.group_by;
    let mut groups =
        BTreeMap::<Vec<FactValue>, BTreeMap<BTreeMap<String, FactValue>, Vec<BindingState>>>::new();
    for state in states {
        let mut group_bytes = 8;
        let mut group = Vec::with_capacity(group_names.len());
        for name in group_names {
            let value = state.bindings.get(name).ok_or_else(|| {
                DerivationError::new("derivation.invalid_sealed_rule", "unbound aggregate group")
            })?;
            group_bytes = encoding::checked_add(group_bytes, encoding::value_len(value)?)?;
            group.push(value.clone());
        }
        meter.aggregate_entry(encoding::checked_add(
            group_bytes,
            encoding::binding_state_len(&state)?,
        )?)?;
        groups
            .entry(group)
            .or_default()
            .entry(state.bindings.clone())
            .or_default()
            .push(state);
    }

    for (group, logical_bindings) in groups {
        let value = aggregate_value(aggregate, &logical_bindings, meter)?;
        let mut bindings = group_names
            .iter()
            .cloned()
            .zip(group.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        bindings.insert(aggregate.output.clone(), value);
        let key = build_head(rule, &bindings, schemas)?;
        meter.intermediate_bytes(encoding::binding_map_len(&bindings)?)?;
        meter.intermediate_bytes(encoding::fact_len(&key)?)?;

        let mut combinations = BTreeSet::from([BTreeSet::new()]);
        for alternatives in logical_bindings.values() {
            let mut next = BTreeSet::new();
            let mut capability_sets = BTreeSet::new();
            for accumulated in &combinations {
                for alternative in alternatives {
                    let bytes = encoding::checked_add(
                        encoding::support_set_len(accumulated)?,
                        encoding::support_set_len(&alternative.supports)?,
                    )?;
                    meter.proof_combination(bytes)?;
                    let supports = accumulated
                        .union(&alternative.supports)
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    if !next.contains(&supports) {
                        if next.len() >= meter.limits().max_proofs_per_fact {
                            return Err(DerivationError::new(
                                "derivation.proofs_per_fact_limit",
                                format!("limit {}", meter.limits().max_proofs_per_fact),
                            ));
                        }
                        capability_sets.insert(combined_capabilities(&supports));
                        if capability_sets.len() > meter.limits().max_capability_alternatives {
                            return Err(DerivationError::new(
                                "derivation.capability_alternative_limit",
                                format!("limit {}", meter.limits().max_capability_alternatives),
                            ));
                        }
                    }
                    next.insert(supports);
                }
            }
            combinations = next;
        }
        for supports in combinations {
            meter.intermediate_bytes(encoding::combined_capabilities_len(&supports)?)?;
            let required_capabilities = combined_capabilities(&supports);
            let depth = 1 + supports.iter().map(SupportRef::depth).max().unwrap_or(0);
            meter.retain(
                result,
                key.clone(),
                ProofAlternative {
                    rule: rule.identity().to_string(),
                    bindings: bindings.clone(),
                    supports,
                    aggregate_group: Some(group.clone()),
                    required_capabilities,
                    depth,
                },
            )?;
        }
    }
    Ok(())
}

fn aggregate_value(
    aggregate: &RuleAggregate,
    logical_bindings: &BTreeMap<BTreeMap<String, FactValue>, Vec<BindingState>>,
    meter: &mut DerivationMeter,
) -> DerivationResult<FactValue> {
    let values = match &aggregate.input {
        Some(input) => logical_bindings
            .keys()
            .map(|bindings| {
                bindings.get(input).cloned().ok_or_else(|| {
                    DerivationError::new("derivation.invalid_sealed_rule", "unbound aggregate")
                })
            })
            .collect::<DerivationResult<Vec<_>>>()?,
        None => vec![FactValue::Count(1); logical_bindings.len()],
    };
    let bytes = values.iter().try_fold(8, |length, value| {
        encoding::checked_add(length, encoding::value_len(value)?)
    })?;
    meter.intermediate_bytes(bytes)?;
    match aggregate.kind {
        AggregateKind::Count => Ok(FactValue::Count(u64::try_from(values.len()).map_err(
            |_| DerivationError::new("derivation.count_overflow", "logical binding count"),
        )?)),
        AggregateKind::Sum => {
            let mut sum = 0_i64;
            for value in values {
                let FactValue::Int(value) = value else {
                    return Err(DerivationError::new(
                        "derivation.aggregate_type",
                        "sum input is not int",
                    ));
                };
                sum = sum.checked_add(value).ok_or_else(|| {
                    DerivationError::new("derivation.sum_overflow", "i64 sum overflow")
                })?;
            }
            Ok(FactValue::Int(sum))
        }
        AggregateKind::Min | AggregateKind::Max => {
            let values = values
                .into_iter()
                .map(|value| match value {
                    FactValue::Int(value) => Ok(value),
                    _ => Err(DerivationError::new(
                        "derivation.aggregate_type",
                        "min/max input is not int",
                    )),
                })
                .collect::<DerivationResult<Vec<_>>>()?;
            let value = if aggregate.kind == AggregateKind::Min {
                values.into_iter().min()
            } else {
                values.into_iter().max()
            };
            value.map(FactValue::Int).ok_or_else(|| {
                DerivationError::new("derivation.empty_aggregate", "empty aggregate group")
            })
        }
    }
}

fn build_head(
    rule: &SealedRulePlan,
    bindings: &BTreeMap<String, FactValue>,
    schemas: &BTreeMap<String, &RelationSchema>,
) -> DerivationResult<FactKey> {
    let tuple = rule
        .typed_plan()
        .head
        .iter()
        .map(|term| match term {
            RuleTerm::Literal(value) => Ok(literal_value(value)),
            RuleTerm::Variable(name) => bindings.get(name).cloned().ok_or_else(|| {
                DerivationError::new("derivation.invalid_sealed_rule", "unbound head variable")
            }),
        })
        .collect::<DerivationResult<Vec<_>>>()?;
    let schema = schemas
        .get(&rule.typed_plan().head_relation)
        .copied()
        .ok_or_else(|| {
            DerivationError::new("derivation.invalid_sealed_rule", "unknown head schema")
        })?;
    crate::relation_runtime::canonical_fact_key(schema, FactKey::new(&schema.identity, tuple))
        .map_err(|error| {
            DerivationError::new(
                "derivation.invalid_sealed_rule",
                format!("head does not match its inferred schema: {}", error.detail),
            )
        })
}

pub(super) fn literal_value(value: &Literal) -> FactValue {
    match value {
        Literal::Int(value) => FactValue::Int(*value),
        Literal::Count(value) => FactValue::Count(*value),
        Literal::Text(value) => FactValue::Text(value.clone()),
    }
}

fn combined_capabilities(supports: &BTreeSet<SupportRef>) -> BTreeSet<String> {
    supports
        .iter()
        .flat_map(|support| support.required_capabilities().iter().cloned())
        .collect()
}
