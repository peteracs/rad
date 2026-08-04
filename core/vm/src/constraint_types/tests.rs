

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_fingerprint_is_deterministic_and_limits_are_validated() {
        let first = ConstraintLimitProfile::default();
        let second = ConstraintLimitProfile::default();
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.fingerprint().len(), 64);
        assert!(matches!(
            ConstraintLimitProfile::try_new(
                CausalValueLimits::default(),
                0,
                1024,
                256,
                4_096,
                1_048_576,
            ),
            Err(ConstraintProfileError::InvalidLimit {
                field: "fuel_per_invocation",
                ..
            })
        ));
    }

    #[test]
    fn candidate_view_reads_base_and_complete_patch_without_mutation() {
        let position = CandidateKey {
            entity: 7,
            component: "Position".into(),
        };
        let velocity = CandidateKey {
            entity: 7,
            component: "Velocity".into(),
        };
        let view = CandidateView::new(
            BTreeMap::from([
                (position.clone(), FrozenValue::Int(1)),
                (velocity.clone(), FrozenValue::Int(2)),
            ]),
            BTreeMap::from([(position.clone(), FrozenValue::Int(3))]),
        );
        assert_eq!(view.base(&position), Some(&FrozenValue::Int(1)));
        assert_eq!(view.candidate(&position), Some(&FrozenValue::Int(3)));
        assert_eq!(view.candidate(&velocity), Some(&FrozenValue::Int(2)));
    }

    #[test]
    fn canonical_rejection_encoding_is_fallible_under_a_narrow_output_profile() {
        let key = CandidateKey {
            entity: 7,
            component: "Data".into(),
        };
        let profile =
            ConstraintLimitProfile::try_new(CausalValueLimits::default(), 100, 1024, 16, 32, 1024)
                .unwrap();
        let rejection = SettlementRejection {
            settlement_id: 99,
            base_world_digest: "base".into(),
            applicable_constraints: vec![ConstraintIdentity {
                qualified_name: "Bounds".into(),
                attached_component: "Data".into(),
            }],
            violations: vec![ConstraintViolation {
                constraint: ConstraintIdentity {
                    qualified_name: "Bounds".into(),
                    attached_component: "Data".into(),
                },
                subject: 7,
                code: "data.too_large".into(),
                occurrence: 1,
                source_line: 1,
                candidate: key.clone(),
            }],
            evaluation_failures: Vec::new(),
            candidate_details: BTreeMap::from([(
                key,
                RejectionValue::Visible(FrozenValue::String("x".repeat(2048))),
            )]),
            explanation: EphemeralCausalExplanation::default(),
            limit_profile_fingerprint: profile.fingerprint(),
            capabilities: RejectionCapabilityMetadata {
                profile_id: "test".into(),
                readable_components: BTreeSet::from(["*".into()]),
                origins_visible: true,
            },
        };
        assert!(matches!(
            rejection.canonical_bytes(&profile),
            Err(RejectionEncodingError::Value(
                CausalValueError::EncodedByteLimit { .. }
            )) | Err(RejectionEncodingError::OutcomeByteLimit { .. })
        ));
    }

    #[test]
    fn opaque_settlement_ids_do_not_change_semantic_rejection_bytes() {
        let profile = ConstraintLimitProfile::default();
        let mut first = SettlementRejection {
            settlement_id: 1,
            base_world_digest: "base".into(),
            applicable_constraints: Vec::new(),
            violations: Vec::new(),
            evaluation_failures: vec![ConstraintEvaluationFailure {
                constraint: ConstraintIdentity {
                    qualified_name: "Bounds".into(),
                    attached_component: "Position".into(),
                },
                subject: 7,
                code: "constraint.evaluation_failed".into(),
                message: "failed".into(),
                source_line: 12,
            }],
            candidate_details: BTreeMap::new(),
            explanation: EphemeralCausalExplanation::default(),
            limit_profile_fingerprint: profile.fingerprint(),
            capabilities: RejectionCapabilityMetadata {
                profile_id: "trusted".into(),
                readable_components: BTreeSet::from(["*".into()]),
                origins_visible: true,
            },
        };
        let mut second = first.clone();
        second.settlement_id = 999;
        first.canonicalize();
        second.canonicalize();
        assert_eq!(
            first.canonical_bytes(&profile).unwrap(),
            second.canonical_bytes(&profile).unwrap()
        );
    }
}
