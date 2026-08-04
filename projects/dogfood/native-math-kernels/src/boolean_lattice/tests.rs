#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closure_of_singletons_is_the_power_set() {
        let family = or_closure(&[1, 2, 4, 8]).unwrap();
        assert_eq!(family, (0..16).collect::<Vec<_>>());
        assert!(is_or_closed(&family).unwrap());
        assert_eq!(bit_frequencies(&family, 4).unwrap(), vec![8, 8, 8, 8]);
        let stats = or_closure_stats(&[1, 2, 4, 8], 4).unwrap();
        assert_eq!((stats.0, stats.1, stats.2), (16, vec![8; 4], true));
        assert_eq!(stats.3, or_closure_stats(&[8, 4, 2, 1], 4).unwrap().3);
    }

    #[test]
    fn cyclic_bitmask_orbits_partition_the_cube() {
        let representatives = bitmask_rotation_representatives(13).unwrap();
        assert_eq!(representatives.len(), 632);
        let mut members = representatives
            .iter()
            .flat_map(|representative| bitmask_rotation_orbit(*representative, 13).unwrap())
            .collect::<Vec<_>>();
        members.sort_unstable();
        assert_eq!(members, (0..8192).collect::<Vec<_>>());
    }

    #[test]
    fn closure_deduplicates_generators_and_uses_sparse_masks() {
        let family = or_closure(&[1 << 40, 3, 3]).unwrap();
        assert_eq!(family, vec![0, 3, 1 << 40, (1 << 40) | 3]);
        assert!(is_or_closed(&family).unwrap());
    }

    #[test]
    fn audit_rejects_duplicates_and_missing_joins() {
        assert!(!is_or_closed(&[0, 1, 1]).unwrap());
        assert!(!is_or_closed(&[0, 1, 2]).unwrap());
        assert_eq!(or_violation_count(&[0, 1, 2], 2).unwrap(), 2);
        assert_eq!(or_violation_count(&[0, 1, 2, 3], 2).unwrap(), 0);
        assert!(or_violation_count(&[0, 1, 1], 2).is_err());
        assert_eq!(
            or_deletable_members(&[0, 1, 2, 3], 2).unwrap(),
            vec![0, 1, 2]
        );
        assert_eq!(or_deletable_members(&[0, 2, 3], 2).unwrap(), vec![0, 2, 3]);
        assert!(or_deletable_members(&[0, 1, 2], 2).is_err());
        let initial = or_deletion_state(&[], 2).unwrap();
        assert_eq!(initial.family_size, 4);
        assert_eq!(initial.family_frequencies, vec![2, 2]);
        assert_eq!(initial.deletable_members, vec![0, 1, 2]);
        assert_eq!(initial.effective_deletable_members, vec![0, 1, 2]);
        assert!(initial.separating);
        let after_one = or_deletion_state(&[1], 2).unwrap();
        assert_eq!(after_one.family_size, 3);
        assert_eq!(after_one.family_frequencies, vec![1, 2]);
        assert_eq!(after_one.deletion_surpluses, vec![1, -1]);
        assert_eq!(after_one.effective_deletable_members, vec![0]);
        assert!(after_one.separating);
        assert!(or_deletion_state(&[3], 2).is_err());
    }

    #[test]
    fn masks_and_widths_are_checked() {
        assert!(or_closure(&[-1]).is_err());
        assert!(bit_frequencies(&[8], 3).is_err());
        assert!(bit_frequencies(&[0], 64).is_err());
        assert!(or_violation_count(&[8], 3).is_err());
        assert!(or_violation_count(&[0], 21).is_err());
        assert!(or_deletable_members(&[0], 21).is_err());
        assert!(or_deletion_state(&[], 21).is_err());
    }

    #[test]
    fn incremental_rollout_matches_full_transform_at_every_prefix() {
        let rollout =
            or_deletion_rollout(&[], 6, 40, usize::MAX, 1979, OrDeletionObjective::MaxMin, 0)
                .unwrap();
        let mut machine = IncrementalOrDeletion::new(&[], 6).unwrap();
        for prefix in 0..=rollout.deleted.len() {
            let expected = or_deletion_state(&rollout.deleted[..prefix], 6).unwrap();
            let actual = machine.snapshot();
            assert_eq!(actual.family_size, expected.family_size);
            assert_eq!(actual.family_frequencies, expected.family_frequencies);
            assert_eq!(actual.deletion_surpluses, expected.deletion_surpluses);
            assert_eq!(actual.pair_biases, expected.pair_biases);
            assert_eq!(actual.deletable_members, expected.deletable_members);
            assert_eq!(
                actual.effective_deletable_members,
                expected.effective_deletable_members
            );
            assert_eq!(actual.separating, expected.separating);
            if prefix < rollout.deleted.len() {
                machine.remove(rollout.deleted[prefix] as usize).unwrap();
            }
        }
    }

    #[test]
    fn rollout_is_seeded_and_preserves_effective_universe() {
        let left =
            or_deletion_rollout(&[], 8, 100, 12, 42, OrDeletionObjective::MaxMinLowPeak, 250)
                .unwrap();
        let right =
            or_deletion_rollout(&[], 8, 100, 12, 42, OrDeletionObjective::MaxMinLowPeak, 250)
                .unwrap();
        assert_eq!(left.deleted, right.deleted);
        assert_eq!(left.state.family_size, right.state.family_size);
        assert!(left.state.separating);
        assert!(left
            .state
            .family_frequencies
            .iter()
            .all(|count| { count * 1000 >= left.state.family_size * 250 }));
    }

    #[test]
    fn explicit_deletion_matches_the_full_transform() {
        let applied = or_apply_deletion(&[], 4, 1).unwrap();
        let expected = or_deletion_state(&[1], 4).unwrap();
        assert_eq!(applied.deleted, vec![1]);
        assert_eq!(applied.state.family_size, expected.family_size);
        assert_eq!(
            applied.state.family_frequencies,
            expected.family_frequencies
        );
        assert_eq!(applied.state.deletable_members, expected.deletable_members);
        assert!(or_apply_deletion(&[], 4, 15).is_err());
    }

    #[test]
    fn exchange_rollout_preserves_cardinality_closure_and_is_deterministic() {
        let start =
            or_deletion_rollout(&[], 6, 48, usize::MAX, 1979, OrDeletionObjective::MaxMin, 0)
                .unwrap();
        let left = or_exchange_rollout(
            &start.deleted,
            6,
            12,
            64,
            2026,
            OrDeletionObjective::MaxMin,
            0,
            OrExchangeAcceptance::NonWorsening,
            1,
        )
        .unwrap();
        let right = or_exchange_rollout(
            &start.deleted,
            6,
            12,
            64,
            2026,
            OrDeletionObjective::MaxMin,
            0,
            OrExchangeAcceptance::NonWorsening,
            1,
        )
        .unwrap();
        assert_eq!(left.deleted, right.deleted);
        assert_eq!(left.state.family_size, start.state.family_size);
        assert!(left.state.separating);

        let rebuilt = or_deletion_state(&left.deleted, 6).unwrap();
        assert_eq!(left.state.family_size, rebuilt.family_size);
        assert_eq!(left.state.family_frequencies, rebuilt.family_frequencies);
        assert_eq!(left.state.deletion_surpluses, rebuilt.deletion_surpluses);
        assert_eq!(left.state.pair_biases, rebuilt.pair_biases);

        let mut before = start.state.deletion_surpluses;
        let mut after = left.state.deletion_surpluses;
        before.sort_unstable();
        after.sort_unstable();
        assert!(after >= before);
    }

    #[test]
    fn exploratory_exchange_returns_a_deterministic_valid_endpoint() {
        let start =
            or_deletion_rollout(&[], 6, 48, usize::MAX, 1979, OrDeletionObjective::MaxMin, 0)
                .unwrap();
        let left = or_exchange_rollout(
            &start.deleted,
            6,
            24,
            16,
            8675309,
            OrDeletionObjective::MaxMin,
            0,
            OrExchangeAcceptance::Exploratory,
            4,
        )
        .unwrap();
        let right = or_exchange_rollout(
            &start.deleted,
            6,
            24,
            16,
            8675309,
            OrDeletionObjective::MaxMin,
            0,
            OrExchangeAcceptance::Exploratory,
            4,
        )
        .unwrap();
        assert_eq!(left.deleted, right.deleted);
        assert_eq!(left.state.family_size, start.state.family_size);
        assert!(left.state.separating);

        let rebuilt = or_deletion_state(&left.deleted, 6).unwrap();
        assert_eq!(left.state.family_frequencies, rebuilt.family_frequencies);
    }

    #[test]
    fn repair_beam_never_loses_the_greedy_repair_candidate() {
        let start =
            or_deletion_rollout(&[], 6, 48, usize::MAX, 1979, OrDeletionObjective::MaxMin, 0)
                .unwrap();
        let greedy = or_exchange_rollout(
            &start.deleted,
            6,
            1,
            64,
            99,
            OrDeletionObjective::MaxMin,
            0,
            OrExchangeAcceptance::NonWorsening,
            1,
        )
        .unwrap();
        let beamed = or_exchange_rollout(
            &start.deleted,
            6,
            1,
            64,
            99,
            OrDeletionObjective::MaxMin,
            0,
            OrExchangeAcceptance::NonWorsening,
            8,
        )
        .unwrap();
        let greedy_machine = IncrementalOrDeletion::new(&greedy.deleted, 6).unwrap();
        let beamed_machine = IncrementalOrDeletion::new(&beamed.deleted, 6).unwrap();
        let greedy_profile = objective_profile(&greedy_machine, OrDeletionObjective::MaxMin);
        let beamed_profile = objective_profile(&beamed_machine, OrDeletionObjective::MaxMin);
        assert!(!profile_is_better(
            &greedy_profile,
            &beamed_profile,
            OrDeletionObjective::MaxMin,
        ));
    }
}
