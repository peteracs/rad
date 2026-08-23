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
    fn cyclic_pair_lane_batch_matches_generic_profiles() {
        let width = 5;
        let representatives = bitmask_rotation_representatives(width)
            .unwrap()
            .into_iter()
            .filter(|representative| *representative != 0 && *representative != (1 << width) - 1)
            .collect::<Vec<_>>();
        let mut expected = CyclicPairLaneProfile {
            symmetry_classes: 0,
            evaluated: 0,
            equality_families: 0,
            diagonal_equality_families: 0,
            off_diagonal_equality_families: 0,
            full_cube_families: 0,
            equality_non_full_families: 0,
            positive_families: 0,
            negative_families: 0,
            separating_families: 0,
            best_generators: Vec::new(),
            best_frequencies: Vec::new(),
            best_members: 0,
            best_max_frequency: 0,
            best_margin: i64::MAX,
            best_signature: 0,
        };
        for (left_index, left) in representatives.iter().enumerate() {
            let left_orbit = bitmask_rotation_orbit(*left, width).unwrap();
            for (right_index, right) in representatives.iter().enumerate().skip(left_index) {
                let mut generators = left_orbit.clone();
                generators.extend(bitmask_rotation_orbit(*right, width).unwrap());
                let (members, frequencies, separating, signature) =
                    or_closure_stats(&generators, width).unwrap();
                let maximum = *frequencies.iter().max().unwrap();
                let margin = maximum * 2 - members;
                expected.evaluated += 1;
                expected.separating_families += i64::from(separating);
                match margin.cmp(&0) {
                    std::cmp::Ordering::Less => expected.negative_families += 1,
                    std::cmp::Ordering::Equal => {
                        expected.equality_families += 1;
                        if left_index == right_index {
                            expected.diagonal_equality_families += 1;
                        } else {
                            expected.off_diagonal_equality_families += 1;
                        }
                        if members == 1 << width {
                            expected.full_cube_families += 1;
                        } else {
                            expected.equality_non_full_families += 1;
                        }
                    }
                    std::cmp::Ordering::Greater => expected.positive_families += 1,
                }
                let better = separating
                    && (expected.best_members == 0
                        || maximum * expected.best_members
                            < expected.best_max_frequency * members
                        || (maximum * expected.best_members
                            == expected.best_max_frequency * members
                            && members > expected.best_members));
                if better {
                    expected.best_generators = generators;
                    expected.best_frequencies = frequencies;
                    expected.best_members = members;
                    expected.best_max_frequency = maximum;
                    expected.best_margin = margin;
                    expected.best_signature = signature;
                }
            }
        }

        let actual = cyclic_pair_profiles(&representatives, width, 1, 1)
            .unwrap()
            .remove(0);
        assert!(actual.symmetry_classes > 0);
        expected.symmetry_classes = actual.symmetry_classes;
        assert_eq!(actual, expected);
    }

    #[test]
    fn cyclic_pair_width_thirteen_receipt_is_exact() {
        let width = 13;
        let representatives = bitmask_rotation_representatives(width)
            .unwrap()
            .into_iter()
            .filter(|representative| *representative != 0 && *representative != (1 << width) - 1)
            .collect::<Vec<_>>();
        let profile = cyclic_pair_profiles(&representatives, width, 1, 51)
            .unwrap()
            .remove(0);
        assert_eq!(representatives.len(), 630);
        assert_eq!(profile.symmetry_classes, 17_363);
        assert_eq!(profile.evaluated, 198_765);
        assert_eq!(profile.equality_families, 630);
        assert_eq!(profile.diagonal_equality_families, 1);
        assert_eq!(profile.off_diagonal_equality_families, 629);
        assert_eq!(profile.full_cube_families, 630);
        assert_eq!(profile.equality_non_full_families, 0);
        assert_eq!(profile.positive_families, 198_135);
        assert_eq!(profile.negative_families, 0);
        assert_eq!(profile.separating_families, 198_765);
        assert_eq!(profile.best_members, 8192);
        assert_eq!(profile.best_max_frequency, 4096);
        assert_eq!(profile.best_margin, 0);
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
        let config = OrExchangeConfig {
            width: 6,
            steps: 12,
            choices_per_step: 64,
            seed: 2026,
            objective: OrDeletionObjective::MaxMin,
            minimum_density_per_mille: 0,
            acceptance: OrExchangeAcceptance::NonWorsening,
            repair_beam_width: 1,
        };
        let left = or_exchange_rollout(&start.deleted, config).unwrap();
        let right = or_exchange_rollout(&start.deleted, config).unwrap();
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
        let config = OrExchangeConfig {
            width: 6,
            steps: 24,
            choices_per_step: 16,
            seed: 8675309,
            objective: OrDeletionObjective::MaxMin,
            minimum_density_per_mille: 0,
            acceptance: OrExchangeAcceptance::Exploratory,
            repair_beam_width: 4,
        };
        let left = or_exchange_rollout(&start.deleted, config).unwrap();
        let right = or_exchange_rollout(&start.deleted, config).unwrap();
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
        let config = OrExchangeConfig {
            width: 6,
            steps: 1,
            choices_per_step: 64,
            seed: 99,
            objective: OrDeletionObjective::MaxMin,
            minimum_density_per_mille: 0,
            acceptance: OrExchangeAcceptance::NonWorsening,
            repair_beam_width: 1,
        };
        let greedy = or_exchange_rollout(&start.deleted, config).unwrap();
        let beamed = or_exchange_rollout(
            &start.deleted,
            OrExchangeConfig {
                repair_beam_width: 8,
                ..config
            },
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
