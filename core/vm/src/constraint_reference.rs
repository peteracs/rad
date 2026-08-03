//! Deliberately small, VM-independent RFC-0002 settlement oracle.
//!
//! It models trigger selection, deduplication, isolated fuel, outcome
//! collection, canonical ordering, and atomic accept/reject. It has no
//! bytecode, GC, modules, scheduler, or mutable world and remains the
//! differential oracle after the production runtime lands.

use crate::constraint_types::{
    CandidateKey, CandidateView, ConstraintEvaluationFailure, ConstraintIdentity,
    ConstraintLimitProfile, ConstraintOutcome, EphemeralCausalExplanation,
    RejectionCapabilityMetadata, SettlementRejection,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceConstraint {
    pub identity: ConstraintIdentity,
    pub watches: BTreeSet<String>,
    pub fuel_cost: u64,
    pub outcomes: BTreeMap<u32, ConstraintOutcome>,
}

pub fn select_reference_invocations(
    view: &CandidateView,
    constraints: &[ReferenceConstraint],
) -> Vec<(ConstraintIdentity, u32)> {
    let mut selected = BTreeSet::new();
    for staged in view.staged_keys() {
        for constraint in constraints {
            let triggered = staged.component == constraint.identity.attached_component
                || constraint.watches.contains(&staged.component);
            if !triggered {
                continue;
            }
            let attached = CandidateKey {
                entity: staged.entity,
                component: constraint.identity.attached_component.clone(),
            };
            if view.candidate(&attached).is_some() {
                selected.insert((constraint.identity.clone(), staged.entity));
            }
        }
    }
    selected.into_iter().collect()
}

#[allow(clippy::too_many_arguments)]
pub fn settle_constraints_reference(
    settlement_id: u64,
    base_world_digest: impl Into<String>,
    view: &CandidateView,
    constraints: &[ReferenceConstraint],
    profile: &ConstraintLimitProfile,
    capabilities: RejectionCapabilityMetadata,
    explanation: EphemeralCausalExplanation,
) -> Result<(), Box<SettlementRejection>> {
    let selected = select_reference_invocations(view, constraints);
    let by_identity = constraints
        .iter()
        .map(|constraint| (constraint.identity.clone(), constraint))
        .collect::<BTreeMap<_, _>>();
    let mut violations = Vec::new();
    let mut evaluation_failures = Vec::new();

    for (identity, subject) in &selected {
        let constraint = by_identity
            .get(identity)
            .expect("selected reference constraint must exist");
        if constraint.fuel_cost > profile.fuel_per_invocation() {
            evaluation_failures.push(ConstraintEvaluationFailure {
                constraint: identity.clone(),
                subject: *subject,
                code: "constraint.fuel_exhausted".into(),
                message: format!(
                    "constraint consumed {} fuel with a limit of {}",
                    constraint.fuel_cost,
                    profile.fuel_per_invocation()
                ),
                source_line: 0,
            });
            continue;
        }

        match constraint
            .outcomes
            .get(subject)
            .cloned()
            .unwrap_or(ConstraintOutcome::Valid)
        {
            ConstraintOutcome::Valid => {}
            ConstraintOutcome::Violations(mut invocation) => {
                if invocation.len() > profile.max_violations_per_invocation() {
                    evaluation_failures.push(ConstraintEvaluationFailure {
                        constraint: identity.clone(),
                        subject: *subject,
                        code: "constraint.invocation_violation_limit".into(),
                        message: format!(
                            "constraint produced {} violations with a limit of {}",
                            invocation.len(),
                            profile.max_violations_per_invocation()
                        ),
                        source_line: 0,
                    });
                } else {
                    violations.append(&mut invocation);
                }
            }
            ConstraintOutcome::EvaluationFailure(failure) => evaluation_failures.push(failure),
        }
    }

    if violations.len() > profile.max_violations_per_settlement() {
        violations.clear();
        evaluation_failures.push(ConstraintEvaluationFailure {
            constraint: ConstraintIdentity {
                qualified_name: "<settlement>".into(),
                attached_component: "<all>".into(),
            },
            subject: 0,
            code: "constraint.settlement_violation_limit".into(),
            message: format!(
                "settlement violation count exceeded {}",
                profile.max_violations_per_settlement()
            ),
            source_line: 0,
        });
    }

    if violations.is_empty() && evaluation_failures.is_empty() {
        return Ok(());
    }

    let mut rejection = SettlementRejection {
        settlement_id,
        base_world_digest: base_world_digest.into(),
        applicable_constraints: selected
            .iter()
            .map(|(identity, _)| identity.clone())
            .collect(),
        violations,
        evaluation_failures,
        explanation,
        limit_profile_fingerprint: profile.fingerprint(),
        capabilities,
    };
    rejection.canonicalize();
    Err(Box::new(rejection))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint_types::{ConstraintViolation, RejectionValue, RuntimeError};
    use crate::host_value::FrozenValue;
    use crate::CausalValueLimits;

    fn identity(name: &str, attached: &str) -> ConstraintIdentity {
        ConstraintIdentity {
            qualified_name: name.into(),
            attached_component: attached.into(),
        }
    }

    fn key(component: &str) -> CandidateKey {
        CandidateKey {
            entity: 7,
            component: component.into(),
        }
    }

    fn violation(name: &str, code: &str, occurrence: u32) -> ConstraintViolation {
        ConstraintViolation {
            constraint: identity(name, "Position"),
            subject: 7,
            code: code.into(),
            occurrence,
            source_line: 10,
            details: BTreeMap::from([("candidate".into(), RejectionValue::Redacted)]),
        }
    }

    fn capabilities() -> RejectionCapabilityMetadata {
        RejectionCapabilityMetadata {
            profile_id: "test".into(),
            readable_components: BTreeSet::new(),
            origins_visible: false,
        }
    }

    #[test]
    fn watches_trigger_once_and_candidate_reads_complete_patch_with_base_fallback() {
        let view = CandidateView::new(
            BTreeMap::from([
                (key("Position"), FrozenValue::Int(1)),
                (key("Velocity"), FrozenValue::Int(2)),
            ]),
            BTreeMap::from([
                (key("Position"), FrozenValue::Int(3)),
                (key("Velocity"), FrozenValue::Int(4)),
                (key("Irrelevant"), FrozenValue::Int(9)),
            ]),
        );
        let constraint = ReferenceConstraint {
            identity: identity("ValidMotion", "Position"),
            watches: BTreeSet::from(["Velocity".into()]),
            fuel_cost: 1,
            outcomes: BTreeMap::new(),
        };
        assert_eq!(view.base(&key("Position")), Some(&FrozenValue::Int(1)));
        assert_eq!(view.candidate(&key("Position")), Some(&FrozenValue::Int(3)));
        assert_eq!(view.candidate(&key("Velocity")), Some(&FrozenValue::Int(4)));
        assert_eq!(
            select_reference_invocations(&view, &[constraint]),
            vec![(identity("ValidMotion", "Position"), 7)]
        );
    }

    #[test]
    fn every_outcome_is_collected_and_canonical_across_constraint_permutations() {
        let view = CandidateView::new(
            BTreeMap::from([(key("Position"), FrozenValue::Int(1))]),
            BTreeMap::from([(key("Position"), FrozenValue::Int(3))]),
        );
        let bounds = ReferenceConstraint {
            identity: identity("WorldBounds", "Position"),
            watches: BTreeSet::new(),
            fuel_cost: 10,
            outcomes: BTreeMap::from([(
                7,
                ConstraintOutcome::Violations(vec![
                    violation("WorldBounds", "position.max", 1),
                    violation("WorldBounds", "position.max", 2),
                ]),
            )]),
        };
        let penetration = ReferenceConstraint {
            identity: identity("NonPenetration", "Position"),
            watches: BTreeSet::new(),
            fuel_cost: 10,
            outcomes: BTreeMap::from([(
                7,
                ConstraintOutcome::Violations(vec![violation(
                    "NonPenetration",
                    "position.solid",
                    1,
                )]),
            )]),
        };
        let evaluate = |constraints: &[ReferenceConstraint]| {
            settle_constraints_reference(
                1,
                "base",
                &view,
                constraints,
                &ConstraintLimitProfile::default(),
                capabilities(),
                EphemeralCausalExplanation::default(),
            )
            .unwrap_err()
        };
        let first = evaluate(&[bounds.clone(), penetration.clone()]);
        let second = evaluate(&[penetration, bounds]);
        assert_eq!(first, second);
        assert_eq!(first.violations.len(), 3, "duplicates are preserved");
    }

    #[test]
    fn isolated_fuel_and_output_caps_become_canonical_failures() {
        let view = CandidateView::new(
            BTreeMap::from([(key("Position"), FrozenValue::Int(1))]),
            BTreeMap::from([(key("Position"), FrozenValue::Int(3))]),
        );
        let profile =
            ConstraintLimitProfile::try_new(CausalValueLimits::default(), 5, 1024, 1, 2, 1_048_576)
                .expect("test limits");
        let hungry = ReferenceConstraint {
            identity: identity("Hungry", "Position"),
            watches: BTreeSet::new(),
            fuel_cost: 6,
            outcomes: BTreeMap::new(),
        };
        let noisy = ReferenceConstraint {
            identity: identity("Noisy", "Position"),
            watches: BTreeSet::new(),
            fuel_cost: 1,
            outcomes: BTreeMap::from([(
                7,
                ConstraintOutcome::Violations(vec![
                    violation("Noisy", "one", 1),
                    violation("Noisy", "two", 2),
                ]),
            )]),
        };
        let rejection = settle_constraints_reference(
            1,
            "base",
            &view,
            &[noisy, hungry],
            &profile,
            capabilities(),
            EphemeralCausalExplanation::default(),
        )
        .unwrap_err();
        assert_eq!(rejection.evaluation_failures.len(), 2);
        assert!(rejection
            .evaluation_failures
            .iter()
            .any(|failure| failure.code == "constraint.fuel_exhausted"));
        assert!(rejection
            .evaluation_failures
            .iter()
            .any(|failure| failure.code == "constraint.invocation_violation_limit"));
        let _typed_runtime_boundary = crate::constraint_types::VmFailure::Runtime(RuntimeError {
            code: "test".into(),
            message: "separate from rejection".into(),
        });
    }
}
