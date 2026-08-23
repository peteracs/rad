#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NaturalTailLaneProfile {
    pub depth: u32,
    pub lane_index: u64,
    pub lane_count: u64,
    pub survivor_classes: u64,
    pub coefficient_stops: u64,
    pub descents: u64,
    pub unresolved: u64,
    pub max_coefficient_stop_step: u32,
    pub max_coefficient_stop_residue: u64,
    pub max_descent_step: u32,
    pub max_descent_residue: u64,
    pub max_additive_delay: u32,
    pub max_additive_delay_residue: u64,
    pub max_peak: u128,
    pub max_peak_residue: u64,
    pub coefficient_stop_histogram: Vec<u64>,
    pub descent_histogram: Vec<u64>,
    pub signature: u64,
}

fn empty_natural_tail_profile(
    depth: u32,
    lane_index: u64,
    lane_count: u64,
    max_steps: u32,
) -> NaturalTailLaneProfile {
    NaturalTailLaneProfile {
        depth,
        lane_index,
        lane_count,
        survivor_classes: 0,
        coefficient_stops: 0,
        descents: 0,
        unresolved: 0,
        max_coefficient_stop_step: 0,
        max_coefficient_stop_residue: 0,
        max_descent_step: 0,
        max_descent_residue: 0,
        max_additive_delay: 0,
        max_additive_delay_residue: 0,
        max_peak: 0,
        max_peak_residue: 0,
        coefficient_stop_histogram: vec![0; max_steps as usize + 1],
        descent_histogram: vec![0; max_steps as usize + 1],
        signature: 0,
    }
}

fn minimum_noncontracting_odd_steps(multiplier: u64, max_steps: u32) -> Vec<u32> {
    let multiplier = BigUint::from(multiplier);
    let mut multiplier_power = BigUint::from(1u8);
    let mut two_power = BigUint::from(1u8);
    let mut odd_steps = 0u32;
    let mut minimums = Vec::with_capacity(max_steps as usize + 1);
    minimums.push(0);
    for _step in 1..=max_steps {
        two_power <<= 1usize;
        while multiplier_power < two_power {
            multiplier_power *= &multiplier;
            odd_steps += 1;
        }
        minimums.push(odd_steps);
    }
    minimums
}

fn choose_record(current_value: u32, current_residue: u64, value: u32, residue: u64) -> bool {
    value > current_value || (value == current_value && residue < current_residue)
}

fn trace_natural_tail_u64(
    node: ResidueNode,
    multiplier: u64,
    addend: u64,
    depth: u32,
    max_steps: u32,
    minimum_odds: &[u32],
) -> Option<(Option<u32>, Option<u32>, u128)> {
    let start = node.residue;
    let mut value = u64::try_from(node.probe).ok()?;
    let mut peak = u64::try_from((node.residue as u128).max(node.peak)).ok()?;
    let mut odd_steps = node.odd_steps;
    let mut coefficient_stop = (odd_steps < minimum_odds[depth as usize]).then_some(depth);
    let mut descent = (value < start).then_some(depth);

    for step in (depth + 1)..=max_steps {
        if value & 1 == 0 {
            value /= 2;
        } else {
            value = value.checked_mul(multiplier)?.checked_add(addend)? / 2;
            odd_steps += 1;
        }
        peak = peak.max(value);
        if coefficient_stop.is_none() && odd_steps < minimum_odds[step as usize] {
            coefficient_stop = Some(step);
        }
        if descent.is_none() && value < start {
            descent = Some(step);
        }
        if coefficient_stop.is_some() && descent.is_some() {
            break;
        }
    }
    Some((coefficient_stop, descent, peak as u128))
}

fn trace_natural_tail_u128(
    node: ResidueNode,
    multiplier: u64,
    addend: u64,
    depth: u32,
    max_steps: u32,
    minimum_odds: &[u32],
) -> Result<(Option<u32>, Option<u32>, u128), String> {
    let start = node.residue as u128;
    let mut value = node.probe;
    let mut peak = start.max(node.peak);
    let mut odd_steps = node.odd_steps;
    let mut coefficient_stop = (odd_steps < minimum_odds[depth as usize]).then_some(depth);
    let mut descent = (value < start).then_some(depth);

    for step in (depth + 1)..=max_steps {
        if value & 1 == 0 {
            value /= 2;
        } else {
            value = value
                .checked_mul(multiplier as u128)
                .and_then(|next| next.checked_add(addend as u128))
                .ok_or_else(|| {
                    format!(
                        "affine natural-tail orbit overflow for residue {}",
                        node.residue
                    )
                })?
                / 2;
            odd_steps += 1;
        }
        peak = peak.max(value);
        if coefficient_stop.is_none() && odd_steps < minimum_odds[step as usize] {
            coefficient_stop = Some(step);
        }
        if descent.is_none() && value < start {
            descent = Some(step);
        }
        if coefficient_stop.is_some() && descent.is_some() {
            break;
        }
    }
    Ok((coefficient_stop, descent, peak))
}

fn record_natural_tail_node(
    profile: &mut NaturalTailLaneProfile,
    node: ResidueNode,
    multiplier: u64,
    addend: u64,
    depth: u32,
    max_steps: u32,
    minimum_odds: &[u32],
) -> Result<(), String> {
    if node.residue == 0 {
        return Err("affine natural-tail survivor cannot be zero".into());
    }
    let (coefficient_stop, descent, peak) = trace_natural_tail_u64(
        node,
        multiplier,
        addend,
        depth,
        max_steps,
        minimum_odds,
    )
    .map(Ok)
    .unwrap_or_else(|| {
        trace_natural_tail_u128(
            node,
            multiplier,
            addend,
            depth,
            max_steps,
            minimum_odds,
        )
    })?;

    profile.survivor_classes += 1;
    if let Some(step) = coefficient_stop {
        profile.coefficient_stops += 1;
        profile.coefficient_stop_histogram[step as usize] += 1;
        if choose_record(
            profile.max_coefficient_stop_step,
            profile.max_coefficient_stop_residue,
            step,
            node.residue,
        ) {
            profile.max_coefficient_stop_step = step;
            profile.max_coefficient_stop_residue = node.residue;
        }
    }
    if let Some(step) = descent {
        profile.descents += 1;
        profile.descent_histogram[step as usize] += 1;
        if choose_record(
            profile.max_descent_step,
            profile.max_descent_residue,
            step,
            node.residue,
        ) {
            profile.max_descent_step = step;
            profile.max_descent_residue = node.residue;
        }
    }
    match (coefficient_stop, descent) {
        (Some(coefficient_step), Some(descent_step)) => {
            let delay = descent_step - coefficient_step;
            if choose_record(
                profile.max_additive_delay,
                profile.max_additive_delay_residue,
                delay,
                node.residue,
            ) {
                profile.max_additive_delay = delay;
                profile.max_additive_delay_residue = node.residue;
            }
        }
        _ => profile.unresolved += 1,
    }
    if peak > profile.max_peak
        || (peak == profile.max_peak && node.residue < profile.max_peak_residue)
    {
        profile.max_peak = peak;
        profile.max_peak_residue = node.residue;
    }
    Ok(())
}

/// Continue each surviving residue with zero high input bits.
///
/// An ordinary positive integer has a finite binary expansion, so its residue
/// representative eventually stops changing as the input modulus grows. This
/// profile measures the exact cost of that natural-number boundary condition:
/// when the affine coefficient first becomes contracting, and when the actual
/// orbit subsequently falls below its fixed starting value. Generic 2-adic
/// branches need not have this zero-tail property.
fn validate_natural_tail_steps(depth: u32, max_steps: u32) -> Result<(), String> {
    if max_steps <= depth || max_steps > 4096 {
        return Err(
            "affine natural-tail max_steps must be greater than depth and at most 4096"
                .to_string(),
        );
    }
    Ok(())
}

fn finalize_natural_tail_profile(
    profile: &mut NaturalTailLaneProfile,
    multiplier: u64,
    addend: u64,
    verified_power: u32,
    max_steps: u32,
) -> Result<(), String> {
    if profile.descents + profile.unresolved != profile.survivor_classes
        || profile.coefficient_stops < profile.descents
        || profile.coefficient_stops > profile.survivor_classes
    {
        return Err("affine natural-tail partition mismatch".into());
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"rad-affine-natural-tail/v1\0");
    hasher.update(&multiplier.to_le_bytes());
    hasher.update(&addend.to_le_bytes());
    hasher.update(&profile.depth.to_le_bytes());
    hasher.update(&verified_power.to_le_bytes());
    hasher.update(&profile.lane_index.to_le_bytes());
    hasher.update(&profile.lane_count.to_le_bytes());
    hasher.update(&max_steps.to_le_bytes());
    hasher.update(&profile.survivor_classes.to_le_bytes());
    hasher.update(&profile.coefficient_stops.to_le_bytes());
    hasher.update(&profile.descents.to_le_bytes());
    hasher.update(&profile.unresolved.to_le_bytes());
    for (step, count) in profile.coefficient_stop_histogram.iter().enumerate() {
        hasher.update(&(step as u32).to_le_bytes());
        hasher.update(&count.to_le_bytes());
    }
    for (step, count) in profile.descent_histogram.iter().enumerate() {
        hasher.update(&(step as u32).to_le_bytes());
        hasher.update(&count.to_le_bytes());
    }
    hasher.update(&profile.max_coefficient_stop_step.to_le_bytes());
    hasher.update(&profile.max_coefficient_stop_residue.to_le_bytes());
    hasher.update(&profile.max_descent_step.to_le_bytes());
    hasher.update(&profile.max_descent_residue.to_le_bytes());
    hasher.update(&profile.max_additive_delay.to_le_bytes());
    hasher.update(&profile.max_additive_delay_residue.to_le_bytes());
    hasher.update(&profile.max_peak.to_le_bytes());
    hasher.update(&profile.max_peak_residue.to_le_bytes());
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    profile.signature = u64::from_le_bytes(prefix) & i64::MAX as u64;
    Ok(())
}

fn merge_natural_tail_profile(
    target: &mut NaturalTailLaneProfile,
    source: NaturalTailLaneProfile,
) {
    target.survivor_classes += source.survivor_classes;
    target.coefficient_stops += source.coefficient_stops;
    target.descents += source.descents;
    target.unresolved += source.unresolved;

    if choose_record(
        target.max_coefficient_stop_step,
        target.max_coefficient_stop_residue,
        source.max_coefficient_stop_step,
        source.max_coefficient_stop_residue,
    ) {
        target.max_coefficient_stop_step = source.max_coefficient_stop_step;
        target.max_coefficient_stop_residue = source.max_coefficient_stop_residue;
    }
    if choose_record(
        target.max_descent_step,
        target.max_descent_residue,
        source.max_descent_step,
        source.max_descent_residue,
    ) {
        target.max_descent_step = source.max_descent_step;
        target.max_descent_residue = source.max_descent_residue;
    }
    if choose_record(
        target.max_additive_delay,
        target.max_additive_delay_residue,
        source.max_additive_delay,
        source.max_additive_delay_residue,
    ) {
        target.max_additive_delay = source.max_additive_delay;
        target.max_additive_delay_residue = source.max_additive_delay_residue;
    }
    if source.max_peak > target.max_peak
        || (source.max_peak == target.max_peak
            && source.max_peak_residue < target.max_peak_residue)
    {
        target.max_peak = source.max_peak;
        target.max_peak_residue = source.max_peak_residue;
    }
    for (target, source) in target
        .coefficient_stop_histogram
        .iter_mut()
        .zip(source.coefficient_stop_histogram)
    {
        *target += source;
    }
    for (target, source) in target
        .descent_histogram
        .iter_mut()
        .zip(source.descent_histogram)
    {
        *target += source;
    }
}

#[allow(clippy::too_many_arguments)]
fn record_natural_tail_subtree(
    profile: &mut NaturalTailLaneProfile,
    node: ResidueNode,
    current_depth: u32,
    multiplier: u64,
    addend: u64,
    depth: u32,
    max_steps: u32,
    minimum_odds: &[u32],
    verified_bound: u128,
) -> Result<(), String> {
    if current_depth == depth {
        return record_natural_tail_node(
            profile,
            node,
            multiplier,
            addend,
            depth,
            max_steps,
            minimum_odds,
        );
    }

    for extension_bit in 0..=1 {
        let child = step_node(
            node,
            extension_bit,
            multiplier as u128,
            addend as u128,
        )?;
        if !prunable(child, verified_bound) {
            record_natural_tail_subtree(
                profile,
                child,
                current_depth + 1,
                multiplier,
                addend,
                depth,
                max_steps,
                minimum_odds,
                verified_bound,
            )?;
        }
    }
    Ok(())
}

fn natural_tail_lane_profile_with_minimums(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_index: u64,
    lane_count: u64,
    max_steps: u32,
    minimum_odds: &[u32],
) -> Result<NaturalTailLaneProfile, String> {
    let lane_bits = checked_inputs(
        multiplier,
        addend,
        depth,
        verified_power,
        lane_index,
        lane_count,
    )?;
    let mut profile = empty_natural_tail_profile(depth, lane_index, lane_count, max_steps);

    let multiplier_u128 = multiplier as u128;
    let addend_u128 = addend as u128;
    let verified_bound = 1u128 << verified_power;
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
    let mut prefix_pruned = false;
    for bit_index in 0..lane_bits {
        node = step_node(
            node,
            (lane_index >> bit_index) & 1,
            multiplier_u128,
            addend_u128,
        )?;
        if prunable(node, verified_bound) {
            prefix_pruned = true;
            break;
        }
    }

    if prefix_pruned {
        // The empty profile is still finalized below so its deterministic
        // signature remains identical to the generic analysis path.
    } else if lane_bits == depth {
        record_natural_tail_node(
            &mut profile,
            node,
            multiplier,
            addend,
            depth,
            max_steps,
            minimum_odds,
        )?;
    } else {
        let mut frontier = vec![node];
        for current_depth in lane_bits..depth {
            let next_depth = current_depth + 1;
            let final_depth = next_depth == depth;
            let mut next = Vec::with_capacity(if final_depth {
                0
            } else {
                frontier.len().saturating_mul(2)
            });
            for parent in frontier {
                for extension_bit in 0..=1 {
                    let child =
                        step_node(parent, extension_bit, multiplier_u128, addend_u128)?;
                    if prunable(child, verified_bound) {
                        continue;
                    }
                    if final_depth {
                        record_natural_tail_node(
                            &mut profile,
                            child,
                            multiplier,
                            addend,
                            depth,
                            max_steps,
                            minimum_odds,
                        )?;
                    } else {
                        next.push(child);
                    }
                }
            }
            frontier = next;
        }
    }

    finalize_natural_tail_profile(
        &mut profile,
        multiplier,
        addend,
        verified_power,
        max_steps,
    )?;
    Ok(profile)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn natural_tail_lane_profile(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_index: u64,
    lane_count: u64,
    max_steps: u32,
) -> Result<NaturalTailLaneProfile, String> {
    validate_natural_tail_steps(depth, max_steps)?;
    let minimum_odds = minimum_noncontracting_odd_steps(multiplier, max_steps);
    natural_tail_lane_profile_with_minimums(
        multiplier,
        addend,
        depth,
        verified_power,
        lane_index,
        lane_count,
        max_steps,
        &minimum_odds,
    )
}

pub(crate) fn natural_tail_lane_profiles(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_count: u64,
    max_steps: u32,
) -> Result<Vec<NaturalTailLaneProfile>, String> {
    let lane_bits = checked_inputs(multiplier, addend, depth, verified_power, 0, lane_count)?;
    validate_natural_tail_steps(depth, max_steps)?;
    let minimum_odds = minimum_noncontracting_odd_steps(multiplier, max_steps);
    let worker_count = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(16);
    let target_tasks = worker_count.saturating_mul(256).next_power_of_two();
    let split_depth = lane_bits.max(target_tasks.ilog2()).min(depth);
    let task_count = 1usize << split_depth;
    let next_task = std::sync::atomic::AtomicUsize::new(0);
    let verified_bound = 1u128 << verified_power;

    let worker_profiles = std::thread::scope(|scope| {
        let handles = (0..worker_count)
            .map(|_| {
                let next_task = &next_task;
                let minimum_odds = &minimum_odds;
                scope.spawn(move || {
                    let mut profiles = (0..lane_count)
                        .map(|lane_index| {
                            empty_natural_tail_profile(depth, lane_index, lane_count, max_steps)
                        })
                        .collect::<Vec<_>>();
                    loop {
                        let prefix = next_task.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if prefix >= task_count {
                            break;
                        }

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
                        let mut prefix_pruned = false;
                        for bit_index in 0..split_depth {
                            node = step_node(
                                node,
                                ((prefix >> bit_index) & 1) as u64,
                                multiplier as u128,
                                addend as u128,
                            )?;
                            if prunable(node, verified_bound) {
                                prefix_pruned = true;
                                break;
                            }
                        }
                        if prefix_pruned {
                            continue;
                        }

                        let lane_index = (prefix as u64) & (lane_count - 1);
                        record_natural_tail_subtree(
                            &mut profiles[lane_index as usize],
                            node,
                            split_depth,
                            multiplier,
                            addend,
                            depth,
                            max_steps,
                            minimum_odds,
                            verified_bound,
                        )?;
                    }
                    Ok::<_, String>(profiles)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "affine natural-tail worker panicked".to_string())?
            })
            .collect::<Result<Vec<_>, _>>()
    })?;

    let mut profiles = (0..lane_count)
        .map(|lane_index| empty_natural_tail_profile(depth, lane_index, lane_count, max_steps))
        .collect::<Vec<_>>();
    for worker in worker_profiles {
        for (profile, partial) in profiles.iter_mut().zip(worker) {
            merge_natural_tail_profile(profile, partial);
        }
    }
    for profile in &mut profiles {
        finalize_natural_tail_profile(profile, multiplier, addend, verified_power, max_steps)?;
    }
    Ok(profiles)
}
