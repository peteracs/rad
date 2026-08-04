//! Bounded, deterministic frontier exploration for odd affine parity maps.
//!
//! This module is deliberately heuristic: unlike the exhaustive certificate
//! kernel in `affine_parity`, a beam may discard a later-winning prefix.  Its
//! role is counterexample generation and invariant discovery.  All retained
//! states and reported witnesses are nevertheless evaluated with exact
//! integers.

use num_bigint::BigUint;
use std::cmp::Ordering;

const MAX_FRONTIER_DEPTH: u32 = 16_384;
const MAX_FRONTIER_SUPPORT: u32 = 128;
const MAX_BEAM_PER_SUPPORT: usize = 100_000;
const MAX_REPORTED_RECORDS: usize = 4_096;

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
    residue: BigUint,
    coefficient: BigUint,
    denominator: BigUint,
    probe: BigUint,
    input_ones: u32,
    odd_steps: u32,
    rank_runway: u32,
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

fn step(
    node: &FrontierNode,
    extension_bit: u32,
    multiplier: &BigUint,
    addend: &BigUint,
) -> FrontierNode {
    let residue = if extension_bit == 0 {
        node.residue.clone()
    } else {
        &node.residue + &node.denominator
    };
    let source = if extension_bit == 0 {
        node.probe.clone()
    } else {
        &node.probe + &node.coefficient
    };
    let denominator = &node.denominator << 1usize;
    if !source.bit(0) {
        return FrontierNode {
            residue,
            coefficient: node.coefficient.clone(),
            denominator,
            probe: source >> 1usize,
            input_ones: node.input_ones + extension_bit,
            odd_steps: node.odd_steps,
            rank_runway: 0,
        };
    }
    FrontierNode {
        residue,
        coefficient: &node.coefficient * multiplier,
        denominator,
        probe: (source * multiplier + addend) >> 1usize,
        input_ones: node.input_ones + extension_bit,
        odd_steps: node.odd_steps + 1,
        rank_runway: 0,
    }
}

fn zero_runway(node: &FrontierNode, multiplier: &BigUint, addend: &BigUint, limit: u32) -> u32 {
    let mut probe = node.probe.clone();
    let mut coefficient = node.coefficient.clone();
    let mut denominator = node.denominator.clone();
    let mut survived = 0;
    while survived < limit {
        denominator <<= 1usize;
        if probe.bit(0) {
            probe = (probe * multiplier + addend) >> 1usize;
            coefficient *= multiplier;
        } else {
            probe >>= 1usize;
        }
        if coefficient < denominator {
            break;
        }
        survived += 1;
    }
    survived
}

fn mixed_key(node: &FrontierNode, seed: u64) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-frontier-mix/v1\0");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&node.residue.to_bytes_le());
    hasher.update(&node.probe.to_bytes_le());
    hasher.update(&node.input_ones.to_le_bytes());
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(prefix)
}

fn compare_nodes(
    left: &FrontierNode,
    right: &FrontierNode,
    objective: FrontierObjective,
    seed: u64,
) -> Ordering {
    let primary = match objective {
        FrontierObjective::ZeroRunway => right.rank_runway.cmp(&left.rank_runway),
        FrontierObjective::OddHeadroom => right.odd_steps.cmp(&left.odd_steps),
        FrontierObjective::SmallProbe => left
            .probe
            .bits()
            .cmp(&right.probe.bits())
            .then_with(|| left.probe.cmp(&right.probe)),
        FrontierObjective::DeterministicMix => mixed_key(left, seed).cmp(&mixed_key(right, seed)),
    };
    primary
        .then_with(|| right.odd_steps.cmp(&left.odd_steps))
        .then_with(|| left.probe.bits().cmp(&right.probe.bits()))
        .then_with(|| left.residue.cmp(&right.residue))
}

fn one_positions(value: &BigUint) -> Vec<u32> {
    let mut positions = Vec::new();
    for bit in 0..value.bits() {
        if value.bit(bit) {
            positions.push(bit as u32);
        }
    }
    positions
}

fn record_for(
    node: &FrontierNode,
    depth: u32,
    multiplier: &BigUint,
    addend: &BigUint,
    remaining_depth: u32,
) -> FrontierRecord {
    FrontierRecord {
        depth,
        minimum_input_ones: node.input_ones,
        witness: node.residue.to_string(),
        one_positions: one_positions(&node.residue),
        odd_steps: node.odd_steps,
        coefficient_bits: node.coefficient.bits(),
        probe_bits: node.probe.bits(),
        zero_runway: zero_runway(node, multiplier, addend, remaining_depth),
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
    let mut frontier = vec![FrontierNode {
        residue: BigUint::from(0u8),
        coefficient: BigUint::from(1u8),
        denominator: BigUint::from(1u8),
        probe: BigUint::from(0u8),
        input_ones: 0,
        odd_steps: 0,
        rank_runway: 0,
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
                if parent.input_ones + extension_bit > max_input_ones {
                    continue;
                }
                let mut child = step(parent, extension_bit, &multiplier_big, &addend_big);
                expanded_nodes = expanded_nodes
                    .checked_add(1)
                    .ok_or_else(|| "affine frontier expansion count overflow".to_string())?;
                if child.coefficient >= child.denominator {
                    if matches!(objective, FrontierObjective::ZeroRunway) {
                        child.rank_runway = zero_runway(
                            &child,
                            &multiplier_big,
                            &addend_big,
                            (max_depth - depth).min(512),
                        );
                    }
                    buckets[child.input_ones as usize].push(child);
                }
            }
        }

        let remaining_depth = max_depth - depth;
        frontier.clear();
        for bucket in &mut buckets {
            if bucket.len() > beam_per_support {
                bucket.select_nth_unstable_by(beam_per_support, |left, right| {
                    compare_nodes(left, right, objective, seed)
                });
                bucket.truncate(beam_per_support);
            }
            if let Some(best) = bucket
                .iter()
                .min_by(|left, right| compare_nodes(left, right, objective, seed))
            {
                deepest_retained[best.input_ones as usize] = Some((depth, best.clone()));
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
            .map(|node| node.input_ones)
            .min()
            .ok_or_else(|| "affine frontier lost its minimum support".to_string())?;
        if prior_minimum != Some(minimum) && records.len() < MAX_REPORTED_RECORDS {
            let witness = frontier
                .iter()
                .filter(|node| node.input_ones == minimum)
                .min_by(|left, right| left.residue.cmp(&right.residue))
                .ok_or_else(|| "affine frontier lost its record witness".to_string())?;
            records.push(record_for(
                witness,
                depth,
                &multiplier_big,
                &addend_big,
                remaining_depth,
            ));
            prior_minimum = Some(minimum);
        }
    }

    let terminal = frontier.iter().min_by(|left, right| {
        left.input_ones
            .cmp(&right.input_ones)
            .then_with(|| left.residue.cmp(&right.residue))
    });
    let terminal_minimum_input_ones = terminal.map(|node| node.input_ones);
    let terminal_witness = terminal.map(|node| node.residue.to_string());
    let terminal_one_positions =
        terminal.map_or_else(Vec::new, |node| one_positions(&node.residue));
    let deepest_retained_by_support = deepest_retained
        .into_iter()
        .flatten()
        .map(|(depth, node)| {
            record_for(
                &node,
                depth,
                &multiplier_big,
                &addend_big,
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

    #[test]
    fn frontier_finds_only_exact_noncontracting_witnesses() {
        let profile = affine_frontier_profile(3, 1, 128, 8, 256, "zero_runway", 7).unwrap();
        assert_eq!(profile.reached_depth, 128);
        let witness =
            BigUint::parse_bytes(profile.terminal_witness.as_ref().unwrap().as_bytes(), 10)
                .unwrap();
        assert_eq!(
            one_positions(&witness).len() as u32,
            profile.terminal_minimum_input_ones.unwrap()
        );
        assert!(profile.records.windows(2).all(|pair| {
            pair[0].minimum_input_ones < pair[1].minimum_input_ones && pair[0].depth < pair[1].depth
        }));
        let multiplier = BigUint::from(3u8);
        let addend = BigUint::from(1u8);
        for record in &profile.deepest_retained_by_support {
            let residue = BigUint::parse_bytes(record.witness.as_bytes(), 10).unwrap();
            let mut node = FrontierNode {
                residue: BigUint::from(0u8),
                coefficient: BigUint::from(1u8),
                denominator: BigUint::from(1u8),
                probe: BigUint::from(0u8),
                input_ones: 0,
                odd_steps: 0,
                rank_runway: 0,
            };
            for bit in 0..record.depth {
                node = step(
                    &node,
                    u32::from(residue.bit(u64::from(bit))),
                    &multiplier,
                    &addend,
                );
                assert!(node.coefficient >= node.denominator);
            }
            assert_eq!(node.input_ones, record.minimum_input_ones);
            assert_eq!(node.odd_steps, record.odd_steps);
            assert_eq!(one_positions(&residue), record.one_positions);
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
}
