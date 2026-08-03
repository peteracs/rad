//! Exact project-owned kernels for affine parity dynamics.
//!
//! The kernel studies the parameterized integer map
//!
//! ```text
//! F(n) = n / 2                 when n is even
//!        (multiplier*n+addend)/2 when n is odd.
//! ```
//!
//! On one residue class modulo `2^d`, every prefix is affine:
//!
//! ```text
//! F^j(n) = (coefficient*n + offset) / 2^j.
//! ```
//!
//! A prefix with `coefficient < 2^j` descends for every
//! `n > offset/(2^j-coefficient)`.  Once that threshold lies below an
//! independently verified convergence bound, the whole residue subtree is
//! impossible for a *least* counterexample and can be pruned exactly. The
//! companion valuation-word kernel is parameterized by the same multiplier
//! and addend. This module belongs to the dogfood extension, not the RAD VM.

use std::collections::BTreeMap;

const MAX_DEPTH: u32 = 50;
const MAX_LANES: u64 = 64;
const MAX_SURVIVOR_SAMPLE: usize = 32;
const MAX_CYCLE_ODD_STEPS: u32 = 12;
const MAX_CYCLE_DIVISIONS: u32 = 28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResidueLaneProfile {
    pub depth: u32,
    pub lane_index: u64,
    pub lane_count: u64,
    pub classes: u64,
    pub residue_sum: u128,
    pub pruned_classes: u64,
    pub survivor_classes: u64,
    pub contracting_survivors: u64,
    pub noncontracting_survivors: u64,
    pub expanded_nodes: u64,
    pub max_odd_steps: u32,
    pub max_odd_residue: u64,
    pub max_threshold: u128,
    pub max_threshold_residue: u64,
    pub prune_depth_histogram: Vec<u64>,
    pub survivor_odd_histogram: Vec<u64>,
    pub survivor_sample: Vec<u64>,
    pub signature: u64,
}

#[derive(Clone, Copy, Debug)]
struct ResidueNode {
    residue: u64,
    coefficient: u128,
    offset: u128,
    denominator: u128,
    probe: u128,
    odd_steps: u32,
}

fn checked_inputs(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_index: u64,
    lane_count: u64,
) -> Result<u32, String> {
    if multiplier < 3 || multiplier.is_multiple_of(2) {
        return Err("affine_parity_profile multiplier must be an odd integer >= 3".into());
    }
    if addend == 0 || addend.is_multiple_of(2) {
        return Err("affine_parity_profile addend must be a positive odd integer".into());
    }
    if depth == 0 || depth > MAX_DEPTH {
        return Err(format!(
            "affine_parity_profile depth must be between 1 and {MAX_DEPTH}"
        ));
    }
    if verified_power > 120 {
        return Err("affine_parity_profile verified power must be at most 120".into());
    }
    if lane_count == 0
        || lane_count > MAX_LANES
        || !lane_count.is_power_of_two()
        || lane_index >= lane_count
    {
        return Err(format!(
            "affine_parity_profile lanes must be a power of two up to {MAX_LANES}, with lane_index in range"
        ));
    }
    let lane_bits = lane_count.trailing_zeros();
    if lane_bits > depth {
        return Err("affine_parity_profile cannot use more lanes than residue classes".into());
    }
    Ok(lane_bits)
}

fn step_node(
    node: ResidueNode,
    extension_bit: u64,
    multiplier: u128,
    addend: u128,
) -> Result<ResidueNode, String> {
    let residue_increment = u64::try_from(node.denominator)
        .map_err(|_| "affine parity residue exceeds u64".to_string())?;
    let residue_increment = extension_bit
        .checked_mul(residue_increment)
        .ok_or_else(|| "affine parity residue increment overflow".to_string())?;
    let residue = node
        .residue
        .checked_add(residue_increment)
        .ok_or_else(|| "affine parity residue overflow".to_string())?;
    // Adding 2^j to the input adds `coefficient` to F^j(input).
    let probe_increment = (extension_bit as u128)
        .checked_mul(node.coefficient)
        .ok_or_else(|| "affine parity probe increment overflow".to_string())?;
    let source = node
        .probe
        .checked_add(probe_increment)
        .ok_or_else(|| "affine parity probe overflow".to_string())?;
    let denominator = node
        .denominator
        .checked_mul(2)
        .ok_or_else(|| "affine parity denominator overflow".to_string())?;

    if source & 1 == 0 {
        return Ok(ResidueNode {
            residue,
            coefficient: node.coefficient,
            offset: node.offset,
            denominator,
            probe: source / 2,
            odd_steps: node.odd_steps,
        });
    }

    let coefficient = node
        .coefficient
        .checked_mul(multiplier)
        .ok_or_else(|| "affine parity coefficient overflow".to_string())?;
    let offset_increment = addend
        .checked_mul(node.denominator)
        .ok_or_else(|| "affine parity offset increment overflow".to_string())?;
    let offset = node
        .offset
        .checked_mul(multiplier)
        .and_then(|value| value.checked_add(offset_increment))
        .ok_or_else(|| "affine parity offset overflow".to_string())?;
    let probe = source
        .checked_mul(multiplier)
        .and_then(|value| value.checked_add(addend))
        .ok_or_else(|| "affine parity probe overflow".to_string())?
        / 2;
    Ok(ResidueNode {
        residue,
        coefficient,
        offset,
        denominator,
        probe,
        odd_steps: node.odd_steps + 1,
    })
}

fn threshold(node: ResidueNode) -> Option<u128> {
    (node.coefficient < node.denominator)
        .then(|| node.offset / (node.denominator - node.coefficient))
}

fn descendant_sum(residue: u64, at_depth: u32, target_depth: u32) -> u128 {
    let count = 1u128 << (target_depth - at_depth);
    count * residue as u128 + (1u128 << at_depth) * count * (count - 1) / 2
}

fn lane_sum(lane_index: u64, lane_count: u64, classes: u64) -> u128 {
    let count = classes as u128;
    count * lane_index as u128 + lane_count as u128 * count * (count - 1) / 2
}

fn prunable(node: ResidueNode, verified_bound: u128) -> bool {
    threshold(node).is_some_and(|limit| limit < verified_bound)
}

/// Analyze one low-bit lane of the residue tree exactly.
///
/// `verified_power` means that every positive integer below `2^verified_power`
/// is independently known to converge.  A pruned class is therefore a proof
/// about a hypothetical *least* counterexample, not a probabilistic guess.
pub(crate) fn residue_lane_profile(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_index: u64,
    lane_count: u64,
) -> Result<ResidueLaneProfile, String> {
    let lane_bits = checked_inputs(
        multiplier,
        addend,
        depth,
        verified_power,
        lane_index,
        lane_count,
    )?;
    let multiplier = multiplier as u128;
    let addend = addend as u128;
    let verified_bound = 1u128 << verified_power;
    let classes = 1u64 << (depth - lane_bits);
    let expected_sum = lane_sum(lane_index, lane_count, classes);
    let mut profile = ResidueLaneProfile {
        depth,
        lane_index,
        lane_count,
        classes,
        residue_sum: 0,
        pruned_classes: 0,
        survivor_classes: 0,
        contracting_survivors: 0,
        noncontracting_survivors: 0,
        expanded_nodes: 0,
        max_odd_steps: 0,
        max_odd_residue: 0,
        max_threshold: 0,
        max_threshold_residue: 0,
        prune_depth_histogram: vec![0; depth as usize + 1],
        survivor_odd_histogram: vec![0; depth as usize + 1],
        survivor_sample: Vec::new(),
        signature: 0,
    };

    let mut node = ResidueNode {
        residue: 0,
        coefficient: 1,
        offset: 0,
        denominator: 1,
        probe: 0,
        odd_steps: 0,
    };
    for bit_index in 0..lane_bits {
        let bit = (lane_index >> bit_index) & 1;
        node = step_node(node, bit, multiplier, addend)?;
        profile.expanded_nodes += 1;
        if prunable(node, verified_bound) {
            profile.pruned_classes = classes;
            profile.residue_sum = expected_sum;
            profile.prune_depth_histogram[(bit_index + 1) as usize] = classes;
            return Ok(profile);
        }
    }

    let mut frontier = vec![node];
    for current_depth in lane_bits..depth {
        let next_depth = current_depth + 1;
        let mut next = Vec::with_capacity(frontier.len().saturating_mul(2));
        for parent in frontier {
            for extension_bit in 0..=1 {
                let child = step_node(parent, extension_bit, multiplier, addend)?;
                profile.expanded_nodes += 1;
                if prunable(child, verified_bound) {
                    let represented = 1u64 << (depth - next_depth);
                    profile.pruned_classes += represented;
                    profile.residue_sum += descendant_sum(child.residue, next_depth, depth);
                    profile.prune_depth_histogram[next_depth as usize] += represented;
                } else {
                    next.push(child);
                }
            }
        }
        frontier = next;
    }

    profile.survivor_classes = frontier.len() as u64;
    for node in frontier {
        profile.residue_sum += node.residue as u128;
        profile.survivor_odd_histogram[node.odd_steps as usize] += 1;
        if node.coefficient < node.denominator {
            profile.contracting_survivors += 1;
            let limit = threshold(node).unwrap_or(0);
            if limit > profile.max_threshold
                || (limit == profile.max_threshold && node.residue < profile.max_threshold_residue)
            {
                profile.max_threshold = limit;
                profile.max_threshold_residue = node.residue;
            }
        } else {
            profile.noncontracting_survivors += 1;
        }
        if node.odd_steps > profile.max_odd_steps
            || (node.odd_steps == profile.max_odd_steps && node.residue < profile.max_odd_residue)
        {
            profile.max_odd_steps = node.odd_steps;
            profile.max_odd_residue = node.residue;
        }
        if profile.survivor_sample.len() < MAX_SURVIVOR_SAMPLE {
            profile.survivor_sample.push(node.residue);
        }
    }
    profile.survivor_sample.sort_unstable();

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-parity-survivors/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&depth.to_le_bytes());
    hasher.update(&verified_power.to_le_bytes());
    hasher.update(&lane_index.to_le_bytes());
    for (odd_steps, count) in profile.survivor_odd_histogram.iter().enumerate() {
        hasher.update(&(odd_steps as u64).to_le_bytes());
        hasher.update(&count.to_le_bytes());
    }
    hasher.update(&profile.survivor_classes.to_le_bytes());
    hasher.update(&profile.max_odd_residue.to_le_bytes());
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    profile.signature = u64::from_le_bytes(prefix) & i64::MAX as u64;

    if profile.pruned_classes + profile.survivor_classes != profile.classes {
        return Err("affine parity internal class partition mismatch".into());
    }
    if profile.residue_sum != expected_sum {
        return Err("affine parity internal residue-sum mismatch".into());
    }
    Ok(profile)
}

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
