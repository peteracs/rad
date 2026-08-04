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
            peak: node.peak.max(source / 2),
            odd_steps: node.odd_steps,
            input_ones: node.input_ones + extension_bit as u32,
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
        peak: node.peak.max(probe),
        odd_steps: node.odd_steps + 1,
        input_ones: node.input_ones + extension_bit as u32,
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
fn residue_lane_analysis(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_index: u64,
    lane_count: u64,
) -> Result<(ResidueLaneProfile, Vec<ResidueNode>), String> {
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
        peak: 0,
        odd_steps: 0,
        input_ones: 0,
    };
    for bit_index in 0..lane_bits {
        let bit = (lane_index >> bit_index) & 1;
        node = step_node(node, bit, multiplier, addend)?;
        profile.expanded_nodes += 1;
        if prunable(node, verified_bound) {
            profile.pruned_classes = classes;
            profile.residue_sum = expected_sum;
            profile.prune_depth_histogram[(bit_index + 1) as usize] = classes;
            return Ok((profile, Vec::new()));
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
    for node in &frontier {
        profile.residue_sum += node.residue as u128;
        profile.survivor_odd_histogram[node.odd_steps as usize] += 1;
        if node.coefficient < node.denominator {
            profile.contracting_survivors += 1;
            let limit = threshold(*node).unwrap_or(0);
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
    Ok((profile, frontier))
}

pub(crate) fn residue_lane_profile(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_index: u64,
    lane_count: u64,
) -> Result<ResidueLaneProfile, String> {
    residue_lane_analysis(
        multiplier,
        addend,
        depth,
        verified_power,
        lane_index,
        lane_count,
    )
    .map(|(profile, _)| profile)
}

/// Analyze every low-bit lane concurrently and return results in canonical
/// lane-index order. Input validation happens before any worker is spawned.
pub(crate) fn residue_lane_profiles(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_count: u64,
) -> Result<Vec<ResidueLaneProfile>, String> {
    checked_inputs(multiplier, addend, depth, verified_power, 0, lane_count)?;
    std::thread::scope(|scope| {
        let handles = (0..lane_count)
            .map(|lane_index| {
                scope.spawn(move || {
                    residue_lane_profile(
                        multiplier,
                        addend,
                        depth,
                        verified_power,
                        lane_index,
                        lane_count,
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "affine residue worker panicked".to_string())?
            })
            .collect()
    })
}
