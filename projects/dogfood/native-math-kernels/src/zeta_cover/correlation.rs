#[derive(Clone, Copy, Debug)]
pub struct FourthMomentTransferScoutReport {
    pub limit: i64,
    pub max_shift: i64,
    pub small_factor_cap: i64,
    pub nonzero_coefficients: i64,
    pub mean_coefficient_scaled: i64,
    pub aggregate_ratio_scaled: i64,
    pub sampled_aggregate_ratio_scaled: i64,
    pub fejer_aggregate_ratio_scaled: i64,
    pub ascending_aggregate_ratio_scaled: i64,
    pub even_ratio_scaled: i64,
    pub odd_ratio_scaled: i64,
    pub rms_error_scaled: i64,
    pub minimum_ratio_scaled: i64,
    pub maximum_ratio_scaled: i64,
    pub residue_block_rms_scaled: i64,
    pub maximum_residue_block_error_scaled: i64,
    pub typical_aggregate_ratio_scaled: i64,
    pub balanced_aggregate_ratio_scaled: i64,
    pub cross_aggregate_ratio_scaled: i64,
    pub excluded_mean_fraction_scaled: i64,
    pub tail_l2_fraction_scaled: i64,
    pub inside_relative_slack: bool,
}

/// Finite arithmetic scout for the promoted fourth-moment transfer attack.
///
/// It constructs the truncated von Mangoldt convolution
/// `a(n)=(Lambda*Lambda)(n)` natively, then measures its shifted correlation
/// over `[limit/2, limit]`.  Averaging over shifts removes the singular-series
/// oscillation in the proposed transfer.  This is diagnostic evidence only:
/// it does not prove the uniform weighted asymptotic needed by the theorem.
pub fn fourth_moment_transfer_scout(
    limit: i64,
    max_shift: i64,
    small_factor_cap: i64,
) -> Result<FourthMomentTransferScoutReport, String> {
    if !(20_000..=2_000_000).contains(&limit) {
        return Err("fourth-moment scout limit is out of range".to_string());
    }
    if !(4..=4096).contains(&max_shift) || max_shift >= limit / 4 {
        return Err("fourth-moment scout shift range is out of range".to_string());
    }
    if !(2..=65_536).contains(&small_factor_cap) {
        return Err("fourth-moment scout factor cutoff is out of range".to_string());
    }
    let limit_usize = usize::try_from(limit).map_err(|_| "negative scout limit".to_string())?;
    let shift_usize = usize::try_from(max_shift).map_err(|_| "negative scout shift".to_string())?;

    let mut composite = vec![false; limit_usize + 1];
    let mut mangoldt = vec![0.0_f64; limit_usize + 1];
    for p in 2..=limit_usize {
        if composite[p] {
            continue;
        }
        if p <= limit_usize / p {
            let mut multiple = p * p;
            while multiple <= limit_usize {
                composite[multiple] = true;
                multiple += p;
            }
        }
        let log_p = (p as f64).ln();
        let mut power = p;
        loop {
            mangoldt[power] = log_p;
            match power.checked_mul(p) {
                Some(next) if next <= limit_usize => power = next,
                _ => break,
            }
        }
    }

    let support: Vec<(usize, f64)> = mangoldt
        .iter()
        .enumerate()
        .skip(2)
        .filter_map(|(index, value)| (*value > 0.0).then_some((index, *value)))
        .collect();
    let mut convolution = vec![0.0_f64; limit_usize + 1];
    let mut typical_convolution = vec![0.0_f64; limit_usize + 1];
    let small_factor_cap_usize = usize::try_from(small_factor_cap)
        .map_err(|_| "negative fourth-moment factor cutoff".to_string())?;
    for &(left, left_value) in &support {
        let maximum_right = limit_usize / left;
        for &(right, right_value) in &support {
            if right > maximum_right {
                break;
            }
            convolution[left * right] += left_value * right_value;
            if left.min(right) <= small_factor_cap_usize {
                typical_convolution[left * right] += left_value * right_value;
            }
        }
    }

    let start = limit_usize / 2;
    let end = limit_usize - shift_usize;
    let count = end - start + 1;
    let mut nonzero = 0_i64;
    let mut base_sum = 0.0_f64;
    let mut typical_base_sum = 0.0_f64;
    let mut full_l2 = 0.0_f64;
    let mut tail_l2 = 0.0_f64;
    for index in start..=end {
        let value = convolution[index];
        let typical_value = typical_convolution[index];
        base_sum += value;
        typical_base_sum += typical_value;
        full_l2 += value * value;
        let tail = value - typical_value;
        tail_l2 += tail * tail;
        if value > 0.0 {
            nonzero += 1;
        }
    }
    let base_mean = base_sum / count as f64;
    let typical_base_mean = typical_base_sum / count as f64;
    if !base_mean.is_finite() || base_mean <= 0.0 {
        return Err("fourth-moment scout produced an empty convolution".to_string());
    }

    // Evaluate the pooled correlation over every 1 <= h <= max_shift in
    // linear time.  For each n, a prefix-sum query replaces the shift loop:
    //   sum_h a(n)a(n+h) = a(n) * (A(n+H)-A(n)).
    let mut convolution_prefix = vec![0.0_f64; limit_usize + 2];
    let mut typical_prefix = vec![0.0_f64; limit_usize + 2];
    let mut weighted_convolution_prefix = vec![0.0_f64; limit_usize + 2];
    for index in 0..=limit_usize {
        convolution_prefix[index + 1] = convolution_prefix[index] + convolution[index];
        typical_prefix[index + 1] = typical_prefix[index] + typical_convolution[index];
        weighted_convolution_prefix[index + 1] =
            weighted_convolution_prefix[index] + index as f64 * convolution[index];
    }
    let mut pooled_correlation = 0.0_f64;
    let mut pooled_shifted_sum = 0.0_f64;
    let mut pooled_typical_correlation = 0.0_f64;
    let mut pooled_typical_shifted_sum = 0.0_f64;
    let mut pooled_balanced_correlation = 0.0_f64;
    let mut pooled_balanced_shifted_sum = 0.0_f64;
    let mut pooled_cross_correlation = 0.0_f64;
    let mut fejer_correlation = 0.0_f64;
    let mut fejer_shifted_sum = 0.0_f64;
    let mut ascending_correlation = 0.0_f64;
    let mut ascending_shifted_sum = 0.0_f64;
    for index in start..=end {
        let shifted_sum =
            convolution_prefix[index + shift_usize + 1] - convolution_prefix[index + 1];
        let typical_shifted_sum =
            typical_prefix[index + shift_usize + 1] - typical_prefix[index + 1];
        let balanced = convolution[index] - typical_convolution[index];
        let balanced_shifted_sum = shifted_sum - typical_shifted_sum;
        let weighted_shifted_sum = weighted_convolution_prefix[index + shift_usize + 1]
            - weighted_convolution_prefix[index + 1]
            - index as f64 * shifted_sum;
        let fejer_window = (shift_usize + 1) as f64 * shifted_sum - weighted_shifted_sum;
        pooled_correlation += convolution[index] * shifted_sum;
        pooled_shifted_sum += shifted_sum;
        pooled_typical_correlation += typical_convolution[index] * typical_shifted_sum;
        pooled_typical_shifted_sum += typical_shifted_sum;
        pooled_balanced_correlation += balanced * balanced_shifted_sum;
        pooled_balanced_shifted_sum += balanced_shifted_sum;
        pooled_cross_correlation +=
            typical_convolution[index] * balanced_shifted_sum + balanced * typical_shifted_sum;
        fejer_correlation += convolution[index] * fejer_window;
        fejer_shifted_sum += fejer_window;
        ascending_correlation += convolution[index] * weighted_shifted_sum;
        ascending_shifted_sum += weighted_shifted_sum;
    }
    let pooled_ratio = pooled_correlation * count as f64 / (base_sum * pooled_shifted_sum);
    let fejer_ratio = fejer_correlation * count as f64 / (base_sum * fejer_shifted_sum);
    let ascending_ratio = ascending_correlation * count as f64 / (base_sum * ascending_shifted_sum);
    let pooled_typical_ratio =
        pooled_typical_correlation * count as f64 / (typical_base_sum * pooled_typical_shifted_sum);
    let balanced_base_sum = base_sum - typical_base_sum;
    let pooled_balanced_ratio = if balanced_base_sum > 0.0 && pooled_balanced_shifted_sum > 0.0 {
        pooled_balanced_correlation * count as f64
            / (balanced_base_sum * pooled_balanced_shifted_sum)
    } else {
        0.0
    };
    let pooled_cross_ratio = if typical_base_sum * pooled_balanced_shifted_sum
        + balanced_base_sum * pooled_typical_shifted_sum
        > 0.0
    {
        pooled_cross_correlation * count as f64
            / (typical_base_sum * pooled_balanced_shifted_sum
                + balanced_base_sum * pooled_typical_shifted_sum)
    } else {
        0.0
    };

    // For long shift ranges, use eight complete residue blocks modulo 30.
    // This keeps the hot path bounded while preventing the scout itself from
    // introducing a parity or small-prime residue bias.
    let sampled_shifts: Vec<usize> = if shift_usize <= 256 {
        (1..=shift_usize).collect()
    } else {
        const BLOCK_WIDTH: usize = 30;
        const BLOCK_COUNT: usize = 8;
        let available = shift_usize - BLOCK_WIDTH;
        let mut shifts = Vec::with_capacity(BLOCK_WIDTH * BLOCK_COUNT);
        for block in 0..BLOCK_COUNT {
            let start = 1 + block * available / (BLOCK_COUNT - 1);
            shifts.extend(start..start + BLOCK_WIDTH);
        }
        shifts
    };
    let sample_count = sampled_shifts.len();
    let mut ratios = Vec::with_capacity(sample_count);
    let mut typical_ratios = Vec::with_capacity(sample_count);
    let mut balanced_ratios = Vec::with_capacity(sample_count);
    let mut cross_ratios = Vec::with_capacity(sample_count);
    let mut even_sum = 0.0_f64;
    let mut odd_sum = 0.0_f64;
    let mut even_count = 0usize;
    let mut odd_count = 0usize;
    for shift in sampled_shifts {
        let mut shifted_sum = 0.0_f64;
        let mut typical_shifted_sum = 0.0_f64;
        let mut correlation = 0.0_f64;
        let mut typical_correlation = 0.0_f64;
        let mut balanced_correlation = 0.0_f64;
        let mut cross_correlation = 0.0_f64;
        for index in start..=end {
            shifted_sum += convolution[index + shift];
            correlation += convolution[index] * convolution[index + shift];
            typical_shifted_sum += typical_convolution[index + shift];
            typical_correlation += typical_convolution[index] * typical_convolution[index + shift];
            let balanced = convolution[index] - typical_convolution[index];
            let balanced_shifted = convolution[index + shift] - typical_convolution[index + shift];
            balanced_correlation += balanced * balanced_shifted;
            cross_correlation += typical_convolution[index] * balanced_shifted
                + balanced * typical_convolution[index + shift];
        }
        let shifted_mean = shifted_sum / count as f64;
        let ratio = correlation / (count as f64 * base_mean * shifted_mean);
        if !ratio.is_finite() {
            return Err("fourth-moment scout produced a non-finite ratio".to_string());
        }
        ratios.push(ratio);
        if typical_base_mean > 0.0 && typical_shifted_sum > 0.0 {
            let typical_shifted_mean = typical_shifted_sum / count as f64;
            typical_ratios.push(
                typical_correlation / (count as f64 * typical_base_mean * typical_shifted_mean),
            );
            let balanced_base_mean = base_mean - typical_base_mean;
            let balanced_shifted_mean = shifted_mean - typical_shifted_mean;
            if balanced_base_mean > 0.0 && balanced_shifted_mean > 0.0 {
                balanced_ratios.push(
                    balanced_correlation
                        / (count as f64 * balanced_base_mean * balanced_shifted_mean),
                );
                cross_ratios.push(
                    cross_correlation
                        / (count as f64
                            * (typical_base_mean * balanced_shifted_mean
                                + balanced_base_mean * typical_shifted_mean)),
                );
            }
        }
        if shift % 2 == 0 {
            even_sum += ratio;
            even_count += 1;
        } else {
            odd_sum += ratio;
            odd_count += 1;
        }
    }

    let sampled_aggregate = ratios.iter().sum::<f64>() / ratios.len() as f64;
    let aggregate = pooled_ratio;
    let typical_aggregate = pooled_typical_ratio;
    let balanced_aggregate = pooled_balanced_ratio;
    let cross_aggregate = pooled_cross_ratio;
    let even_ratio = even_sum / even_count as f64;
    let odd_ratio = odd_sum / odd_count as f64;
    let rms = (ratios
        .iter()
        .map(|ratio| (ratio - 1.0) * (ratio - 1.0))
        .sum::<f64>()
        / ratios.len() as f64)
        .sqrt();
    let minimum = ratios.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = ratios.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let residue_block_means: Vec<f64> = ratios
        .chunks_exact(30)
        .map(|block| block.iter().sum::<f64>() / block.len() as f64)
        .collect();
    let residue_block_rms = if residue_block_means.is_empty() {
        rms
    } else {
        (residue_block_means
            .iter()
            .map(|ratio| (ratio - 1.0) * (ratio - 1.0))
            .sum::<f64>()
            / residue_block_means.len() as f64)
            .sqrt()
    };
    let maximum_residue_block_error = if residue_block_means.is_empty() {
        (aggregate - 1.0).abs()
    } else {
        residue_block_means
            .iter()
            .map(|ratio| (ratio - 1.0).abs())
            .fold(0.0_f64, f64::max)
    };
    let scaled = |value: f64| -> i64 { (value * 1_000_000_000.0).round() as i64 };
    let aggregate_scaled = scaled(aggregate);

    Ok(FourthMomentTransferScoutReport {
        limit,
        max_shift,
        small_factor_cap,
        nonzero_coefficients: nonzero,
        mean_coefficient_scaled: (base_mean * 1_000_000.0).round() as i64,
        aggregate_ratio_scaled: aggregate_scaled,
        sampled_aggregate_ratio_scaled: scaled(sampled_aggregate),
        fejer_aggregate_ratio_scaled: scaled(fejer_ratio),
        ascending_aggregate_ratio_scaled: scaled(ascending_ratio),
        even_ratio_scaled: scaled(even_ratio),
        odd_ratio_scaled: scaled(odd_ratio),
        rms_error_scaled: scaled(rms),
        minimum_ratio_scaled: scaled(minimum),
        maximum_ratio_scaled: scaled(maximum),
        residue_block_rms_scaled: scaled(residue_block_rms),
        maximum_residue_block_error_scaled: scaled(maximum_residue_block_error),
        typical_aggregate_ratio_scaled: scaled(typical_aggregate),
        balanced_aggregate_ratio_scaled: scaled(balanced_aggregate),
        cross_aggregate_ratio_scaled: scaled(cross_aggregate),
        excluded_mean_fraction_scaled: scaled((base_sum - typical_base_sum) / base_sum),
        tail_l2_fraction_scaled: scaled(tail_l2 / full_l2),
        inside_relative_slack: (aggregate_scaled - 1_000_000_000).abs() <= 1_000_000_000 / 117,
    })
}
