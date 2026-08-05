//! Independent indexed evaluation and affected-head maintenance.
//!
//! Full recomputation remains semantic truth. This path builds canonical
//! per-column row indexes, computes its own bindings/proofs, and uses the
//! reference meter's logical scan quote so both paths choose the same typed
//! resource failure even when the indexed path skips physical comparisons.

use super::evaluator::{
    canonical_head_order, evaluate_aggregate, evaluate_plain, literal_value, logical_rows,
    predicate_matches, unify, validate_installation,
};
use super::meter::DerivationMeter;
use super::model::{
    BindingState, DerivationLimits, DerivationResult, DerivedRelationState, LogicalRow,
    ProofAlternative,
};
use crate::relation_frontend::{RelationSchema, RuleAtom, RuleTerm, SealedRulePlan};
use crate::relation_runtime::RelationRuntimeManifest;
use crate::relation_runtime::{AuthoritativeRelationState, FactChange, FactKey, FactValue};
use std::collections::{BTreeMap, BTreeSet};

pub fn derive_indexed_all(
    authoritative: &AuthoritativeRelationState,
    manifest: &RelationRuntimeManifest,
    limits: DerivationLimits,
) -> DerivationResult<DerivedRelationState> {
    validate_installation(authoritative, manifest)?;
    let schemas = schemas(manifest);
    let order = canonical_head_order(manifest)?;
    derive_indexed_in_order(authoritative, manifest, &schemas, &order, limits)
}

fn derive_indexed_in_order(
    authoritative: &AuthoritativeRelationState,
    manifest: &RelationRuntimeManifest,
    schemas: &BTreeMap<String, &RelationSchema>,
    order: &[String],
    limits: DerivationLimits,
) -> DerivationResult<DerivedRelationState> {
    let mut meter = DerivationMeter::new(limits);
    let mut result = BTreeMap::new();
    for head in order {
        evaluate_head(
            head,
            manifest,
            authoritative,
            schemas,
            &mut result,
            &mut meter,
        )?;
    }
    Ok(DerivedRelationState::new(result, meter.stats()))
}

/// Recompute only heads reachable from the authoritative delta. A separate
/// indexed full preflight supplies the RFC-required canonical limit result;
/// the maintained answer itself is built from the prior state plus affected
/// heads and never copied from that preflight result.
pub fn maintain_indexed(
    previous: &DerivedRelationState,
    authoritative: &AuthoritativeRelationState,
    manifest: &RelationRuntimeManifest,
    changes: &[FactChange],
    limits: DerivationLimits,
) -> DerivationResult<DerivedRelationState> {
    validate_installation(authoritative, manifest)?;
    if changes.is_empty() {
        return Ok(previous.clone());
    }
    let schemas = schemas(manifest);
    let order = canonical_head_order(manifest)?;
    let full_quote = derive_indexed_in_order(authoritative, manifest, &schemas, &order, limits)?;
    let affected = affected_heads(manifest, changes, &order);
    let mut meter = DerivationMeter::new(limits);
    let mut result = BTreeMap::new();
    for head in order {
        if affected.contains(&head) {
            evaluate_head(
                &head,
                manifest,
                authoritative,
                &schemas,
                &mut result,
                &mut meter,
            )?;
        } else {
            for (fact, proofs) in previous
                .facts()
                .iter()
                .filter(|(fact, _)| fact.relation == head)
            {
                for proof in proofs {
                    meter.retain(&mut result, fact.clone(), proof.clone())?;
                }
            }
        }
    }
    Ok(DerivedRelationState::new(result, full_quote.stats()))
}

fn schemas(manifest: &RelationRuntimeManifest) -> BTreeMap<String, &RelationSchema> {
    manifest
        .schemas()
        .iter()
        .map(|schema| (schema.schema().identity.clone(), schema.schema()))
        .collect()
}

fn affected_heads(
    manifest: &RelationRuntimeManifest,
    changes: &[FactChange],
    order: &[String],
) -> BTreeSet<String> {
    let mut affected = changes
        .iter()
        .map(|change| change.fact_key.relation.clone())
        .collect::<BTreeSet<_>>();
    for head in order {
        if manifest
            .rules()
            .iter()
            .filter(|rule| rule.typed_plan().head_relation == *head)
            .any(|rule| {
                rule.dependencies()
                    .iter()
                    .any(|input| affected.contains(input))
            })
        {
            affected.insert(head.clone());
        }
    }
    affected
}

fn evaluate_head(
    head: &str,
    manifest: &RelationRuntimeManifest,
    authoritative: &AuthoritativeRelationState,
    schemas: &BTreeMap<String, &RelationSchema>,
    result: &mut BTreeMap<FactKey, BTreeSet<ProofAlternative>>,
    meter: &mut DerivationMeter,
) -> DerivationResult<()> {
    for rule in manifest
        .rules()
        .iter()
        .filter(|rule| rule.typed_plan().head_relation == head)
    {
        evaluate_rule(rule, authoritative, schemas, result, meter)?;
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
        let rows = IndexedRows::new(logical_rows(atom, authoritative, schemas, derived, meter)?);
        let mut next = BTreeSet::new();
        for state in &states {
            match rows.candidates(state, atom) {
                CandidateRows::All => {
                    for row in &rows.rows {
                        meter.scan_attempts(1)?;
                        join_candidate(state, atom, row, &mut next, meter)?;
                    }
                }
                CandidateRows::Indexed(indices) => {
                    let mut cursor = 0;
                    for index in indices {
                        meter.scan_attempts(index.saturating_sub(cursor))?;
                        meter.scan_attempts(1)?;
                        join_candidate(state, atom, &rows.rows[*index], &mut next, meter)?;
                        cursor = *index + 1;
                    }
                    meter.scan_attempts(rows.rows.len().saturating_sub(cursor))?;
                }
                CandidateRows::Empty => meter.scan_attempts(rows.rows.len())?,
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

fn join_candidate(
    state: &BindingState,
    atom: &RuleAtom,
    row: &LogicalRow,
    next: &mut BTreeSet<BindingState>,
    meter: &mut DerivationMeter,
) -> DerivationResult<()> {
    meter.physical_join()?;
    if let Some(joined) = unify(state, &atom.terms, row, meter)? {
        if !next.contains(&joined) && next.len() >= meter.limits().max_bindings {
            return Err(super::DerivationError::new(
                "derivation.binding_limit",
                format!("limit {}", meter.limits().max_bindings),
            ));
        }
        next.insert(joined);
    }
    Ok(())
}

struct IndexedRows {
    rows: Vec<LogicalRow>,
    columns: Vec<BTreeMap<FactValue, Vec<usize>>>,
}

impl IndexedRows {
    fn new(rows: Vec<LogicalRow>) -> Self {
        let width = rows.first().map_or(0, |row| row.tuple.len());
        let mut columns = vec![BTreeMap::<FactValue, Vec<usize>>::new(); width];
        for (index, row) in rows.iter().enumerate() {
            for (column, value) in row.tuple.iter().enumerate() {
                columns[column]
                    .entry(value.clone())
                    .or_default()
                    .push(index);
            }
        }
        Self { rows, columns }
    }

    fn candidates<'a>(&'a self, state: &BindingState, atom: &RuleAtom) -> CandidateRows<'a> {
        if self.rows.is_empty() {
            return CandidateRows::Empty;
        }
        let mut best: Option<&[usize]> = None;
        for (column, term) in atom.terms.iter().enumerate() {
            let value = match term {
                RuleTerm::Literal(literal) => Some(literal_value(literal)),
                RuleTerm::Variable(name) => state.bindings.get(name).cloned(),
            };
            let Some(value) = value else {
                continue;
            };
            let Some(indices) = self.columns[column].get(&value) else {
                return CandidateRows::Empty;
            };
            if best.is_none_or(|current| indices.len() < current.len()) {
                best = Some(indices);
            }
        }
        best.map_or(CandidateRows::All, CandidateRows::Indexed)
    }
}

enum CandidateRows<'a> {
    All,
    Indexed(&'a [usize]),
    Empty,
}
