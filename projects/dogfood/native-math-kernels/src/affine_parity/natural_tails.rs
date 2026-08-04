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

/// Continue each surviving residue with zero high input bits.
///
/// An ordinary positive integer has a finite binary expansion, so its residue
/// representative eventually stops changing as the input modulus grows. This
/// profile measures the exact cost of that natural-number boundary condition:
/// when the affine coefficient first becomes contracting, and when the actual
/// orbit subsequently falls below its fixed starting value. Generic 2-adic
/// branches need not have this zero-tail property.
pub(crate) fn natural_tail_lane_profile(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_index: u64,
    lane_count: u64,
    max_steps: u32,
) -> Result<NaturalTailLaneProfile, String> {
    if max_steps <= depth || max_steps > 4096 {
        return Err(
            "affine natural-tail max_steps must be greater than depth and at most 4096".to_string(),
        );
    }
    let (residue_profile, frontier) = residue_lane_analysis(
        multiplier,
        addend,
        depth,
        verified_power,
        lane_index,
        lane_count,
    )?;
    let minimum_odds = minimum_noncontracting_odd_steps(multiplier, max_steps);
    let mut profile = NaturalTailLaneProfile {
        depth,
        lane_index,
        lane_count,
        survivor_classes: residue_profile.survivor_classes,
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
    };

    for node in frontier {
        let start = node.residue as u128;
        if start == 0 {
            return Err("affine natural-tail survivor cannot be zero".into());
        }
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
    }

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
    hasher.update(&depth.to_le_bytes());
    hasher.update(&verified_power.to_le_bytes());
    hasher.update(&lane_index.to_le_bytes());
    hasher.update(&lane_count.to_le_bytes());
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
    Ok(profile)
}

pub(crate) fn natural_tail_lane_profiles(
    multiplier: u64,
    addend: u64,
    depth: u32,
    verified_power: u32,
    lane_count: u64,
    max_steps: u32,
) -> Result<Vec<NaturalTailLaneProfile>, String> {
    checked_inputs(multiplier, addend, depth, verified_power, 0, lane_count)?;
    std::thread::scope(|scope| {
        let handles = (0..lane_count)
            .map(|lane_index| {
                scope.spawn(move || {
                    natural_tail_lane_profile(
                        multiplier,
                        addend,
                        depth,
                        verified_power,
                        lane_index,
                        lane_count,
                        max_steps,
                    )
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
            .collect()
    })
}
