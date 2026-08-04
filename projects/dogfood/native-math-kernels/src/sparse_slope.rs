//! Exact sparse-input search for noncontracting affine-prefix coefficients.
//!
//! This is deliberately domain-neutral: it handles any positive odd affine
//! parity map.  Nodes store only state consulted by the search.  Sparse input
//! witnesses remain as inline bit positions and are materialized as big
//! integers only when a depth record is replaced.

use std::sync::atomic::{AtomicUsize, Ordering};

use num_bigint::BigUint;
use smallvec::SmallVec;

const MAX_DEPTH: u32 = 2048;
const MAX_SUPPORT: u32 = 64;
const MAX_LOCAL_ANCHORS: u64 = 350_000_000;
const MAX_LANE_COUNT: u32 = 256;
const LANE_SPLIT_WEIGHT: u32 = 6;
const PARALLEL_SPLIT_WEIGHT: u32 = 7;

type Positions = SmallVec<[u16; 16]>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SparseSlopeSummary {
    pub max_depth: u32,
    pub verified_power: u32,
    pub max_input_ones: u32,
    pub all_budgets_terminated: bool,
    pub termination_depth_by_budget: Vec<u32>,
    pub deepest_survival_by_weight: Vec<u32>,
    pub deepest_witness_by_weight: Vec<String>,
    pub deepest_witness_one_positions_by_weight: Vec<Vec<u32>>,
    pub anchors_by_weight: Vec<u64>,
    pub expanded_nodes: u64,
    pub signature: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SparseSlopeLaneSummary {
    pub max_depth: u32,
    pub max_input_ones: u32,
    pub lane_index: u32,
    pub lane_count: u32,
    pub split_weight: u32,
    pub seed_count: u64,
    pub assigned_seed_count: u64,
    pub deepest_survival_by_weight: Vec<u32>,
    pub deepest_witness_by_weight: Vec<String>,
    pub deepest_witness_one_positions_by_weight: Vec<Vec<u32>>,
    pub anchors_by_weight: Vec<u64>,
    pub expanded_nodes: u64,
    pub signature: u64,
}

#[derive(Clone)]
struct Node {
    positions: Positions,
    probe: BigUint,
    odd_steps: u32,
}

struct Search {
    multiplier: u64,
    addend: u64,
    multiplier_powers: Vec<BigUint>,
    max_depth: u32,
    max_support: u32,
    deepest: Vec<u32>,
    witnesses: Vec<Positions>,
    anchors: Vec<u64>,
    expanded: u64,
}

fn validate(multiplier: u64, addend: u64, max_depth: u32, max_support: u32) -> Result<(), String> {
    if multiplier < 3 || multiplier.is_multiple_of(2) {
        return Err("affine sparse-slope multiplier must be an odd integer >= 3".into());
    }
    if addend == 0 || addend.is_multiple_of(2) {
        return Err("affine sparse-slope addend must be a positive odd integer".into());
    }
    if max_depth == 0 || max_depth > MAX_DEPTH {
        return Err(format!(
            "affine sparse-slope depth must be between 1 and {MAX_DEPTH}"
        ));
    }
    if max_support > max_depth || max_support > MAX_SUPPORT {
        return Err(format!(
            "affine sparse-slope input-one budget must not exceed depth or {MAX_SUPPORT}"
        ));
    }
    Ok(())
}

fn root() -> Node {
    Node {
        positions: Positions::new(),
        probe: BigUint::from(0u8),
        odd_steps: 0,
    }
}

fn positions_less(left: &[u16], right: &[u16]) -> bool {
    left.iter().rev().cmp(right.iter().rev()).is_lt()
}

fn positions_value(positions: &[u16]) -> BigUint {
    let mut value = BigUint::from(0u8);
    for position in positions {
        value |= BigUint::from(1u8) << usize::from(*position);
    }
    value
}

impl Search {
    fn new(multiplier: u64, addend: u64, max_depth: u32, max_support: u32) -> Self {
        let mut multiplier_powers = Vec::with_capacity(max_depth as usize + 1);
        let mut power = BigUint::from(1u8);
        for _ in 0..=max_depth {
            multiplier_powers.push(power.clone());
            power *= multiplier;
        }
        Self {
            multiplier,
            addend,
            multiplier_powers,
            max_depth,
            max_support,
            deepest: vec![0; max_support as usize + 1],
            witnesses: vec![Positions::new(); max_support as usize + 1],
            anchors: vec![0; max_support as usize + 1],
            expanded: 0,
        }
    }

    fn step(&self, mut node: Node, extension_bit: bool, parent_depth: u32) -> Node {
        let coefficient = &self.multiplier_powers[node.odd_steps as usize];
        let source = if extension_bit {
            node.positions.push(parent_depth as u16);
            node.probe + coefficient
        } else {
            node.probe
        };
        if source.bit(0) {
            node.probe = (source * self.multiplier + self.addend) >> 1usize;
            node.odd_steps += 1;
        } else {
            node.probe = source >> 1usize;
        }
        node
    }

    fn prunable(&self, node: &Node, depth: u32) -> bool {
        self.multiplier_powers[node.odd_steps as usize].bits() <= u64::from(depth)
    }

    fn charge(&mut self, count: u64) -> Result<(), String> {
        self.expanded = self
            .expanded
            .checked_add(count)
            .ok_or_else(|| "affine sparse-slope expansion count overflow".to_string())?;
        Ok(())
    }

    fn record(&mut self, node: &Node, depth: u32) {
        let weight = node.positions.len();
        if depth > self.deepest[weight]
            || (depth == self.deepest[weight]
                && positions_less(&node.positions, &self.witnesses[weight]))
        {
            self.deepest[weight] = depth;
            self.witnesses[weight] = node.positions.clone();
        }
    }

    fn explore_exhausted(&mut self, anchor: Node, depth: u32) -> Result<(), String> {
        let positions = anchor.positions;
        let weight = positions.len();
        let mut value = anchor.probe;
        let mut odd_steps = anchor.odd_steps;
        self.record_positions(weight, &positions, depth);
        let mut current_depth = depth;
        while current_depth < self.max_depth {
            let odd = value.bit(0);
            let mut numerator = value;
            if odd {
                numerator *= self.multiplier;
                numerator += self.addend;
                odd_steps += 1;
            }
            let available_shift = numerator.trailing_zeros().unwrap_or(1).max(1);
            let remaining = u64::from(self.max_depth - current_depth);
            let jump = available_shift.min(remaining);
            let coefficient_bits = self.multiplier_powers[odd_steps as usize].bits();
            if coefficient_bits <= u64::from(current_depth) + jump {
                let mut low = 1u64;
                let mut high = jump;
                while low < high {
                    let middle = low + (high - low) / 2;
                    if coefficient_bits <= u64::from(current_depth) + middle {
                        high = middle;
                    } else {
                        low = middle + 1;
                    }
                }
                self.charge(low)?;
                self.record_positions(weight, &positions, current_depth + low as u32 - 1);
                break;
            }
            self.charge(jump)?;
            current_depth += jump as u32;
            value = numerator >> jump as usize;
            self.record_positions(weight, &positions, current_depth);
        }
        Ok(())
    }

    fn record_positions(&mut self, weight: usize, positions: &Positions, depth: u32) {
        if depth > self.deepest[weight]
            || (depth == self.deepest[weight] && positions_less(positions, &self.witnesses[weight]))
        {
            self.deepest[weight] = depth;
            self.witnesses[weight] = positions.clone();
        }
    }

    fn explore(&mut self, anchor: Node, depth: u32) -> Result<(), String> {
        let weight = anchor.positions.len();
        self.anchors[weight] = self.anchors[weight]
            .checked_add(1)
            .ok_or_else(|| "affine sparse-slope anchor count overflow".to_string())?;
        if self.anchors.iter().copied().sum::<u64>() > MAX_LOCAL_ANCHORS {
            return Err(format!(
                "affine sparse-slope local search exceeds {MAX_LOCAL_ANCHORS} anchors"
            ));
        }
        if weight == self.max_support as usize {
            return self.explore_exhausted(anchor, depth);
        }
        self.record(&anchor, depth);
        let mut zero_parent = anchor;
        for next_depth in (depth + 1)..=self.max_depth {
            let one_child = self.step(zero_parent.clone(), true, next_depth - 1);
            self.charge(1)?;
            if !self.prunable(&one_child, next_depth) {
                self.explore(one_child, next_depth)?;
            }
            let zero_child = self.step(zero_parent, false, next_depth - 1);
            self.charge(1)?;
            if self.prunable(&zero_child, next_depth) {
                break;
            }
            self.record(&zero_child, next_depth);
            zero_parent = zero_child;
        }
        Ok(())
    }

    fn collect(
        &mut self,
        anchor: Node,
        depth: u32,
        split_weight: usize,
        seeds: &mut Vec<(Node, u32)>,
    ) -> Result<(), String> {
        if anchor.positions.len() == split_weight {
            seeds.push((anchor, depth));
            return Ok(());
        }
        let weight = anchor.positions.len();
        self.anchors[weight] = self.anchors[weight]
            .checked_add(1)
            .ok_or_else(|| "affine sparse-slope anchor count overflow".to_string())?;
        self.record(&anchor, depth);
        let mut zero_parent = anchor;
        for next_depth in (depth + 1)..=self.max_depth {
            let one_child = self.step(zero_parent.clone(), true, next_depth - 1);
            self.charge(1)?;
            if !self.prunable(&one_child, next_depth) {
                self.collect(one_child, next_depth, split_weight, seeds)?;
            }
            let zero_child = self.step(zero_parent, false, next_depth - 1);
            self.charge(1)?;
            if self.prunable(&zero_child, next_depth) {
                break;
            }
            self.record(&zero_child, next_depth);
            zero_parent = zero_child;
        }
        Ok(())
    }

    fn merge(&mut self, other: Self) -> Result<(), String> {
        self.expanded = self
            .expanded
            .checked_add(other.expanded)
            .ok_or_else(|| "affine sparse-slope expansion count overflow".to_string())?;
        for weight in 0..self.anchors.len() {
            self.anchors[weight] = self.anchors[weight]
                .checked_add(other.anchors[weight])
                .ok_or_else(|| "affine sparse-slope anchor count overflow".to_string())?;
            let other_depth = other.deepest[weight];
            if other_depth > self.deepest[weight]
                || (other_depth == self.deepest[weight]
                    && positions_less(&other.witnesses[weight], &self.witnesses[weight]))
            {
                self.deepest[weight] = other_depth;
                self.witnesses[weight] = other.witnesses[weight].clone();
            }
        }
        Ok(())
    }
}

fn witnesses(search: &Search) -> (Vec<String>, Vec<Vec<u32>>) {
    let values = search
        .witnesses
        .iter()
        .map(|positions| positions_value(positions).to_string())
        .collect();
    let positions = search
        .witnesses
        .iter()
        .map(|positions| positions.iter().copied().map(u32::from).collect())
        .collect();
    (values, positions)
}

fn summary_signature(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    max_support: u32,
    search: &Search,
    witness_values: &[String],
) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-sparse-support-summary/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&max_depth.to_le_bytes());
    hasher.update(&0u32.to_le_bytes());
    hasher.update(&max_support.to_le_bytes());
    hasher.update(&search.expanded.to_le_bytes());
    for value in &search.deepest {
        hasher.update(&value.to_le_bytes());
    }
    for value in witness_values {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    for value in &search.anchors {
        hasher.update(&value.to_le_bytes());
    }
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(prefix) & i64::MAX as u64
}

pub(crate) fn summary(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    max_support: u32,
) -> Result<SparseSlopeSummary, String> {
    validate(multiplier, addend, max_depth, max_support)?;
    let mut search = Search::new(multiplier, addend, max_depth, max_support);
    let split_weight = max_support.min(PARALLEL_SPLIT_WEIGHT) as usize;
    let mut seeds = Vec::new();
    search.collect(root(), 0, split_weight, &mut seeds)?;
    let worker_count = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(seeds.len().max(1));
    let cursor = AtomicUsize::new(0);
    let workers = std::thread::scope(|scope| {
        let handles = (0..worker_count)
            .map(|_| {
                let seeds = &seeds;
                let cursor = &cursor;
                scope.spawn(move || {
                    let mut local = Search::new(multiplier, addend, max_depth, max_support);
                    loop {
                        let index = cursor.fetch_add(1, Ordering::Relaxed);
                        let Some((seed, depth)) = seeds.get(index) else {
                            break;
                        };
                        local.explore(seed.clone(), *depth)?;
                    }
                    Ok::<_, String>(local)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "affine sparse-slope worker panicked".to_string())?
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    for worker in workers {
        search.merge(worker)?;
    }
    let (witness_values, witness_positions) = witnesses(&search);
    let signature = summary_signature(
        multiplier,
        addend,
        max_depth,
        max_support,
        &search,
        &witness_values,
    );
    let all_budgets_terminated = search.deepest.iter().all(|depth| *depth < max_depth);
    Ok(SparseSlopeSummary {
        max_depth,
        verified_power: 0,
        max_input_ones: max_support,
        all_budgets_terminated,
        termination_depth_by_budget: search.deepest.iter().map(|depth| depth + 1).collect(),
        deepest_survival_by_weight: search.deepest,
        deepest_witness_by_weight: witness_values,
        deepest_witness_one_positions_by_weight: witness_positions,
        anchors_by_weight: search.anchors,
        expanded_nodes: search.expanded,
        signature,
    })
}

pub(crate) fn lane_summary(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    max_support: u32,
    lane_index: u32,
    lane_count: u32,
) -> Result<SparseSlopeLaneSummary, String> {
    validate(multiplier, addend, max_depth, max_support)?;
    if lane_count == 0 || lane_count > MAX_LANE_COUNT {
        return Err(format!(
            "affine sparse-slope lane count must be between 1 and {MAX_LANE_COUNT}"
        ));
    }
    if lane_index >= lane_count {
        return Err("affine sparse-slope lane index must be below lane count".into());
    }
    let split_weight = max_support.min(LANE_SPLIT_WEIGHT);
    let mut trunk = Search::new(multiplier, addend, max_depth, max_support);
    let mut seeds = Vec::new();
    trunk.collect(root(), 0, split_weight as usize, &mut seeds)?;
    let seed_count = seeds.len() as u64;
    let mut lane = if lane_index == 0 {
        trunk
    } else {
        Search::new(multiplier, addend, max_depth, max_support)
    };
    let mut assigned_seed_count = 0u64;
    for (index, (seed, depth)) in seeds.into_iter().enumerate() {
        if index % lane_count as usize == lane_index as usize {
            lane.explore(seed, depth)?;
            assigned_seed_count += 1;
        }
    }
    let (witness_values, witness_positions) = witnesses(&lane);
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-sparse-support-lane/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&max_depth.to_le_bytes());
    hasher.update(&max_support.to_le_bytes());
    hasher.update(&lane_index.to_le_bytes());
    hasher.update(&lane_count.to_le_bytes());
    hasher.update(&split_weight.to_le_bytes());
    hasher.update(&seed_count.to_le_bytes());
    hasher.update(&assigned_seed_count.to_le_bytes());
    hasher.update(&lane.expanded.to_le_bytes());
    for value in &lane.deepest {
        hasher.update(&value.to_le_bytes());
    }
    for value in &witness_values {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    for value in &lane.anchors {
        hasher.update(&value.to_le_bytes());
    }
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    Ok(SparseSlopeLaneSummary {
        max_depth,
        max_input_ones: max_support,
        lane_index,
        lane_count,
        split_weight,
        seed_count,
        assigned_seed_count,
        deepest_survival_by_weight: lane.deepest,
        deepest_witness_by_weight: witness_values,
        deepest_witness_one_positions_by_weight: witness_positions,
        anchors_by_weight: lane.anchors,
        expanded_nodes: lane.expanded,
        signature: u64::from_le_bytes(prefix) & i64::MAX as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_witness_order_matches_numeric_order() {
        assert!(positions_less(&[0, 3], &[1, 3]));
        assert!(positions_less(&[1, 3], &[0, 4]));
        assert!(!positions_less(&[0, 4], &[1, 3]));
    }

    #[test]
    fn compact_search_matches_the_independent_reference_kernel() {
        let compact = summary(3, 1, 400, 7).unwrap();
        let reference = crate::affine_parity::sparse_slope_support_summary(3, 1, 400, 7).unwrap();
        assert_eq!(
            compact.deepest_survival_by_weight,
            reference.deepest_survival_by_weight
        );
        assert_eq!(
            compact.deepest_witness_by_weight,
            reference.deepest_witness_by_weight
        );
        assert_eq!(compact.anchors_by_weight, reference.anchors_by_weight);
        assert_eq!(compact.expanded_nodes, reference.expanded_nodes);

        let compact_lane = lane_summary(3, 1, 400, 7, 2, 4).unwrap();
        let reference_lane =
            crate::affine_parity::sparse_slope_support_lane_summary(3, 1, 400, 7, 2, 4).unwrap();
        assert_eq!(
            compact_lane.deepest_survival_by_weight,
            reference_lane.deepest_survival_by_weight
        );
        assert_eq!(
            compact_lane.deepest_witness_by_weight,
            reference_lane.deepest_witness_by_weight
        );
        assert_eq!(
            compact_lane.anchors_by_weight,
            reference_lane.anchors_by_weight
        );
        assert_eq!(compact_lane.expanded_nodes, reference_lane.expanded_nodes);
    }
}
