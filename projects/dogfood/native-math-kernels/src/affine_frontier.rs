//! Bounded, deterministic frontier exploration for odd affine parity maps.
//!
//! This module is deliberately heuristic: unlike the exhaustive certificate
//! kernel in `affine_parity`, a beam may discard a later-winning prefix.  Its
//! role is counterexample generation and invariant discovery.  All retained
//! states and reported witnesses are nevertheless evaluated with exact
//! integers.

use num_bigint::BigUint;
use rayon::prelude::*;
use smallvec::SmallVec;
use std::cmp::Ordering;

const MAX_FRONTIER_DEPTH: u32 = 16_384;
const MAX_FRONTIER_SUPPORT: u32 = 128;
const MAX_BEAM_PER_SUPPORT: usize = 100_000;
const MAX_REPORTED_RECORDS: usize = 4_096;
const ZERO_BLOCK_BITS: usize = 16;
const ZERO_BLOCK_CARDINALITY: usize = 1 << ZERO_BLOCK_BITS;

type Positions = SmallVec<[u16; 16]>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrontierObjective {
    ZeroRunway,
    OddHeadroom,
    SmallProbe,
    DeterministicMix,
}

impl FrontierObjective {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "zero_runway" => Ok(Self::ZeroRunway),
            "odd_headroom" => Ok(Self::OddHeadroom),
            "small_probe" => Ok(Self::SmallProbe),
            "deterministic_mix" => Ok(Self::DeterministicMix),
            _ => Err(format!(
                "affine frontier objective must be zero_runway, odd_headroom, small_probe, or deterministic_mix; got {value:?}"
            )),
        }
    }
}

#[derive(Clone, Debug)]
struct FrontierNode {
    positions: Positions,
    probe: BigUint,
    odd_steps: u32,
    zero_death_depth: u32,
    rank_runway: u32,
    rank_mix: u64,
}

#[derive(Clone, Debug)]
struct ZeroBlock {
    odd_steps: u8,
    odd_prefix: [u8; ZERO_BLOCK_BITS],
    offset: BigUint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrontierRecord {
    pub depth: u32,
    pub minimum_input_ones: u32,
    pub witness: String,
    pub one_positions: Vec<u32>,
    pub odd_steps: u32,
    pub coefficient_bits: u64,
    pub probe_bits: u64,
    pub zero_runway: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AffineFrontierProfile {
    pub objective: String,
    pub max_depth: u32,
    pub max_input_ones: u32,
    pub beam_per_support: usize,
    pub reached_depth: u32,
    pub expanded_nodes: u64,
    pub peak_frontier: usize,
    pub records: Vec<FrontierRecord>,
    pub deepest_retained_by_support: Vec<FrontierRecord>,
    pub terminal_minimum_input_ones: Option<u32>,
    pub terminal_witness: Option<String>,
    pub terminal_one_positions: Vec<u32>,
    pub signature: u64,
}

fn validate_inputs(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    max_input_ones: u32,
    beam_per_support: usize,
) -> Result<(), String> {
    if multiplier < 3 || multiplier.is_multiple_of(2) {
        return Err("affine frontier multiplier must be an odd integer >= 3".into());
    }
    if addend == 0 || addend.is_multiple_of(2) {
        return Err("affine frontier addend must be a positive odd integer".into());
    }
    if max_depth == 0 || max_depth > MAX_FRONTIER_DEPTH {
        return Err(format!(
            "affine frontier depth must be between 1 and {MAX_FRONTIER_DEPTH}"
        ));
    }
    if max_input_ones == 0 || max_input_ones > max_depth || max_input_ones > MAX_FRONTIER_SUPPORT {
        return Err(format!(
            "affine frontier input-one budget must be between 1 and depth and at most {MAX_FRONTIER_SUPPORT}"
        ));
    }
    if beam_per_support == 0 || beam_per_support > MAX_BEAM_PER_SUPPORT {
        return Err(format!(
            "affine frontier beam per support must be between 1 and {MAX_BEAM_PER_SUPPORT}"
        ));
    }
    Ok(())
}

fn positions_value(positions: &[u16]) -> BigUint {
    let mut value = BigUint::from(0u8);
    for position in positions {
        value |= BigUint::from(1u8) << usize::from(*position);
    }
    value
}

fn compare_positions(left: &[u16], right: &[u16]) -> Ordering {
    left.iter().rev().cmp(right.iter().rev())
}

fn build_zero_blocks(multiplier: u64, addend: u64) -> Vec<ZeroBlock> {
    let multiplier_big = BigUint::from(multiplier);
    let addend_big = BigUint::from(addend);
    let mut multiplier_powers = Vec::with_capacity(ZERO_BLOCK_BITS + 1);
    let mut multiplier_power = BigUint::from(1u8);
    for _ in 0..=ZERO_BLOCK_BITS {
        multiplier_powers.push(multiplier_power.clone());
        multiplier_power *= &multiplier_big;
    }
    (0..ZERO_BLOCK_CARDINALITY)
        .map(|residue| {
            let residue_big = BigUint::from(residue);
            let mut value = residue_big.clone();
            let mut odd_steps = 0u8;
            let mut odd_prefix = [0u8; ZERO_BLOCK_BITS];
            for prefix in &mut odd_prefix {
                if value.bit(0) {
                    value = value * &multiplier_big + &addend_big;
                    odd_steps += 1;
                }
                value >>= 1usize;
                *prefix = odd_steps;
            }
            let offset = (value << ZERO_BLOCK_BITS)
                - &multiplier_powers[usize::from(odd_steps)] * residue_big;
            ZeroBlock {
                odd_steps,
                odd_prefix,
                offset,
            }
        })
        .collect()
}

fn shared_zero_blocks(multiplier: u64, addend: u64) -> std::sync::Arc<Vec<ZeroBlock>> {
    type Cache = std::collections::HashMap<(u64, u64), std::sync::Arc<Vec<ZeroBlock>>>;
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| std::sync::Mutex::new(Cache::new()))
        .lock()
        .expect("zero-block cache lock was poisoned");
    std::sync::Arc::clone(
        cache
            .entry((multiplier, addend))
            .or_insert_with(|| std::sync::Arc::new(build_zero_blocks(multiplier, addend))),
    )
}

fn step(
    node: &FrontierNode,
    extension_bit: u32,
    parent_depth: u32,
    multiplier: &BigUint,
    addend: &BigUint,
    multiplier_powers: &[BigUint],
) -> FrontierNode {
    let coefficient = &multiplier_powers[node.odd_steps as usize];
    let mut positions = node.positions.clone();
    let source = if extension_bit == 0 {
        node.probe.clone()
    } else {
        positions.push(parent_depth as u16);
        &node.probe + coefficient
    };
    if !source.bit(0) {
        return FrontierNode {
            positions,
            probe: source >> 1usize,
            odd_steps: node.odd_steps,
            zero_death_depth: node.zero_death_depth,
            rank_runway: 0,
            rank_mix: node.rank_mix,
        };
    }
    FrontierNode {
        positions,
        probe: (source * multiplier + addend) >> 1usize,
        odd_steps: node.odd_steps + 1,
        zero_death_depth: if extension_bit == 0 {
            node.zero_death_depth
        } else {
            0
        },
        rank_runway: 0,
        rank_mix: node.rank_mix,
    }
}

fn zero_death_depth(
    node: &FrontierNode,
    multiplier: &BigUint,
    addend: &BigUint,
    multiplier_powers: &[BigUint],
    blocks: &[ZeroBlock],
    start_depth: u32,
    max_depth: u32,
) -> u32 {
    let mut probe = node.probe.clone();
    let mut odd_steps = node.odd_steps;
    let mut current_depth = start_depth;
    while max_depth - current_depth >= ZERO_BLOCK_BITS as u32 {
        let residue =
            probe.iter_u32_digits().next().unwrap_or(0) as usize & (ZERO_BLOCK_CARDINALITY - 1);
        let block = &blocks[residue];
        for (index, prefix_odd_steps) in block.odd_prefix.iter().enumerate() {
            let depth = current_depth + index as u32 + 1;
            let coefficient_bits =
                multiplier_powers[odd_steps as usize + usize::from(*prefix_odd_steps)].bits();
            if coefficient_bits <= u64::from(depth) {
                return depth;
            }
        }
        probe = (probe * &multiplier_powers[usize::from(block.odd_steps)] + &block.offset)
            >> ZERO_BLOCK_BITS;
        odd_steps += u32::from(block.odd_steps);
        current_depth += ZERO_BLOCK_BITS as u32;
    }
    while current_depth < max_depth {
        let numerator = if probe.bit(0) {
            odd_steps += 1;
            probe * multiplier + addend
        } else {
            probe
        };
        let remaining = u64::from(max_depth - current_depth);
        let jump = numerator
            .trailing_zeros()
            .unwrap_or(remaining)
            .min(remaining);
        let coefficient_bits = multiplier_powers[odd_steps as usize].bits();
        let contraction_depth = u64::from(current_depth) + jump;
        if coefficient_bits <= contraction_depth {
            return u32::try_from(coefficient_bits.max(u64::from(current_depth) + 1))
                .unwrap_or(max_depth);
        }
        probe = numerator >> jump as usize;
        current_depth += u32::try_from(jump).unwrap_or(max_depth - current_depth);
    }
    max_depth + 1
}

fn ranked_runway(node: &FrontierNode, depth: u32, remaining_depth: u32) -> u32 {
    node.zero_death_depth
        .saturating_sub(depth + 1)
        .min(remaining_depth)
}

fn retain_zero_runway_beam(
    bucket: &mut Vec<FrontierNode>,
    beam: usize,
    depth: u32,
    max_depth: u32,
    multiplier: &BigUint,
    addend: &BigUint,
    multiplier_powers: &[BigUint],
    blocks: &[ZeroBlock],
) {
    for candidate in bucket.iter_mut() {
        if candidate.zero_death_depth == 0 {
            candidate.zero_death_depth = zero_death_depth(
                candidate,
                multiplier,
                addend,
                multiplier_powers,
                blocks,
                depth,
                max_depth,
            );
            candidate.rank_runway = ranked_runway(&candidate, depth, (max_depth - depth).min(512));
        }
    }
    if bucket.len() > beam {
        bucket.select_nth_unstable_by(beam, |left, right| {
            compare_nodes(left, right, FrontierObjective::ZeroRunway, 0)
        });
        bucket.truncate(beam);
    }
}

fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn mixed_root(seed: u64) -> u64 {
    mix64(seed ^ 0x7261_642d_6672_6f6e)
}

fn mixed_child(parent_key: u64, parent_depth: u32, extension_bit: u32) -> u64 {
    let decision = (u64::from(parent_depth) << 1) | u64::from(extension_bit);
    mix64(parent_key ^ decision.wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

fn compare_nodes(
    left: &FrontierNode,
    right: &FrontierNode,
    objective: FrontierObjective,
    _seed: u64,
) -> Ordering {
    let primary = match objective {
        FrontierObjective::ZeroRunway => right.rank_runway.cmp(&left.rank_runway),
        FrontierObjective::OddHeadroom => right.odd_steps.cmp(&left.odd_steps),
        FrontierObjective::SmallProbe => left
            .probe
            .bits()
            .cmp(&right.probe.bits())
            .then_with(|| left.probe.cmp(&right.probe)),
        FrontierObjective::DeterministicMix => left.rank_mix.cmp(&right.rank_mix),
    };
    primary
        .then_with(|| right.odd_steps.cmp(&left.odd_steps))
        .then_with(|| left.probe.bits().cmp(&right.probe.bits()))
        .then_with(|| compare_positions(&left.positions, &right.positions))
}

fn record_for(
    node: &FrontierNode,
    depth: u32,
    multiplier: &BigUint,
    addend: &BigUint,
    multiplier_powers: &[BigUint],
    blocks: &[ZeroBlock],
    remaining_depth: u32,
) -> FrontierRecord {
    let residue = positions_value(&node.positions);
    let death_depth = if node.zero_death_depth == 0 {
        zero_death_depth(
            node,
            multiplier,
            addend,
            multiplier_powers,
            blocks,
            depth,
            depth + remaining_depth,
        )
    } else {
        node.zero_death_depth
    };
    FrontierRecord {
        depth,
        minimum_input_ones: node.positions.len() as u32,
        witness: residue.to_string(),
        one_positions: node.positions.iter().copied().map(u32::from).collect(),
        odd_steps: node.odd_steps,
        coefficient_bits: multiplier_powers[node.odd_steps as usize].bits(),
        probe_bits: node.probe.bits(),
        zero_runway: death_depth.saturating_sub(depth + 1).min(remaining_depth),
    }
}

/// Explore a bounded exact-integer beam of prefix-noncontracting states.
///
/// Results are witnesses, never exhaustion certificates.  Retention happens
/// independently for each observed input support so a populous low-support
/// layer cannot erase all higher-support renewal candidates.
pub(crate) fn affine_frontier_profile(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    max_input_ones: u32,
    beam_per_support: usize,
    objective_name: &str,
    seed: u64,
) -> Result<AffineFrontierProfile, String> {
    validate_inputs(
        multiplier,
        addend,
        max_depth,
        max_input_ones,
        beam_per_support,
    )?;
    let objective = FrontierObjective::parse(objective_name)?;
    let multiplier_big = BigUint::from(multiplier);
    let addend_big = BigUint::from(addend);
    let mut multiplier_powers = Vec::with_capacity(max_depth as usize + 1);
    let mut multiplier_power = BigUint::from(1u8);
    for _ in 0..=max_depth {
        multiplier_powers.push(multiplier_power.clone());
        multiplier_power *= &multiplier_big;
    }
    let blocks = shared_zero_blocks(multiplier, addend);
    let mut frontier = vec![FrontierNode {
        positions: Positions::new(),
        probe: BigUint::from(0u8),
        odd_steps: 0,
        zero_death_depth: 1,
        rank_runway: 0,
        rank_mix: mixed_root(seed),
    }];
    let mut records = Vec::new();
    let mut prior_minimum = None;
    let mut expanded_nodes = 0u64;
    let mut peak_frontier = 1usize;
    let mut reached_depth = 0u32;
    let mut deepest_retained = vec![None::<(u32, FrontierNode)>; max_input_ones as usize + 1];

    for depth in 1..=max_depth {
        let mut buckets = (0..=max_input_ones)
            .map(|_| Vec::<FrontierNode>::new())
            .collect::<Vec<_>>();
        for parent in &frontier {
            for extension_bit in 0..=1 {
                if parent.positions.len() as u32 + extension_bit > max_input_ones {
                    continue;
                }
                let mut child = step(
                    parent,
                    extension_bit,
                    depth - 1,
                    &multiplier_big,
                    &addend_big,
                    &multiplier_powers,
                );
                expanded_nodes = expanded_nodes
                    .checked_add(1)
                    .ok_or_else(|| "affine frontier expansion count overflow".to_string())?;
                if multiplier_powers[child.odd_steps as usize].bits() > u64::from(depth) {
                    if matches!(objective, FrontierObjective::ZeroRunway) {
                        child.rank_runway = if extension_bit == 0 {
                            ranked_runway(&child, depth, (max_depth - depth).min(512))
                        } else {
                            (max_depth - depth).min(512)
                        };
                    } else if matches!(objective, FrontierObjective::DeterministicMix) {
                        child.rank_mix = mixed_child(parent.rank_mix, depth - 1, extension_bit);
                    }
                    buckets[child.positions.len()].push(child);
                }
            }
        }

        let remaining_depth = max_depth - depth;
        if matches!(objective, FrontierObjective::ZeroRunway) {
            buckets.par_iter_mut().for_each(|bucket| {
                retain_zero_runway_beam(
                    bucket,
                    beam_per_support,
                    depth,
                    max_depth,
                    &multiplier_big,
                    &addend_big,
                    &multiplier_powers,
                    blocks.as_slice(),
                );
            });
        }
        frontier.clear();
        for bucket in &mut buckets {
            if !matches!(objective, FrontierObjective::ZeroRunway)
                && bucket.len() > beam_per_support
            {
                bucket.select_nth_unstable_by(beam_per_support, |left, right| {
                    compare_nodes(left, right, objective, seed)
                });
                bucket.truncate(beam_per_support);
            }
            if let Some(best) = bucket
                .iter()
                .min_by(|left, right| compare_nodes(left, right, objective, seed))
            {
                deepest_retained[best.positions.len()] = Some((depth, best.clone()));
            }
            frontier.append(bucket);
        }
        if frontier.is_empty() {
            break;
        }
        reached_depth = depth;
        peak_frontier = peak_frontier.max(frontier.len());
        let minimum = frontier
            .iter()
            .map(|node| node.positions.len() as u32)
            .min()
            .ok_or_else(|| "affine frontier lost its minimum support".to_string())?;
        if prior_minimum != Some(minimum) && records.len() < MAX_REPORTED_RECORDS {
            let witness = frontier
                .iter()
                .filter(|node| node.positions.len() as u32 == minimum)
                .min_by(|left, right| compare_positions(&left.positions, &right.positions))
                .ok_or_else(|| "affine frontier lost its record witness".to_string())?;
            records.push(record_for(
                witness,
                depth,
                &multiplier_big,
                &addend_big,
                &multiplier_powers,
                blocks.as_slice(),
                remaining_depth,
            ));
            prior_minimum = Some(minimum);
        }
    }

    let terminal = frontier.iter().min_by(|left, right| {
        left.positions
            .len()
            .cmp(&right.positions.len())
            .then_with(|| compare_positions(&left.positions, &right.positions))
    });
    let terminal_minimum_input_ones = terminal.map(|node| node.positions.len() as u32);
    let terminal_witness = terminal.map(|node| positions_value(&node.positions).to_string());
    let terminal_one_positions = terminal.map_or_else(Vec::new, |node| {
        node.positions.iter().copied().map(u32::from).collect()
    });
    let deepest_retained_by_support = deepest_retained
        .into_iter()
        .flatten()
        .map(|(depth, node)| {
            record_for(
                &node,
                depth,
                &multiplier_big,
                &addend_big,
                &multiplier_powers,
                blocks.as_slice(),
                max_depth - depth,
            )
        })
        .collect::<Vec<_>>();

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-frontier-profile/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&max_depth.to_le_bytes());
    hasher.update(&max_input_ones.to_le_bytes());
    hasher.update(&(beam_per_support as u64).to_le_bytes());
    hasher.update(objective_name.as_bytes());
    hasher.update(&seed.to_le_bytes());
    hasher.update(&reached_depth.to_le_bytes());
    hasher.update(&expanded_nodes.to_le_bytes());
    for record in &records {
        hasher.update(&record.depth.to_le_bytes());
        hasher.update(&record.minimum_input_ones.to_le_bytes());
        hasher.update(record.witness.as_bytes());
    }
    for record in &deepest_retained_by_support {
        hasher.update(&record.depth.to_le_bytes());
        hasher.update(&record.minimum_input_ones.to_le_bytes());
        hasher.update(record.witness.as_bytes());
    }
    if let Some(witness) = &terminal_witness {
        hasher.update(witness.as_bytes());
    }
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);

    Ok(AffineFrontierProfile {
        objective: objective_name.to_string(),
        max_depth,
        max_input_ones,
        beam_per_support,
        reached_depth,
        expanded_nodes,
        peak_frontier,
        records,
        deepest_retained_by_support,
        terminal_minimum_input_ones,
        terminal_witness,
        terminal_one_positions,
        signature: u64::from_le_bytes(prefix) & i64::MAX as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_zero_death_depth(
        node: &FrontierNode,
        multiplier: &BigUint,
        addend: &BigUint,
        multiplier_powers: &[BigUint],
        start_depth: u32,
        max_depth: u32,
    ) -> u32 {
        let mut probe = node.probe.clone();
        let mut odd_steps = node.odd_steps;
        for current_depth in start_depth..max_depth {
            if probe.bit(0) {
                probe = probe * multiplier + addend;
                odd_steps += 1;
            }
            probe >>= 1usize;
            let depth = current_depth + 1;
            if multiplier_powers[odd_steps as usize].bits() <= u64::from(depth) {
                return depth;
            }
        }
        max_depth + 1
    }

    #[test]
    fn zero_block_transform_matches_scalar_execution() {
        let multiplier = BigUint::from(3u8);
        let addend = BigUint::from(1u8);
        let mut powers = vec![BigUint::from(1u8)];
        for _ in 0..=256 {
            powers.push(powers.last().unwrap() * &multiplier);
        }
        let blocks = shared_zero_blocks(3, 1);
        for probe in 0u32..1_024 {
            let node = FrontierNode {
                positions: Positions::new(),
                probe: BigUint::from(probe),
                odd_steps: 80,
                zero_death_depth: 0,
                rank_runway: 0,
                rank_mix: 0,
            };
            assert_eq!(
                zero_death_depth(
                    &node,
                    &multiplier,
                    &addend,
                    &powers,
                    blocks.as_slice(),
                    100,
                    228,
                ),
                scalar_zero_death_depth(&node, &multiplier, &addend, &powers, 100, 228),
                "probe={probe}",
            );
        }
    }

    #[test]
    fn frontier_finds_only_exact_noncontracting_witnesses() {
        let profile = affine_frontier_profile(3, 1, 128, 8, 256, "zero_runway", 7).unwrap();
        assert_eq!(profile.reached_depth, 128);
        assert_eq!(
            profile.terminal_one_positions.len() as u32,
            profile.terminal_minimum_input_ones.unwrap()
        );
        assert!(profile.records.windows(2).all(|pair| {
            pair[0].minimum_input_ones < pair[1].minimum_input_ones && pair[0].depth < pair[1].depth
        }));
        let multiplier = BigUint::from(3u8);
        let addend = BigUint::from(1u8);
        let mut multiplier_powers = vec![BigUint::from(1u8)];
        for _ in 0..128 {
            multiplier_powers.push(multiplier_powers.last().unwrap() * &multiplier);
        }
        for record in &profile.deepest_retained_by_support {
            let residue = BigUint::parse_bytes(record.witness.as_bytes(), 10).unwrap();
            let mut node = FrontierNode {
                positions: Positions::new(),
                probe: BigUint::from(0u8),
                odd_steps: 0,
                zero_death_depth: 1,
                rank_runway: 0,
                rank_mix: 0,
            };
            for bit in 0..record.depth {
                node = step(
                    &node,
                    u32::from(residue.bit(u64::from(bit))),
                    bit,
                    &multiplier,
                    &addend,
                    &multiplier_powers,
                );
                assert!(multiplier_powers[node.odd_steps as usize].bits() > u64::from(bit + 1));
            }
            assert_eq!(node.positions.len() as u32, record.minimum_input_ones);
            assert_eq!(node.odd_steps, record.odd_steps);
            assert_eq!(
                node.positions
                    .iter()
                    .copied()
                    .map(u32::from)
                    .collect::<Vec<_>>(),
                record.one_positions
            );
        }
    }

    #[test]
    fn frontier_is_deterministic_for_every_objective() {
        for objective in [
            "zero_runway",
            "odd_headroom",
            "small_probe",
            "deterministic_mix",
        ] {
            let left = affine_frontier_profile(3, 1, 64, 7, 64, objective, 11).unwrap();
            let right = affine_frontier_profile(3, 1, 64, 7, 64, objective, 11).unwrap();
            assert_eq!(left, right);
        }
    }

    #[test]
    fn production_zero_runway_reconstructs_every_known_boundary() {
        let profile = affine_frontier_profile(3, 1, 1_024, 14, 256, "zero_runway", 0).unwrap();
        assert_eq!(profile.reached_depth, 1_024);
        assert_eq!(profile.records.len(), 12);
        assert!(profile.terminal_minimum_input_ones.unwrap() >= 11);
    }

    #[test]
    fn production_portfolio_preserves_all_six_exact_lanes() {
        let plans = [
            ("zero_runway", 0),
            ("odd_headroom", 0),
            ("small_probe", 0),
            ("deterministic_mix", 3),
            ("deterministic_mix", 17),
            ("deterministic_mix", 101),
        ];
        let profiles = plans
            .par_iter()
            .map(|(objective, seed)| {
                affine_frontier_profile(3, 1, 1_024, 14, 256, objective, *seed).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(profiles.len(), plans.len());
        assert_eq!(profiles[0].reached_depth, 1_024);
        assert!(profiles.iter().all(|profile| profile.reached_depth > 0));
        assert!(profiles.iter().all(|profile| profile.records.len() >= 11));
    }
}
