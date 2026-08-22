#[derive(Clone, Debug)]
pub struct EquispacedBlockReport {
    pub point_count: i64,
    pub gap_micros: i64,
    pub energy_upper_scaled: i64,
    pub span_tax_upper_scaled: i64,
    pub total_upper_scaled: i64,
}

#[derive(Clone, Debug)]
pub struct BandwidthScoutReport {
    pub lambda_micros: i64,
    pub baseline_lower_scaled: i64,
}

/// Numerical scout for the scale-free Montgomery--Taylor variational
/// constant beyond the currently proved bandwidth-one range.
///
/// For theta=lambda/sqrt(2), the optimized functional gives
/// `H(lambda)=2-(cot(theta)+theta)/sqrt(2)`.  This function deliberately
/// reports a widened binary64 lower scout, not a theorem for lambda > 1;
/// the missing work there is the prime-side analytic estimate.
pub fn bandwidth_scout(lambda_micros: i64) -> Result<BandwidthScoutReport, String> {
    if !(500_000..=1_500_000).contains(&lambda_micros) {
        return Err("Montgomery--Taylor scout bandwidth is out of range".to_string());
    }
    let lambda = lambda_micros as f64 / 1_000_000.0;
    let sqrt_two = 2.0_f64.sqrt();
    let theta = lambda / sqrt_two;
    let baseline = 2.0 - (theta.cos() / theta.sin() + theta) / sqrt_two;
    if !baseline.is_finite() || baseline <= 0.0 || baseline >= 1.0 {
        return Err("non-finite Montgomery--Taylor scout result".to_string());
    }
    let widened_lower = f64::from_bits(baseline.to_bits() - 1);
    let baseline_lower_scaled = (widened_lower * 1_000_000_000.0).floor() as i64 - 1;
    Ok(BandwidthScoutReport {
        lambda_micros,
        baseline_lower_scaled,
    })
}

/// Rigorous rational upper bound for a block with a positive integer gap.
///
/// At an integer the normalized overlap kernel simplifies exactly, up to sign,
/// to
///
/// `|k(n)| = 1 / (2*pi^2*n^2 - 1)`.
///
/// For `n=gap*r`, the elementary bound `pi > 3` gives
/// `k(n)^2 < 1 / (((18*gap^2-1)^2)*r^4)`.  Summing separately rounded-up
/// rational terms makes this an integer-only obstruction certificate; no
/// floating point or transcendental implementation is in its trust base.
pub fn integer_spacing_block_bound(
    point_count: i64,
    gap_units: i64,
) -> Result<EquispacedBlockReport, String> {
    const REPORT_SCALE: i128 = 1_000_000_000;

    if !(2..=512).contains(&point_count) || !(1..=100).contains(&gap_units) {
        return Err("integer-spacing zeta block parameters are out of range".to_string());
    }
    let gap = i128::from(gap_units);
    let kernel_denominator = 18 * gap * gap - 1;
    let kernel_denominator_factor = kernel_denominator * kernel_denominator;

    let mut energy_upper_scaled = 0_i128;
    for separation in 1..point_count {
        let r = i128::from(separation);
        let denominator = kernel_denominator_factor * r * r * r * r;
        let numerator = 2 * i128::from(point_count - separation) * REPORT_SCALE;
        energy_upper_scaled += (numerator + denominator - 1) / denominator;
    }

    // span = gap*(m-1).  The report scale is divisible by 500, making
    // span/500 exact.
    let span_tax_upper_scaled = i128::from(point_count - 1) * gap * REPORT_SCALE / 500;
    let total_upper_scaled = energy_upper_scaled + span_tax_upper_scaled;
    Ok(EquispacedBlockReport {
        point_count,
        gap_micros: gap_units * 1_000_000,
        energy_upper_scaled: i64::try_from(energy_upper_scaled)
            .map_err(|_| "even-spacing energy bound overflowed i64".to_string())?,
        span_tax_upper_scaled: i64::try_from(span_tax_upper_scaled)
            .map_err(|_| "even-spacing span bound overflowed i64".to_string())?,
        total_upper_scaled: i64::try_from(total_upper_scaled)
            .map_err(|_| "even-spacing total bound overflowed i64".to_string())?,
    })
}

pub fn even_spacing_block_bound(point_count: i64) -> Result<EquispacedBlockReport, String> {
    integer_spacing_block_bound(point_count, 2)
}

/// Numerical strategy witness for an equally spaced block.
///
/// This is deliberately not part of the rigorous certificate consumer.  RAD's
/// strategy laboratory uses it to reject implausible proof architectures
/// before asking Arb for a directed-rounding witness.
pub fn equispaced_block(
    point_count: i64,
    gap_micros: i64,
) -> Result<EquispacedBlockReport, String> {
    if !(7..=512).contains(&point_count) || !(1..=100_000_000).contains(&gap_micros) {
        return Err("equispaced zeta block parameters are out of range".to_string());
    }
    let gap = gap_micros as f64 / 1_000_000.0;
    let inv_sqrt_two = 1.0 / 2.0_f64.sqrt();
    let normalization = 2.0_f64.sqrt() * inv_sqrt_two.sin();
    let sinc = |value: f64| {
        if value.abs() < 1.0e-12 {
            1.0
        } else {
            value.sin() / value
        }
    };
    let mut energy = 0.0f64;
    for separation in 1..point_count {
        let x = separation as f64 * gap;
        let raw = 0.5
            * (sinc(std::f64::consts::PI * x - inv_sqrt_two)
                + sinc(std::f64::consts::PI * x + inv_sqrt_two));
        let kernel = raw / normalization;
        energy += 2.0 * (point_count - separation) as f64 * kernel * kernel;
    }
    let span_tax = (point_count - 1) as f64 * gap / 500.0;
    let upper_scaled = |value: f64| -> Result<i64, String> {
        if !value.is_finite() || value < 0.0 {
            return Err("non-finite equispaced zeta witness".to_string());
        }
        // Widen one binary64 ULP, round upward at 1e-9, then add one unit.
        // This remains a numerical scouting margin, not an Arb enclosure.
        let widened = f64::from_bits(value.to_bits() + 1);
        Ok((widened * 1_000_000_000.0).ceil() as i64 + 1)
    };
    let energy_upper_scaled = upper_scaled(energy)?;
    let span_tax_upper_scaled = upper_scaled(span_tax)?;
    Ok(EquispacedBlockReport {
        point_count,
        gap_micros,
        energy_upper_scaled,
        span_tax_upper_scaled,
        total_upper_scaled: energy_upper_scaled + span_tax_upper_scaled,
    })
}
