/// Exact bounded-support profile of the unpruned affine residue tree.
///
/// The support of an input prefix is the number of set bits in that prefix.
/// A fixed non-negative integer has finite support. Consequently, if the
/// unpruned tree restricted to support at most `max_input_ones` becomes empty,
/// no least counterexample with that support budget can exist. This is a
/// domain-neutral certificate for affine parity maps, not a search heuristic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SparseSupportProfile {
    pub max_depth: u32,
    pub verified_power: u32,
    pub max_input_ones: u32,
    pub terminated: bool,
    pub termination_depth: u32,
    pub expanded_nodes: u64,
    pub survivors_by_depth: Vec<u64>,
    pub minimum_input_ones_by_depth: Vec<i64>,
    pub minimum_weight_witness_by_depth: Vec<String>,
    pub deepest_survival_by_weight: Vec<u32>,
    pub deepest_witness_by_weight: Vec<String>,
    pub signature: u64,
}

#[derive(Clone, Debug)]
struct SparseSupportNode {
    residue: BigUint,
    coefficient: BigUint,
    offset: BigUint,
    denominator: BigUint,
    probe: BigUint,
    input_ones: u32,
    odd_steps: u32,
}

fn sparse_support_inputs(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    verified_power: u32,
    max_input_ones: u32,
) -> Result<(), String> {
    if multiplier < 3 || multiplier.is_multiple_of(2) {
        return Err("affine sparse-support multiplier must be an odd integer >= 3".into());
    }
    if addend == 0 || addend.is_multiple_of(2) {
        return Err("affine sparse-support addend must be a positive odd integer".into());
    }
    if max_depth == 0 || max_depth > MAX_SPARSE_DEPTH {
        return Err(format!(
            "affine sparse-support depth must be between 1 and {MAX_SPARSE_DEPTH}"
        ));
    }
    if verified_power > 4096 {
        return Err("affine sparse-support verified power must be at most 4096".into());
    }
    if max_input_ones > max_depth || max_input_ones > MAX_SPARSE_INPUT_ONES {
        return Err(format!(
            "affine sparse-support input-one budget must not exceed depth or {MAX_SPARSE_INPUT_ONES}"
        ));
    }
    Ok(())
}

fn sparse_step(
    node: SparseSupportNode,
    extension_bit: u32,
    multiplier: &BigUint,
    addend: &BigUint,
) -> SparseSupportNode {
    let residue = if extension_bit == 0 {
        node.residue
    } else {
        node.residue + &node.denominator
    };
    let source = if extension_bit == 0 {
        node.probe
    } else {
        node.probe + &node.coefficient
    };
    let denominator = &node.denominator << 1usize;
    if !source.bit(0) {
        return SparseSupportNode {
            residue,
            coefficient: node.coefficient,
            offset: node.offset,
            denominator,
            probe: source >> 1usize,
            input_ones: node.input_ones + extension_bit,
            odd_steps: node.odd_steps,
        };
    }
    SparseSupportNode {
        residue,
        coefficient: &node.coefficient * multiplier,
        offset: node.offset * multiplier + addend * &node.denominator,
        denominator,
        probe: (source * multiplier + addend) >> 1usize,
        input_ones: node.input_ones + extension_bit,
        odd_steps: node.odd_steps + 1,
    }
}

fn sparse_slope_step(
    node: SparseSupportNode,
    extension_bit: u32,
    depth: u32,
    multiplier_powers: &[BigUint],
    multiplier: &BigUint,
    addend: &BigUint,
) -> SparseSupportNode {
    let coefficient = &multiplier_powers[node.odd_steps as usize];
    let denominator = BigUint::from(1u8) << depth as usize;
    let residue = if extension_bit == 0 {
        node.residue
    } else {
        node.residue + &denominator
    };
    let source = if extension_bit == 0 {
        node.probe
    } else {
        node.probe + coefficient
    };
    if !source.bit(0) {
        return SparseSupportNode {
            residue,
            coefficient: BigUint::from(0u8),
            offset: BigUint::from(0u8),
            denominator: BigUint::from(0u8),
            probe: source >> 1usize,
            input_ones: node.input_ones + extension_bit,
            odd_steps: node.odd_steps,
        };
    }
    SparseSupportNode {
        residue,
        coefficient: BigUint::from(0u8),
        offset: BigUint::from(0u8),
        denominator: BigUint::from(0u8),
        probe: (source * multiplier + addend) >> 1usize,
        input_ones: node.input_ones + extension_bit,
        odd_steps: node.odd_steps + 1,
    }
}

fn sparse_prunable(node: &SparseSupportNode, verified_bound: &BigUint) -> bool {
    node.coefficient < node.denominator
        && node.offset < verified_bound * (&node.denominator - &node.coefficient)
}

pub(crate) fn sparse_support_profile(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    verified_power: u32,
    max_input_ones: u32,
) -> Result<SparseSupportProfile, String> {
    sparse_support_inputs(
        multiplier,
        addend,
        max_depth,
        verified_power,
        max_input_ones,
    )?;
    let multiplier_big = BigUint::from(multiplier);
    let addend_big = BigUint::from(addend);
    let verified_bound = BigUint::from(1u8) << verified_power as usize;
    let mut survivors_by_depth = vec![0u64; max_depth as usize + 1];
    let mut minimum_input_ones_by_depth = vec![-1i64; max_depth as usize + 1];
    let mut minimum_weight_witness_by_depth = vec![String::from("0"); max_depth as usize + 1];
    let mut deepest_survival_by_weight = vec![0u32; max_input_ones as usize + 1];
    let mut deepest_witnesses = vec![BigUint::from(0u8); max_input_ones as usize + 1];
    survivors_by_depth[0] = 1;
    minimum_input_ones_by_depth[0] = 0;
    let mut frontier = vec![SparseSupportNode {
        residue: BigUint::from(0u8),
        coefficient: BigUint::from(1u8),
        offset: BigUint::from(0u8),
        denominator: BigUint::from(1u8),
        probe: BigUint::from(0u8),
        input_ones: 0,
        odd_steps: 0,
    }];
    let mut expanded_nodes = 0u64;
    let mut terminated = false;
    let mut termination_depth = 0u32;

    for depth in 1..=max_depth {
        let mut next =
            Vec::with_capacity(frontier.len().saturating_mul(2).min(MAX_SPARSE_FRONTIER));
        for parent in frontier {
            for extension_bit in 0..=1 {
                if extension_bit == 1 && parent.input_ones == max_input_ones {
                    continue;
                }
                let child =
                    sparse_step(parent.clone(), extension_bit, &multiplier_big, &addend_big);
                expanded_nodes = expanded_nodes
                    .checked_add(1)
                    .ok_or_else(|| "affine sparse-support expansion count overflow".to_string())?;
                if !sparse_prunable(&child, &verified_bound) {
                    if next.len() == MAX_SPARSE_FRONTIER {
                        return Err(format!(
                            "affine sparse-support frontier exceeds {MAX_SPARSE_FRONTIER} nodes"
                        ));
                    }
                    next.push(child);
                }
            }
        }

        survivors_by_depth[depth as usize] = next.len() as u64;
        if next.is_empty() {
            terminated = true;
            termination_depth = depth;
            break;
        }

        let minimum_weight = next
            .iter()
            .map(|node| node.input_ones)
            .min()
            .ok_or_else(|| "affine sparse-support missing minimum weight".to_string())?;
        minimum_input_ones_by_depth[depth as usize] = minimum_weight as i64;
        minimum_weight_witness_by_depth[depth as usize] = next
            .iter()
            .filter(|node| node.input_ones == minimum_weight)
            .map(|node| &node.residue)
            .min()
            .ok_or_else(|| "affine sparse-support missing minimum witness".to_string())?
            .to_string();
        for node in &next {
            let weight = node.input_ones as usize;
            if depth > deepest_survival_by_weight[weight]
                || (depth == deepest_survival_by_weight[weight]
                    && node.residue < deepest_witnesses[weight])
            {
                deepest_survival_by_weight[weight] = depth;
                deepest_witnesses[weight] = node.residue.clone();
            }
        }
        frontier = next;
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-sparse-support/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&max_depth.to_le_bytes());
    hasher.update(&verified_power.to_le_bytes());
    hasher.update(&max_input_ones.to_le_bytes());
    hasher.update(&[u8::from(terminated)]);
    hasher.update(&termination_depth.to_le_bytes());
    hasher.update(&expanded_nodes.to_le_bytes());
    for value in &survivors_by_depth {
        hasher.update(&value.to_le_bytes());
    }
    for value in &minimum_input_ones_by_depth {
        hasher.update(&value.to_le_bytes());
    }
    for value in &minimum_weight_witness_by_depth {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    for value in &deepest_survival_by_weight {
        hasher.update(&value.to_le_bytes());
    }
    let deepest_witness_by_weight = deepest_witnesses
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    for value in &deepest_witness_by_weight {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);

    Ok(SparseSupportProfile {
        max_depth,
        verified_power,
        max_input_ones,
        terminated,
        termination_depth,
        expanded_nodes,
        survivors_by_depth,
        minimum_input_ones_by_depth,
        minimum_weight_witness_by_depth,
        deepest_survival_by_weight,
        deepest_witness_by_weight,
        signature: u64::from_le_bytes(prefix) & i64::MAX as u64,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SparseSupportSummary {
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
pub(crate) struct SparseSupportLaneSummary {
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

struct SparseSupportSearch {
    multiplier: BigUint,
    addend: BigUint,
    verified_bound: BigUint,
    max_depth: u32,
    max_input_ones: u32,
    deepest_survival_by_weight: Vec<u32>,
    deepest_witness_by_weight: Vec<BigUint>,
    anchors_by_weight: Vec<u64>,
    expanded_nodes: u64,
    prune_mode: SparsePruneMode,
    multiplier_powers: Vec<BigUint>,
}

#[derive(Clone, Copy)]
enum SparsePruneMode {
    DescentThreshold,
    PrefixSlope,
}

impl SparseSupportSearch {
    fn new(
        multiplier: u64,
        addend: u64,
        verified_power: u32,
        max_depth: u32,
        max_input_ones: u32,
        prune_mode: SparsePruneMode,
    ) -> Self {
        let multiplier = BigUint::from(multiplier);
        let mut multiplier_powers = Vec::with_capacity(max_depth as usize + 1);
        let mut power = BigUint::from(1u8);
        for _ in 0..=max_depth {
            multiplier_powers.push(power.clone());
            power *= &multiplier;
        }
        Self {
            multiplier,
            addend: BigUint::from(addend),
            verified_bound: BigUint::from(1u8) << verified_power as usize,
            max_depth,
            max_input_ones,
            deepest_survival_by_weight: vec![0; max_input_ones as usize + 1],
            deepest_witness_by_weight: vec![BigUint::from(0u8); max_input_ones as usize + 1],
            anchors_by_weight: vec![0; max_input_ones as usize + 1],
            expanded_nodes: 0,
            prune_mode,
            multiplier_powers,
        }
    }

    fn prunable(&self, node: &SparseSupportNode, depth: u32) -> bool {
        match self.prune_mode {
            SparsePruneMode::DescentThreshold => sparse_prunable(node, &self.verified_bound),
            SparsePruneMode::PrefixSlope => {
                self.multiplier_powers[node.odd_steps as usize].bits() <= u64::from(depth)
            }
        }
    }

    fn step(
        &self,
        node: SparseSupportNode,
        extension_bit: u32,
        parent_depth: u32,
    ) -> SparseSupportNode {
        match self.prune_mode {
            SparsePruneMode::DescentThreshold => {
                sparse_step(node, extension_bit, &self.multiplier, &self.addend)
            }
            SparsePruneMode::PrefixSlope => sparse_slope_step(
                node,
                extension_bit,
                parent_depth,
                &self.multiplier_powers,
                &self.multiplier,
                &self.addend,
            ),
        }
    }

    fn total_anchors(&self) -> u64 {
        self.anchors_by_weight.iter().copied().sum()
    }

    fn charge_expansion(&mut self) -> Result<(), String> {
        self.charge_expansions(1)
    }

    fn charge_expansions(&mut self, count: u64) -> Result<(), String> {
        self.expanded_nodes = self
            .expanded_nodes
            .checked_add(count)
            .ok_or_else(|| "affine sparse-support expansion count overflow".to_string())?;
        Ok(())
    }

    fn record(&mut self, node: &SparseSupportNode, depth: u32) {
        self.record_residue(node.input_ones as usize, &node.residue, depth);
    }

    fn record_residue(&mut self, weight: usize, residue: &BigUint, depth: u32) {
        if depth > self.deepest_survival_by_weight[weight]
            || (depth == self.deepest_survival_by_weight[weight]
                && residue < &self.deepest_witness_by_weight[weight])
        {
            self.deepest_survival_by_weight[weight] = depth;
            self.deepest_witness_by_weight[weight] = residue.clone();
        }
    }

    /// Once every permitted input one-bit has been placed, the cylinder has
    /// exactly one natural-number representative.  Follow that fixed value
    /// directly instead of retaining the stronger unknown-high-bit cylinder
    /// test.  This is both a tighter proof rule and the dominant performance
    /// optimization for high support budgets.
    fn explore_exhausted_leaf(
        &mut self,
        anchor: SparseSupportNode,
        depth: u32,
    ) -> Result<(), String> {
        if matches!(self.prune_mode, SparsePruneMode::PrefixSlope) {
            return self.explore_exhausted_slope_leaf(anchor, depth);
        }
        let residue = anchor.residue;
        if residue < self.verified_bound {
            return Ok(());
        }
        let mut value = anchor.probe;
        if value < residue {
            return Ok(());
        }
        let weight = anchor.input_ones as usize;
        self.record_residue(weight, &residue, depth);
        let mut current_depth = depth;
        while current_depth < self.max_depth {
            let numerator = if value.bit(0) {
                &value * &self.multiplier + &self.addend
            } else {
                value.clone()
            };
            let available_shift = numerator.trailing_zeros().unwrap_or(1).max(1);
            let remaining = u64::from(self.max_depth - current_depth);
            let jump = available_shift.min(remaining);
            let shifted = &numerator >> jump as usize;
            if shifted < residue {
                let mut low = 1u64;
                let mut high = jump;
                while low < high {
                    let middle = low + (high - low) / 2;
                    if (&numerator >> middle as usize) < residue {
                        high = middle;
                    } else {
                        low = middle + 1;
                    }
                }
                self.charge_expansions(low)?;
                let last_survival = current_depth + low as u32 - 1;
                self.record_residue(weight, &residue, last_survival);
                break;
            }
            self.charge_expansions(jump)?;
            current_depth += jump as u32;
            value = shifted;
            self.record_residue(weight, &residue, current_depth);
        }
        Ok(())
    }

    fn explore_exhausted_slope_leaf(
        &mut self,
        anchor: SparseSupportNode,
        depth: u32,
    ) -> Result<(), String> {
        let residue = anchor.residue;
        let weight = anchor.input_ones as usize;
        let mut value = anchor.probe;
        let mut odd_steps = anchor.odd_steps;
        self.record_residue(weight, &residue, depth);
        let mut current_depth = depth;
        while current_depth < self.max_depth {
            let odd = value.bit(0);
            let numerator = if odd {
                &value * &self.multiplier + &self.addend
            } else {
                value.clone()
            };
            if odd {
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
                self.charge_expansions(low)?;
                self.record_residue(weight, &residue, current_depth + low as u32 - 1);
                break;
            }
            self.charge_expansions(jump)?;
            current_depth += jump as u32;
            value = &numerator >> jump as usize;
            self.record_residue(weight, &residue, current_depth);
        }
        Ok(())
    }

    fn explore_anchor(&mut self, anchor: SparseSupportNode, depth: u32) -> Result<(), String> {
        let weight = anchor.input_ones as usize;
        self.anchors_by_weight[weight] = self.anchors_by_weight[weight]
            .checked_add(1)
            .ok_or_else(|| "affine sparse-support anchor count overflow".to_string())?;
        if self.total_anchors() > MAX_SPARSE_ANCHORS {
            return Err(format!(
                "affine sparse-support search exceeds {MAX_SPARSE_ANCHORS} anchors"
            ));
        }
        if anchor.input_ones == self.max_input_ones {
            return self.explore_exhausted_leaf(anchor, depth);
        }
        self.record(&anchor, depth);
        let mut zero_parent = anchor;
        for next_depth in (depth + 1)..=self.max_depth {
            if zero_parent.input_ones < self.max_input_ones {
                let one_child = self.step(zero_parent.clone(), 1, next_depth - 1);
                self.charge_expansion()?;
                if !self.prunable(&one_child, next_depth) {
                    self.explore_anchor(one_child, next_depth)?;
                }
            }
            let zero_child = self.step(zero_parent, 0, next_depth - 1);
            self.charge_expansion()?;
            if self.prunable(&zero_child, next_depth) {
                break;
            }
            self.record(&zero_child, next_depth);
            zero_parent = zero_child;
        }
        Ok(())
    }

    fn collect_split_anchors(
        &mut self,
        anchor: SparseSupportNode,
        depth: u32,
        split_weight: u32,
        seeds: &mut Vec<(SparseSupportNode, u32)>,
    ) -> Result<(), String> {
        if anchor.input_ones == split_weight {
            seeds.push((anchor, depth));
            return Ok(());
        }
        let weight = anchor.input_ones as usize;
        self.anchors_by_weight[weight] = self.anchors_by_weight[weight]
            .checked_add(1)
            .ok_or_else(|| "affine sparse-support anchor count overflow".to_string())?;
        self.record(&anchor, depth);
        let mut zero_parent = anchor;
        for next_depth in (depth + 1)..=self.max_depth {
            let one_child = self.step(zero_parent.clone(), 1, next_depth - 1);
            self.charge_expansion()?;
            if !self.prunable(&one_child, next_depth) {
                self.collect_split_anchors(one_child, next_depth, split_weight, seeds)?;
            }
            let zero_child = self.step(zero_parent, 0, next_depth - 1);
            self.charge_expansion()?;
            if self.prunable(&zero_child, next_depth) {
                break;
            }
            self.record(&zero_child, next_depth);
            zero_parent = zero_child;
        }
        Ok(())
    }

    fn merge(&mut self, other: Self) -> Result<(), String> {
        self.expanded_nodes = self
            .expanded_nodes
            .checked_add(other.expanded_nodes)
            .ok_or_else(|| "affine sparse-support expansion count overflow".to_string())?;
        for weight in 0..self.anchors_by_weight.len() {
            self.anchors_by_weight[weight] = self.anchors_by_weight[weight]
                .checked_add(other.anchors_by_weight[weight])
                .ok_or_else(|| "affine sparse-support anchor count overflow".to_string())?;
            let other_depth = other.deepest_survival_by_weight[weight];
            if other_depth > self.deepest_survival_by_weight[weight]
                || (other_depth == self.deepest_survival_by_weight[weight]
                    && other.deepest_witness_by_weight[weight]
                        < self.deepest_witness_by_weight[weight])
            {
                self.deepest_survival_by_weight[weight] = other_depth;
                self.deepest_witness_by_weight[weight] =
                    other.deepest_witness_by_weight[weight].clone();
            }
        }
        if self.total_anchors() > MAX_SPARSE_ANCHORS {
            return Err(format!(
                "affine sparse-support search exceeds {MAX_SPARSE_ANCHORS} anchors"
            ));
        }
        Ok(())
    }
}

pub(crate) fn sparse_support_summary(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    verified_power: u32,
    max_input_ones: u32,
) -> Result<SparseSupportSummary, String> {
    sparse_support_summary_with_mode(
        multiplier,
        addend,
        max_depth,
        verified_power,
        max_input_ones,
        SparsePruneMode::DescentThreshold,
    )
}

pub(crate) fn sparse_slope_support_summary(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    max_input_ones: u32,
) -> Result<SparseSupportSummary, String> {
    sparse_support_summary_with_mode(
        multiplier,
        addend,
        max_depth,
        0,
        max_input_ones,
        SparsePruneMode::PrefixSlope,
    )
}

pub(crate) fn sparse_slope_support_lane_summary(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    max_input_ones: u32,
    lane_index: u32,
    lane_count: u32,
) -> Result<SparseSupportLaneSummary, String> {
    sparse_support_inputs(multiplier, addend, max_depth, 0, max_input_ones)?;
    if lane_count == 0 || lane_count > 256 {
        return Err("affine sparse-support lane count must be between 1 and 256".into());
    }
    if lane_index >= lane_count {
        return Err("affine sparse-support lane index must be below lane count".into());
    }
    let root = SparseSupportNode {
        residue: BigUint::from(0u8),
        coefficient: BigUint::from(1u8),
        offset: BigUint::from(0u8),
        denominator: BigUint::from(1u8),
        probe: BigUint::from(0u8),
        input_ones: 0,
        odd_steps: 0,
    };
    let split_weight = max_input_ones.min(6);
    let mut trunk = SparseSupportSearch::new(
        multiplier,
        addend,
        0,
        max_depth,
        max_input_ones,
        SparsePruneMode::PrefixSlope,
    );
    let mut seeds = Vec::new();
    if split_weight == 0 {
        seeds.push((root, 0));
    } else {
        trunk.collect_split_anchors(root, 0, split_weight, &mut seeds)?;
    }
    let seed_count = seeds.len() as u64;
    let mut lane = if lane_index == 0 {
        trunk
    } else {
        SparseSupportSearch::new(
            multiplier,
            addend,
            0,
            max_depth,
            max_input_ones,
            SparsePruneMode::PrefixSlope,
        )
    };
    let mut assigned_seed_count = 0u64;
    for (index, (seed, depth)) in seeds.into_iter().enumerate() {
        if index % lane_count as usize == lane_index as usize {
            lane.explore_anchor(seed, depth)?;
            assigned_seed_count += 1;
        }
    }

    let deepest_witness_by_weight = lane
        .deepest_witness_by_weight
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let deepest_witness_one_positions_by_weight = lane
        .deepest_witness_by_weight
        .iter()
        .map(|value| {
            let mut positions = Vec::new();
            for bit in 0..value.bits() {
                if value.bit(bit) {
                    positions.push(bit as u32);
                }
            }
            positions
        })
        .collect::<Vec<_>>();

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-sparse-support-lane/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&max_depth.to_le_bytes());
    hasher.update(&max_input_ones.to_le_bytes());
    hasher.update(&lane_index.to_le_bytes());
    hasher.update(&lane_count.to_le_bytes());
    hasher.update(&split_weight.to_le_bytes());
    hasher.update(&seed_count.to_le_bytes());
    hasher.update(&assigned_seed_count.to_le_bytes());
    hasher.update(&lane.expanded_nodes.to_le_bytes());
    for value in &lane.deepest_survival_by_weight {
        hasher.update(&value.to_le_bytes());
    }
    for value in &deepest_witness_by_weight {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    for value in &lane.anchors_by_weight {
        hasher.update(&value.to_le_bytes());
    }
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);

    Ok(SparseSupportLaneSummary {
        max_depth,
        max_input_ones,
        lane_index,
        lane_count,
        split_weight,
        seed_count,
        assigned_seed_count,
        deepest_survival_by_weight: lane.deepest_survival_by_weight,
        deepest_witness_by_weight,
        deepest_witness_one_positions_by_weight,
        anchors_by_weight: lane.anchors_by_weight,
        expanded_nodes: lane.expanded_nodes,
        signature: u64::from_le_bytes(prefix) & i64::MAX as u64,
    })
}

fn sparse_support_summary_with_mode(
    multiplier: u64,
    addend: u64,
    max_depth: u32,
    verified_power: u32,
    max_input_ones: u32,
    prune_mode: SparsePruneMode,
) -> Result<SparseSupportSummary, String> {
    sparse_support_inputs(
        multiplier,
        addend,
        max_depth,
        verified_power,
        max_input_ones,
    )?;
    let root = SparseSupportNode {
        residue: BigUint::from(0u8),
        coefficient: BigUint::from(1u8),
        offset: BigUint::from(0u8),
        denominator: BigUint::from(1u8),
        probe: BigUint::from(0u8),
        input_ones: 0,
        odd_steps: 0,
    };
    let mut search = SparseSupportSearch::new(
        multiplier,
        addend,
        verified_power,
        max_depth,
        max_input_ones,
        prune_mode,
    );
    let split_weight = max_input_ones.min(6);
    if split_weight == max_input_ones {
        search.explore_anchor(root, 0)?;
    } else {
        let mut seeds = Vec::new();
        search.collect_split_anchors(root, 0, split_weight, &mut seeds)?;
        let worker_count = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(seeds.len().max(1));
        let mut buckets = (0..worker_count).map(|_| Vec::new()).collect::<Vec<_>>();
        for (index, seed) in seeds.into_iter().enumerate() {
            buckets[index % worker_count].push(seed);
        }
        let workers = std::thread::scope(|scope| {
            let handles = buckets
                .into_iter()
                .map(|bucket| {
                    scope.spawn(move || {
                        let mut local = SparseSupportSearch::new(
                            multiplier,
                            addend,
                            verified_power,
                            max_depth,
                            max_input_ones,
                            prune_mode,
                        );
                        for (seed, depth) in bucket {
                            local.explore_anchor(seed, depth)?;
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
                        .map_err(|_| "affine sparse-support worker panicked".to_string())?
                })
                .collect::<Result<Vec<_>, _>>()
        })?;
        for worker in workers {
            search.merge(worker)?;
        }
    }

    let mut cumulative_deepest = 0u32;
    let termination_depth_by_budget = search
        .deepest_survival_by_weight
        .iter()
        .map(|depth| {
            cumulative_deepest = cumulative_deepest.max(*depth);
            if cumulative_deepest < max_depth {
                cumulative_deepest + 1
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let all_budgets_terminated = termination_depth_by_budget.iter().all(|depth| *depth > 0);
    let deepest_witness_one_positions_by_weight = search
        .deepest_witness_by_weight
        .iter()
        .map(|value| {
            (0..value.bits())
                .filter(|position| value.bit(*position))
                .map(|position| position as u32)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let deepest_witness_by_weight = search
        .deepest_witness_by_weight
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-sparse-support-summary/v1\0");
    hasher.update(&[match prune_mode {
        SparsePruneMode::DescentThreshold => 0,
        SparsePruneMode::PrefixSlope => 1,
    }]);
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&max_depth.to_le_bytes());
    hasher.update(&verified_power.to_le_bytes());
    hasher.update(&max_input_ones.to_le_bytes());
    hasher.update(&[u8::from(all_budgets_terminated)]);
    for value in &termination_depth_by_budget {
        hasher.update(&value.to_le_bytes());
    }
    for value in &search.deepest_survival_by_weight {
        hasher.update(&value.to_le_bytes());
    }
    for value in &deepest_witness_by_weight {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    for positions in &deepest_witness_one_positions_by_weight {
        hasher.update(&(positions.len() as u64).to_le_bytes());
        for position in positions {
            hasher.update(&position.to_le_bytes());
        }
    }
    for value in &search.anchors_by_weight {
        hasher.update(&value.to_le_bytes());
    }
    hasher.update(&search.expanded_nodes.to_le_bytes());
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);

    Ok(SparseSupportSummary {
        max_depth,
        verified_power,
        max_input_ones,
        all_budgets_terminated,
        termination_depth_by_budget,
        deepest_survival_by_weight: search.deepest_survival_by_weight,
        deepest_witness_by_weight,
        deepest_witness_one_positions_by_weight,
        anchors_by_weight: search.anchors_by_weight,
        expanded_nodes: search.expanded_nodes,
        signature: u64::from_le_bytes(prefix) & i64::MAX as u64,
    })
}
