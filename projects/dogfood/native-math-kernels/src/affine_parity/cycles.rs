#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CycleProfile {
    pub words_tested: u64,
    pub positive_denominators: u64,
    pub divisible_candidates: u64,
    pub exact_cycle_words: u64,
    pub nonunit_cycle_words: u64,
    pub unit_cycle_words: u64,
    pub first_nonunit_start: u128,
    pub first_nonunit_valuations: Vec<u32>,
    pub closest_q: u32,
    pub closest_divisions: u32,
    pub closest_gap: u128,
    pub signature: u64,
}

fn pow_u128(base: u128, exponent: u32) -> Result<u128, String> {
    (0..exponent).try_fold(1u128, |value, _| {
        value
            .checked_mul(base)
            .ok_or_else(|| "affine cycle coefficient overflow".to_string())
    })
}

fn evaluate_cycle_word(
    multiplier: u128,
    addend: u128,
    word: &[u32],
) -> Result<(u128, u128, u128), String> {
    let mut numerator = 0u128;
    let mut prefix = 0u32;
    for valuation in word {
        numerator = numerator
            .checked_mul(multiplier)
            .and_then(|value| {
                addend
                    .checked_mul(1u128 << prefix)
                    .and_then(|term| value.checked_add(term))
            })
            .ok_or_else(|| "affine cycle numerator overflow".to_string())?;
        prefix += valuation;
    }
    let multiplier_power = pow_u128(multiplier, word.len() as u32)?;
    let two_power = 1u128 << prefix;
    Ok((numerator, two_power, multiplier_power))
}

fn verify_cycle(multiplier: u128, addend: u128, start: u128, word: &[u32]) -> bool {
    if start == 0 || start & 1 == 0 {
        return false;
    }
    let mut value = start;
    for expected in word {
        let Some(next) = value
            .checked_mul(multiplier)
            .and_then(|n| n.checked_add(addend))
        else {
            return false;
        };
        let actual = next.trailing_zeros();
        if actual != *expected {
            return false;
        }
        value = next >> actual;
    }
    value == start
}

fn visit_compositions(
    remaining: u32,
    slots: usize,
    word: &mut Vec<u32>,
    visitor: &mut impl FnMut(&[u32]) -> Result<(), String>,
) -> Result<(), String> {
    if slots == 1 {
        word.push(remaining);
        visitor(word)?;
        word.pop();
        return Ok(());
    }
    let maximum = remaining - (slots as u32 - 1);
    for value in 1..=maximum {
        word.push(value);
        visit_compositions(remaining - value, slots - 1, word, visitor)?;
        word.pop();
    }
    Ok(())
}

/// Exhaust valuation words for positive cycles of the accelerated odd-only
/// affine map. A word `(a_0,...,a_{q-1})` can close only at
///
/// ```text
/// n = C(a_0,...,a_{q-1}) / (2^(sum a_i) - multiplier^q).
/// ```
pub(crate) fn affine_cycle_profile(
    multiplier: u64,
    addend: u64,
    max_odd_steps: u32,
    max_total_divisions: u32,
) -> Result<CycleProfile, String> {
    if multiplier < 3 || multiplier.is_multiple_of(2) {
        return Err("affine_cycle_profile multiplier must be an odd integer >= 3".into());
    }
    if addend == 0 || addend.is_multiple_of(2) {
        return Err("affine_cycle_profile addend must be a positive odd integer".into());
    }
    if max_odd_steps == 0 || max_odd_steps > MAX_CYCLE_ODD_STEPS {
        return Err(format!(
            "affine_cycle_profile max odd steps must be between 1 and {MAX_CYCLE_ODD_STEPS}"
        ));
    }
    if max_total_divisions == 0 || max_total_divisions > MAX_CYCLE_DIVISIONS {
        return Err(format!(
            "affine_cycle_profile max divisions must be between 1 and {MAX_CYCLE_DIVISIONS}"
        ));
    }
    let multiplier = multiplier as u128;
    let addend = addend as u128;

    let mut profile = CycleProfile {
        words_tested: 0,
        positive_denominators: 0,
        divisible_candidates: 0,
        exact_cycle_words: 0,
        nonunit_cycle_words: 0,
        unit_cycle_words: 0,
        first_nonunit_start: 0,
        first_nonunit_valuations: Vec::new(),
        closest_q: 0,
        closest_divisions: 0,
        closest_gap: u128::MAX,
        signature: 0,
    };
    let mut cycle_starts = BTreeMap::<u128, Vec<u32>>::new();

    for q in 1..=max_odd_steps.min(max_total_divisions) {
        for total in q..=max_total_divisions {
            let mut word = Vec::with_capacity(q as usize);
            visit_compositions(total, q as usize, &mut word, &mut |valuations| {
                profile.words_tested += 1;
                let (numerator, two_power, multiplier_power) =
                    evaluate_cycle_word(multiplier, addend, valuations)?;
                if two_power <= multiplier_power {
                    return Ok(());
                }
                profile.positive_denominators += 1;
                let denominator = two_power - multiplier_power;
                if q > 1 && denominator < profile.closest_gap {
                    profile.closest_gap = denominator;
                    profile.closest_q = q;
                    profile.closest_divisions = total;
                }
                if numerator % denominator != 0 {
                    return Ok(());
                }
                profile.divisible_candidates += 1;
                let start = numerator / denominator;
                if !verify_cycle(multiplier, addend, start, valuations) {
                    return Ok(());
                }
                profile.exact_cycle_words += 1;
                cycle_starts
                    .entry(start)
                    .or_insert_with(|| valuations.to_vec());
                if start == 1 {
                    profile.unit_cycle_words += 1;
                } else {
                    profile.nonunit_cycle_words += 1;
                    if profile.first_nonunit_start == 0 || start < profile.first_nonunit_start {
                        profile.first_nonunit_start = start;
                        profile.first_nonunit_valuations = valuations.to_vec();
                    }
                }
                Ok(())
            })?;
        }
    }
    if profile.closest_gap == u128::MAX {
        profile.closest_gap = 0;
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-cycle-words/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&max_odd_steps.to_le_bytes());
    hasher.update(&max_total_divisions.to_le_bytes());
    for (start, word) in cycle_starts {
        hasher.update(&start.to_le_bytes());
        for valuation in word {
            hasher.update(&valuation.to_le_bytes());
        }
    }
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    profile.signature = u64::from_le_bytes(prefix) & i64::MAX as u64;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    type BruteSparseSupport = (Vec<u64>, Vec<i64>, Vec<u64>, Vec<u32>, Vec<u64>);

    fn brute_survivors(
        multiplier: u128,
        addend: u128,
        depth: u32,
        verified_power: u32,
    ) -> (u64, Vec<u64>) {
        let bound = 1u128 << verified_power;
        let mut survivors = 0;
        let mut histogram = vec![0u64; depth as usize + 1];
        for residue in 0..(1u64 << depth) {
            let mut probe = residue as u128;
            let mut coefficient = 1u128;
            let mut offset = 0u128;
            let mut denominator = 1u128;
            let mut odd_steps = 0usize;
            let mut pruned = false;
            for _ in 0..depth {
                if probe & 1 == 1 {
                    probe = (multiplier * probe + addend) / 2;
                    coefficient *= multiplier;
                    offset = multiplier * offset + addend * denominator;
                    odd_steps += 1;
                } else {
                    probe /= 2;
                }
                denominator *= 2;
                if coefficient < denominator && offset / (denominator - coefficient) < bound {
                    pruned = true;
                    break;
                }
            }
            if !pruned {
                survivors += 1;
                histogram[odd_steps] += 1;
            }
        }
        (survivors, histogram)
    }

    fn brute_natural_tails(
        multiplier: u128,
        addend: u128,
        depth: u32,
        verified_power: u32,
        max_steps: u32,
    ) -> (u64, u64, u64, Vec<u64>, Vec<u64>, u128, u64) {
        let bound = 1u128 << verified_power;
        let mut survivors = 0u64;
        let mut coefficient_stops = 0u64;
        let mut descents = 0u64;
        let mut coefficient_histogram = vec![0u64; max_steps as usize + 1];
        let mut descent_histogram = vec![0u64; max_steps as usize + 1];
        let mut maximum_peak = 0u128;
        let mut maximum_peak_residue = 0u64;

        for residue in 0..(1u64 << depth) {
            let start = residue as u128;
            let mut value = start;
            let mut peak = start;
            let mut coefficient = 1u128;
            let mut offset = 0u128;
            let mut denominator = 1u128;
            let mut pruned = false;
            for _ in 0..depth {
                if value & 1 == 1 {
                    value = (multiplier * value + addend) / 2;
                    coefficient *= multiplier;
                    offset = multiplier * offset + addend * denominator;
                } else {
                    value /= 2;
                }
                denominator *= 2;
                peak = peak.max(value);
                if coefficient < denominator && offset / (denominator - coefficient) < bound {
                    pruned = true;
                    break;
                }
            }
            if pruned {
                continue;
            }
            survivors += 1;
            let mut coefficient_stop = (coefficient < denominator).then_some(depth);
            let mut descent = (value < start).then_some(depth);
            for step in (depth + 1)..=max_steps {
                if value & 1 == 1 {
                    value = (multiplier * value + addend) / 2;
                    coefficient *= multiplier;
                } else {
                    value /= 2;
                }
                denominator *= 2;
                peak = peak.max(value);
                if coefficient_stop.is_none() && coefficient < denominator {
                    coefficient_stop = Some(step);
                }
                if descent.is_none() && value < start {
                    descent = Some(step);
                }
                if coefficient_stop.is_some() && descent.is_some() {
                    break;
                }
            }
            if let Some(step) = coefficient_stop {
                coefficient_stops += 1;
                coefficient_histogram[step as usize] += 1;
            }
            if let Some(step) = descent {
                descents += 1;
                descent_histogram[step as usize] += 1;
            }
            if peak > maximum_peak || (peak == maximum_peak && residue < maximum_peak_residue) {
                maximum_peak = peak;
                maximum_peak_residue = residue;
            }
        }
        (
            survivors,
            coefficient_stops,
            descents,
            coefficient_histogram,
            descent_histogram,
            maximum_peak,
            maximum_peak_residue,
        )
    }

    fn brute_sparse_support(
        multiplier: u128,
        addend: u128,
        max_depth: u32,
        verified_power: u32,
        max_input_ones: u32,
    ) -> BruteSparseSupport {
        let bound = 1u128 << verified_power;
        let mut counts = vec![0u64; max_depth as usize + 1];
        let mut minimum_weights = vec![-1i64; max_depth as usize + 1];
        let mut minimum_witnesses = vec![0u64; max_depth as usize + 1];
        let mut deepest_by_weight = vec![0u32; max_input_ones as usize + 1];
        let mut deepest_witnesses = vec![0u64; max_input_ones as usize + 1];
        counts[0] = 1;
        minimum_weights[0] = 0;

        for depth in 1..=max_depth {
            for residue in 0..(1u64 << depth) {
                let weight = residue.count_ones();
                if weight > max_input_ones {
                    continue;
                }
                let mut value = residue as u128;
                let mut coefficient = 1u128;
                let mut offset = 0u128;
                let mut denominator = 1u128;
                let mut alive = true;
                for _ in 0..depth {
                    if value & 1 == 1 {
                        value = (multiplier * value + addend) / 2;
                        coefficient *= multiplier;
                        offset = multiplier * offset + addend * denominator;
                    } else {
                        value /= 2;
                    }
                    denominator *= 2;
                    if coefficient < denominator && offset / (denominator - coefficient) < bound {
                        alive = false;
                        break;
                    }
                }
                if !alive {
                    continue;
                }
                counts[depth as usize] += 1;
                if minimum_weights[depth as usize] < 0
                    || i64::from(weight) < minimum_weights[depth as usize]
                {
                    minimum_weights[depth as usize] = i64::from(weight);
                    minimum_witnesses[depth as usize] = residue;
                } else if i64::from(weight) == minimum_weights[depth as usize] {
                    minimum_witnesses[depth as usize] =
                        minimum_witnesses[depth as usize].min(residue);
                }
                let weight_index = weight as usize;
                if depth > deepest_by_weight[weight_index]
                    || (depth == deepest_by_weight[weight_index]
                        && residue < deepest_witnesses[weight_index])
                {
                    deepest_by_weight[weight_index] = depth;
                    deepest_witnesses[weight_index] = residue;
                }
            }
        }
        (
            counts,
            minimum_weights,
            minimum_witnesses,
            deepest_by_weight,
            deepest_witnesses,
        )
    }

    #[test]
    fn residue_lanes_partition_the_complete_cube() {
        let depth = 16;
        let lanes = 8;
        let profiles = (0..lanes)
            .map(|lane| residue_lane_profile(3, 1, depth, 20, lane, lanes).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(profiles.iter().map(|p| p.classes).sum::<u64>(), 1 << depth);
        assert_eq!(
            profiles.iter().map(|p| p.residue_sum).sum::<u128>(),
            (1u128 << depth) * ((1u128 << depth) - 1) / 2
        );
        assert!(profiles.iter().all(|p| {
            p.pruned_classes + p.survivor_classes == p.classes
                && p.contracting_survivors + p.noncontracting_survivors == p.survivor_classes
        }));
    }

    #[test]
    fn concurrent_batch_matches_canonical_sequential_lanes() {
        let sequential = (0..8)
            .map(|lane| residue_lane_profile(5, 3, 14, 18, lane, 8).unwrap())
            .collect::<Vec<_>>();
        let concurrent = residue_lane_profiles(5, 3, 14, 18, 8).unwrap();
        assert_eq!(concurrent.len(), sequential.len());
        for (actual, expected) in concurrent.iter().zip(&sequential) {
            assert_eq!(actual.lane_index, expected.lane_index);
            assert_eq!(actual.classes, expected.classes);
            assert_eq!(actual.residue_sum, expected.residue_sum);
            assert_eq!(actual.pruned_classes, expected.pruned_classes);
            assert_eq!(actual.survivor_classes, expected.survivor_classes);
            assert_eq!(actual.prune_depth_histogram, expected.prune_depth_histogram);
            assert_eq!(
                actual.survivor_odd_histogram,
                expected.survivor_odd_histogram
            );
            assert_eq!(actual.signature, expected.signature);
        }
    }

    #[test]
    fn pruned_tree_matches_independent_brute_force_for_small_general_maps() {
        for (multiplier, addend) in [(3, 1), (5, 1), (3, 5)] {
            for depth in 3..=11 {
                let expected = brute_survivors(multiplier, addend, depth, 12);
                let lanes = 4.min(1u64 << depth);
                let profiles = (0..lanes)
                    .map(|lane| {
                        residue_lane_profile(
                            multiplier as u64,
                            addend as u64,
                            depth,
                            12,
                            lane,
                            lanes,
                        )
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    profiles
                        .iter()
                        .map(|profile| profile.survivor_classes)
                        .sum::<u64>(),
                    expected.0,
                    "survivor mismatch for ({multiplier},{addend}) depth {depth}"
                );
                let mut histogram = vec![0u64; depth as usize + 1];
                for profile in profiles {
                    for (slot, count) in histogram.iter_mut().zip(profile.survivor_odd_histogram) {
                        *slot += count;
                    }
                }
                assert_eq!(histogram, expected.1);
            }
        }
    }

    #[test]
    fn natural_tail_profiles_match_independent_small_cube_enumeration() {
        let expected = brute_natural_tails(3, 1, 8, 12, 128);
        let profiles = natural_tail_lane_profiles(3, 1, 8, 12, 4, 128).unwrap();
        assert_eq!(
            profiles.iter().map(|p| p.survivor_classes).sum::<u64>(),
            expected.0
        );
        assert_eq!(
            profiles.iter().map(|p| p.coefficient_stops).sum::<u64>(),
            expected.1
        );
        assert_eq!(profiles.iter().map(|p| p.descents).sum::<u64>(), expected.2);
        assert_eq!(profiles.iter().map(|p| p.unresolved).sum::<u64>(), 0);
        let actual_peak = profiles
            .iter()
            .map(|profile| (profile.max_peak, profile.max_peak_residue))
            .max_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)))
            .unwrap();
        assert_eq!(actual_peak, (expected.5, expected.6));
        let mut coefficient_histogram = vec![0u64; 129];
        let mut descent_histogram = vec![0u64; 129];
        for profile in profiles {
            for (total, count) in coefficient_histogram
                .iter_mut()
                .zip(profile.coefficient_stop_histogram)
            {
                *total += count;
            }
            for (total, count) in descent_histogram.iter_mut().zip(profile.descent_histogram) {
                *total += count;
            }
        }
        assert_eq!(coefficient_histogram, expected.3);
        assert_eq!(descent_histogram, expected.4);
    }

    #[test]
    fn sparse_support_profile_matches_independent_small_cube_enumeration() {
        let expected = brute_sparse_support(3, 1, 12, 12, 4);
        let actual = sparse_support_profile(3, 1, 12, 12, 4).unwrap();
        assert_eq!(actual.survivors_by_depth, expected.0);
        assert_eq!(actual.minimum_input_ones_by_depth, expected.1);
        assert_eq!(
            actual.minimum_weight_witness_by_depth,
            expected
                .2
                .into_iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(actual.deepest_survival_by_weight, expected.3);
        assert_eq!(
            actual.deepest_witness_by_weight,
            expected
                .4
                .into_iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(actual.terminated, actual.survivors_by_depth[12] == 0);
    }

    #[test]
    fn sparse_support_kernel_is_parameterized_over_the_affine_map() {
        let expected = brute_sparse_support(5, 3, 10, 12, 3);
        let actual = sparse_support_profile(5, 3, 10, 12, 3).unwrap();
        assert_eq!(actual.survivors_by_depth, expected.0);
        assert_eq!(actual.minimum_input_ones_by_depth, expected.1);
        assert_eq!(actual.deepest_survival_by_weight, expected.3);
    }

    #[test]
    fn sparse_anchor_summary_matches_detailed_frontier_profile() {
        let detailed = sparse_support_profile(3, 1, 16, 20, 5).unwrap();
        let summary = sparse_support_summary(3, 1, 16, 20, 5).unwrap();
        assert_eq!(
            summary.deepest_survival_by_weight[..5],
            detailed.deepest_survival_by_weight[..5]
        );
        assert_eq!(
            summary.deepest_witness_by_weight[..5],
            detailed.deepest_witness_by_weight[..5]
        );
        assert!(summary.deepest_survival_by_weight[5] <= detailed.deepest_survival_by_weight[5]);
        for budget in 0..5usize {
            let deepest = summary.deepest_survival_by_weight[..=budget]
                .iter()
                .copied()
                .max()
                .unwrap();
            let expected = if deepest < 16 { deepest + 1 } else { 0 };
            assert_eq!(summary.termination_depth_by_budget[budget], expected);
        }
    }

    #[test]
    fn sparse_anchor_summary_is_parameterized() {
        let detailed = sparse_support_profile(5, 3, 14, 20, 4).unwrap();
        let summary = sparse_support_summary(5, 3, 14, 20, 4).unwrap();
        assert_eq!(
            summary.deepest_survival_by_weight[..4],
            detailed.deepest_survival_by_weight[..4]
        );
        assert_eq!(
            summary.deepest_witness_by_weight[..4],
            detailed.deepest_witness_by_weight[..4]
        );
        assert!(summary.deepest_survival_by_weight[4] <= detailed.deepest_survival_by_weight[4]);
    }

    #[test]
    fn sparse_anchor_summary_matches_detailed_support_seven_certificate() {
        let detailed = sparse_support_profile(3, 1, 400, 71, 7).unwrap();
        let summary = sparse_support_summary(3, 1, 400, 71, 7).unwrap();
        assert_eq!(
            summary.deepest_survival_by_weight,
            detailed.deepest_survival_by_weight
        );
        assert_eq!(
            summary.deepest_witness_by_weight,
            detailed.deepest_witness_by_weight
        );
        assert_eq!(
            summary.termination_depth_by_budget,
            vec![1, 2, 4, 7, 59, 137, 214, 365]
        );
    }

    #[test]
    fn sparse_slope_and_descent_frontiers_agree_through_support_seven() {
        let descent = sparse_support_summary(3, 1, 400, 71, 7).unwrap();
        let slope = sparse_slope_support_summary(3, 1, 400, 7).unwrap();
        assert_eq!(
            slope.termination_depth_by_budget,
            descent.termination_depth_by_budget
        );
        assert_eq!(
            slope.deepest_survival_by_weight,
            descent.deepest_survival_by_weight
        );
        assert_eq!(
            slope.deepest_witness_by_weight,
            descent.deepest_witness_by_weight
        );
    }

    #[test]
    fn sparse_slope_lanes_merge_to_the_monolithic_summary() {
        let full = sparse_slope_support_summary(3, 1, 400, 7).unwrap();
        let lanes = (0..4)
            .map(|lane| sparse_slope_support_lane_summary(3, 1, 400, 7, lane, 4).unwrap())
            .collect::<Vec<_>>();
        assert!(lanes
            .iter()
            .all(|lane| lane.seed_count == lanes[0].seed_count));
        assert_eq!(
            lanes
                .iter()
                .map(|lane| lane.assigned_seed_count)
                .sum::<u64>(),
            lanes[0].seed_count
        );
        assert_eq!(
            lanes.iter().map(|lane| lane.expanded_nodes).sum::<u64>(),
            full.expanded_nodes
        );
        for weight in 0..=7usize {
            assert_eq!(
                lanes
                    .iter()
                    .map(|lane| lane.anchors_by_weight[weight])
                    .sum::<u64>(),
                full.anchors_by_weight[weight]
            );
            let deepest = lanes
                .iter()
                .map(|lane| lane.deepest_survival_by_weight[weight])
                .max()
                .unwrap();
            assert_eq!(deepest, full.deepest_survival_by_weight[weight]);
            let witness = lanes
                .iter()
                .filter(|lane| lane.deepest_survival_by_weight[weight] == deepest)
                .map(|lane| lane.deepest_witness_by_weight[weight].as_str())
                .filter(|value| *value != "0" || weight == 0)
                .min()
                .unwrap();
            assert_eq!(witness, full.deepest_witness_by_weight[weight]);
        }
    }

    #[test]
    fn concurrent_natural_tail_batch_matches_sequential_canonical_lanes() {
        let sequential = (0..4)
            .map(|lane| natural_tail_lane_profile(5, 3, 8, 12, lane, 4, 128).unwrap())
            .collect::<Vec<_>>();
        let concurrent = natural_tail_lane_profiles(5, 3, 8, 12, 4, 128).unwrap();
        assert_eq!(concurrent, sequential);
    }

    #[test]
    fn verified_bound_prunes_a_known_small_parameterized_instance() {
        let profile = residue_lane_profile(3, 1, 12, 20, 0, 1).unwrap();
        assert_eq!(profile.classes, 4096);
        assert_eq!(profile.pruned_classes + profile.survivor_classes, 4096);
        assert_eq!(profile.survivor_classes, 226);
        assert_eq!(profile.contracting_survivors, 0);
        assert_eq!(profile.max_odd_steps, 12);
        assert_eq!(profile.max_odd_residue, 4095);
    }

    #[test]
    fn all_odd_prefix_is_the_visible_two_adic_obstruction() {
        let profile = residue_lane_profile(3, 1, 20, 71, 0, 1).unwrap();
        assert_eq!(profile.survivor_classes, 27_328);
        assert_eq!(profile.max_odd_steps, 20);
        assert_eq!(profile.max_odd_residue, (1 << 20) - 1);
    }

    #[test]
    fn cycle_equation_finds_only_repetitions_of_the_trivial_cycle_in_small_box() {
        let profile = affine_cycle_profile(3, 1, 8, 18).unwrap();
        assert!(profile.words_tested > 10_000);
        assert!(profile.exact_cycle_words >= 1);
        assert_eq!(profile.nonunit_cycle_words, 0);
        assert_eq!(profile.exact_cycle_words, profile.unit_cycle_words);
    }

    #[test]
    fn cycle_word_equation_matches_the_trivial_cycle() {
        let (numerator, two_power, multiplier_power) = evaluate_cycle_word(3, 1, &[2]).unwrap();
        assert_eq!((numerator, two_power, multiplier_power), (1, 4, 3));
        assert!(verify_cycle(
            3,
            1,
            numerator / (two_power - multiplier_power),
            &[2]
        ));
    }

    #[test]
    fn cycle_equation_is_parameterized_over_the_affine_map() {
        let (numerator, two_power, multiplier_power) = evaluate_cycle_word(5, 3, &[3]).unwrap();
        assert_eq!((numerator, two_power, multiplier_power), (3, 8, 5));
        assert!(verify_cycle(
            5,
            3,
            numerator / (two_power - multiplier_power),
            &[3]
        ));

        let profile = affine_cycle_profile(5, 3, 3, 9).unwrap();
        assert!(profile.unit_cycle_words >= 1);
        assert!(profile.exact_cycle_words >= profile.unit_cycle_words);
    }
}
