#[derive(Clone, Debug)]
pub struct CosinePairScoutReport {
    pub a_micros: i64,
    pub baseline_scaled: i64,
    pub best_q_scaled: i64,
    pub local_a_scaled: i64,
    pub minimizing_gap_micros: i64,
    pub simple_bound_scaled: i64,
}

#[derive(Clone, Debug)]
pub struct CosineDirectSumScoutReport {
    pub companion_a_micros: i64,
    pub companion_weight_ppm: i64,
    pub baseline_scaled: i64,
    pub best_q_scaled: i64,
    pub local_a_scaled: i64,
    pub minimizing_gap_micros: i64,
    pub simple_bound_scaled: i64,
}

#[derive(Clone, Debug)]
pub struct PerturbedWindowScoutReport {
    pub mode_coefficient_micros: i64,
    pub second_mode_coefficient_micros: i64,
    pub baseline_scaled: i64,
    pub local_c_scaled: i64,
    pub best_block_size: i64,
    pub simple_bound_scaled: i64,
    pub minimizing_gaps_micros: Vec<i64>,
    pub starts_checked: i64,
}

#[derive(Clone, Debug)]
pub struct PeriodicDensityScoutReport {
    pub long_gap_mask: i64,
    pub short_gap_micros: i64,
    pub long_gap_micros: i64,
    pub energy_per_point_scaled: i64,
    pub implied_simple_bound_scaled: i64,
    pub neighbor_terms: i64,
}

/// Numerical scout for a bandwidth-one cosine window optimized jointly with
/// the exact two-point Gram defect.
///
/// Put `v_a(s)=cos(2*a*s)` on `[-1/2,1/2]`.  Its Montgomery--Taylor baseline
/// and normalized overlap kernel are
///
/// `H(a)=2-(b(a)+J(a))/A(a)^2`,
/// `K_a(x)=(sinc(pi*x-a)+sinc(pi*x+a))/(2*sinc(a))`.
///
/// A consecutive two-point block has defect `2*K_a(g)^2`.  If
/// `2*K_a(g)^2+q*g >= C` for every gap, block summation gives the simple-zero
/// proportion `(H-q/2)/(1-C/2)`.  This hot path samples the one-dimensional
/// infimum and then optimizes q.  It is a strategy scout, not a proof: the
/// sampled C is an upper approximation to the true infimum.
pub fn cosine_pair_scout(a_micros: i64) -> Result<CosinePairScoutReport, String> {
    const REPORT_SCALE: f64 = 1_000_000_000.0;
    const GAP_STEP: f64 = 0.001;
    const GAP_STEPS: usize = 8_000;
    const Q_MAX: f64 = 0.8;
    const Q_STEPS: usize = 400;

    if !(100_000..=1_570_000).contains(&a_micros) {
        return Err("cosine-window parameter is out of range".to_string());
    }
    let a = a_micros as f64 / 1_000_000.0;
    let sinc = |value: f64| {
        if value.abs() < 1.0e-12 {
            1.0
        } else {
            value.sin() / value
        }
    };

    let integral = sinc(a);
    let square_integral = 0.5 + (2.0 * a).sin() / (4.0 * a);
    let distance_integral = (2.0 * a).sin() / (8.0 * a * a * a) - (2.0 * a).cos() / (4.0 * a * a);
    let baseline = 2.0 - (square_integral + distance_integral) / (integral * integral);
    if !baseline.is_finite() || baseline <= 0.0 || baseline >= 1.0 {
        return Err("non-finite cosine-window baseline".to_string());
    }

    // The kernel is independent of q, so calculate its sampled defect once.
    // GAP_STEPS reaches well beyond the first transform zero.  For every q
    // considered by the optimizer that first zero beats the trivial q*x
    // lower tail beyond this cutoff.
    let mut defects = Vec::with_capacity(GAP_STEPS + 1);
    for gap_step in 0..=GAP_STEPS {
        let gap = gap_step as f64 * GAP_STEP;
        let raw =
            0.5 * (sinc(std::f64::consts::PI * gap - a) + sinc(std::f64::consts::PI * gap + a));
        let kernel = raw / integral;
        defects.push(2.0 * kernel * kernel);
    }

    let evaluate_q = |q: f64| -> (f64, usize, f64) {
        if q == 0.0 {
            return (0.0, GAP_STEPS, baseline);
        }
        let mut local_a = f64::INFINITY;
        let mut minimizing_step = 0;
        for (gap_step, defect) in defects.iter().enumerate() {
            let value = *defect + q * gap_step as f64 * GAP_STEP;
            if value < local_a {
                local_a = value;
                minimizing_step = gap_step;
            }
        }
        let simple_bound = (baseline - q / 2.0) / (1.0 - local_a / 2.0);
        (local_a, minimizing_step, simple_bound)
    };

    let mut best_q = 0.0;
    let mut best_local_a = 0.0;
    let mut best_gap_step = GAP_STEPS;
    let mut best_bound = baseline;
    for q_step in 1..=Q_STEPS {
        let q = Q_MAX * q_step as f64 / Q_STEPS as f64;
        let (local_a, gap_step, bound) = evaluate_q(q);
        if bound > best_bound {
            best_q = q;
            best_local_a = local_a;
            best_gap_step = gap_step;
            best_bound = bound;
        }
    }

    // Golden-section refinement is deterministic and keeps transcendental
    // work out of the inner optimization loop (all K_a samples are reused).
    let coarse_step = Q_MAX / Q_STEPS as f64;
    let mut left = (best_q - coarse_step).max(0.0);
    let mut right = (best_q + coarse_step).min(Q_MAX);
    let golden = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut q1 = right - golden * (right - left);
    let mut q2 = left + golden * (right - left);
    let mut value1 = evaluate_q(q1).2;
    let mut value2 = evaluate_q(q2).2;
    for _ in 0..48 {
        if value1 < value2 {
            left = q1;
            q1 = q2;
            value1 = value2;
            q2 = left + golden * (right - left);
            value2 = evaluate_q(q2).2;
        } else {
            right = q2;
            q2 = q1;
            value2 = value1;
            q1 = right - golden * (right - left);
            value1 = evaluate_q(q1).2;
        }
    }
    let refined_q = (left + right) / 2.0;
    let (refined_local_a, refined_gap_step, refined_bound) = evaluate_q(refined_q);
    if refined_bound > best_bound {
        best_q = refined_q;
        best_local_a = refined_local_a;
        best_gap_step = refined_gap_step;
        best_bound = refined_bound;
    }

    let scaled = |value: f64| -> i64 { (value * REPORT_SCALE).round() as i64 };
    Ok(CosinePairScoutReport {
        a_micros,
        baseline_scaled: scaled(baseline),
        best_q_scaled: scaled(best_q),
        local_a_scaled: scaled(best_local_a),
        minimizing_gap_micros: (best_gap_step as f64 * GAP_STEP * 1_000_000.0).round() as i64,
        simple_bound_scaled: scaled(best_bound),
    })
}

/// Numerical direct-sum scout at proved bandwidth one.
///
/// Convexly combine the standard optimized window `a=1/sqrt(2)` with a
/// companion cosine window.  Both individual second-moment inequalities are
/// available at bandwidth one, so their weighted baseline and Gram defects
/// may be combined.  Distinct kernels need not share transform zeros, which
/// is the simplest way to evade the scalar-window obstruction.
pub fn cosine_direct_sum_scout(
    companion_a_micros: i64,
    companion_weight_ppm: i64,
) -> Result<CosineDirectSumScoutReport, String> {
    const REPORT_SCALE: f64 = 1_000_000_000.0;
    const GAP_STEP: f64 = 0.001;
    const GAP_STEPS: usize = 8_000;
    const Q_MAX: f64 = 0.8;
    const Q_STEPS: usize = 400;

    if !(100_000..=1_570_000).contains(&companion_a_micros)
        || !(0..=1_000_000).contains(&companion_weight_ppm)
    {
        return Err("cosine direct-sum parameters are out of range".to_string());
    }
    let standard_a = std::f64::consts::FRAC_1_SQRT_2;
    let companion_a = companion_a_micros as f64 / 1_000_000.0;
    let weight = companion_weight_ppm as f64 / 1_000_000.0;
    let sinc = |value: f64| {
        if value.abs() < 1.0e-12 {
            1.0
        } else {
            value.sin() / value
        }
    };
    let baseline = |a: f64| {
        let integral = sinc(a);
        let square_integral = 0.5 + (2.0 * a).sin() / (4.0 * a);
        let distance_integral =
            (2.0 * a).sin() / (8.0 * a * a * a) - (2.0 * a).cos() / (4.0 * a * a);
        2.0 - (square_integral + distance_integral) / (integral * integral)
    };
    let standard_baseline = baseline(standard_a);
    let companion_baseline = baseline(companion_a);
    let combined_baseline = (1.0 - weight) * standard_baseline + weight * companion_baseline;
    if !combined_baseline.is_finite() || combined_baseline <= 0.0 || combined_baseline >= 1.0 {
        return Err("non-finite cosine direct-sum baseline".to_string());
    }

    let standard_integral = sinc(standard_a);
    let companion_integral = sinc(companion_a);
    let mut defects = Vec::with_capacity(GAP_STEPS + 1);
    for gap_step in 0..=GAP_STEPS {
        let gap = gap_step as f64 * GAP_STEP;
        let kernel = |a: f64, integral: f64| {
            0.5 * (sinc(std::f64::consts::PI * gap - a) + sinc(std::f64::consts::PI * gap + a))
                / integral
        };
        let standard_kernel = kernel(standard_a, standard_integral);
        let companion_kernel = kernel(companion_a, companion_integral);
        defects.push(
            2.0 * ((1.0 - weight) * standard_kernel * standard_kernel
                + weight * companion_kernel * companion_kernel),
        );
    }

    let evaluate_q = |q: f64| -> (f64, usize, f64) {
        if q == 0.0 {
            return (0.0, GAP_STEPS, combined_baseline);
        }
        let mut local_a = f64::INFINITY;
        let mut minimizing_step = 0;
        for (gap_step, defect) in defects.iter().enumerate() {
            let value = *defect + q * gap_step as f64 * GAP_STEP;
            if value < local_a {
                local_a = value;
                minimizing_step = gap_step;
            }
        }
        let simple_bound = (combined_baseline - q / 2.0) / (1.0 - local_a / 2.0);
        (local_a, minimizing_step, simple_bound)
    };

    let mut best_q = 0.0;
    let mut best_local_a = 0.0;
    let mut best_gap_step = GAP_STEPS;
    let mut best_bound = combined_baseline;
    for q_step in 1..=Q_STEPS {
        let q = Q_MAX * q_step as f64 / Q_STEPS as f64;
        let (local_a, gap_step, bound) = evaluate_q(q);
        if bound > best_bound {
            best_q = q;
            best_local_a = local_a;
            best_gap_step = gap_step;
            best_bound = bound;
        }
    }

    let coarse_step = Q_MAX / Q_STEPS as f64;
    let mut left = (best_q - coarse_step).max(0.0);
    let mut right = (best_q + coarse_step).min(Q_MAX);
    let golden = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut q1 = right - golden * (right - left);
    let mut q2 = left + golden * (right - left);
    let mut value1 = evaluate_q(q1).2;
    let mut value2 = evaluate_q(q2).2;
    for _ in 0..48 {
        if value1 < value2 {
            left = q1;
            q1 = q2;
            value1 = value2;
            q2 = left + golden * (right - left);
            value2 = evaluate_q(q2).2;
        } else {
            right = q2;
            q2 = q1;
            value2 = value1;
            q1 = right - golden * (right - left);
            value1 = evaluate_q(q1).2;
        }
    }
    let refined_q = (left + right) / 2.0;
    let (refined_local_a, refined_gap_step, refined_bound) = evaluate_q(refined_q);
    if refined_bound > best_bound {
        best_q = refined_q;
        best_local_a = refined_local_a;
        best_gap_step = refined_gap_step;
        best_bound = refined_bound;
    }

    let scaled = |value: f64| -> i64 { (value * REPORT_SCALE).round() as i64 };
    Ok(CosineDirectSumScoutReport {
        companion_a_micros,
        companion_weight_ppm,
        baseline_scaled: scaled(combined_baseline),
        best_q_scaled: scaled(best_q),
        local_a_scaled: scaled(best_local_a),
        minimizing_gap_micros: (best_gap_step as f64 * GAP_STEP * 1_000_000.0).round() as i64,
        simple_bound_scaled: scaled(best_bound),
    })
}

/// Numerical seven-point scout for a single bandwidth-one window perturbed in
/// a direction that detects the near-integer minimizers of the current cover.
///
/// The window is
///
/// `v(s)=cos(sqrt(2)*s)+c*cos(2*pi*s)`,  `|s|<=1/2`.
///
/// The second mode has integral zero and is tangent to the normalization
/// constraint.  Since the first mode is the variational optimum, the baseline
/// loss starts quadratically in c, whereas the certified seven-point defect
/// can change at first order.  A deterministic multi-start pattern search
/// minimizes the six-gap functional at pressure 1/3000.  Its returned minimum
/// is sampled and therefore an upper approximation, suitable only for attack
/// selection until a directed cover certifies it.
pub fn perturbed_window_scout(
    mode_coefficient_micros: i64,
) -> Result<PerturbedWindowScoutReport, String> {
    perturbed_window_two_mode_scout(mode_coefficient_micros, 0)
}

/// Two-mode extension of [`perturbed_window_scout`].  Both added cosine
/// modes integrate to zero, so normalization stays exact while the hot path
/// searches a larger bandwidth-one window family.
pub fn perturbed_window_two_mode_scout(
    mode_coefficient_micros: i64,
    second_mode_coefficient_micros: i64,
) -> Result<PerturbedWindowScoutReport, String> {
    const REPORT_SCALE: f64 = 1_000_000_000.0;
    const PRESSURE: f64 = 1.0 / 3_000.0;
    const COEFFICIENTS: [f64; 7] = [0.0, 1.0 / 3.0, 2.0 / 5.0, 1.0 / 2.0, 2.0 / 3.0, 1.0, 2.0];
    const GRID_STARTS: usize = 729;
    const RANDOM_STARTS: usize = 64;

    if !(-2_000_000..=2_000_000).contains(&mode_coefficient_micros)
        || !(-2_000_000..=2_000_000).contains(&second_mode_coefficient_micros)
    {
        return Err("perturbed-window coefficient is out of range".to_string());
    }
    let mode_coefficient = mode_coefficient_micros as f64 / 1_000_000.0;
    let second_mode_coefficient = second_mode_coefficient_micros as f64 / 1_000_000.0;
    let alpha0 = 2.0_f64.sqrt();
    let alpha1 = 2.0 * std::f64::consts::PI;
    let alpha2 = 4.0 * std::f64::consts::PI;
    let sinc = |value: f64| {
        if value.abs() < 1.0e-12 {
            1.0
        } else {
            value.sin() / value
        }
    };
    let integral = sinc(alpha0 / 2.0);
    let cosine_inner = |alpha: f64, beta: f64| {
        let difference = (alpha - beta) / 2.0;
        let sum = (alpha + beta) / 2.0;
        0.5 * (sinc(difference) + sinc(sum))
    };
    let distance_inner = |alpha: f64, beta: f64| {
        let inner = cosine_inner(alpha, beta);
        let constant =
            beta.mul_add(0.5, 0.0).sin() / beta + 2.0 * (beta / 2.0).cos() / (beta * beta);
        -2.0 * inner / (beta * beta) + constant * sinc(alpha / 2.0)
    };
    let quadratic = |alpha: f64, beta: f64| cosine_inner(alpha, beta) + distance_inner(alpha, beta);
    let alphas = [alpha0, alpha1, alpha2];
    let mode_coefficients = [1.0, mode_coefficient, second_mode_coefficient];
    let mut baseline_quadratic = 0.0;
    for left in 0..alphas.len() {
        for right in 0..alphas.len() {
            let symmetric = 0.5
                * (quadratic(alphas[left], alphas[right]) + quadratic(alphas[right], alphas[left]));
            baseline_quadratic += mode_coefficients[left] * mode_coefficients[right] * symmetric;
        }
    }
    let baseline = 2.0 - baseline_quadratic / (integral * integral);
    if !baseline.is_finite() || baseline <= 0.0 || baseline >= 1.0 {
        return Err("non-finite perturbed-window baseline".to_string());
    }

    let transform = |alpha: f64, x: f64| {
        0.5 * (sinc(std::f64::consts::PI * x - alpha / 2.0)
            + sinc(std::f64::consts::PI * x + alpha / 2.0))
    };
    let kernel_squared = |x: f64| {
        let value = (transform(alpha0, x)
            + mode_coefficient * transform(alpha1, x)
            + second_mode_coefficient * transform(alpha2, x))
            / integral;
        value * value
    };
    let objective = |gaps: &[f64; 6]| {
        let mut result = PRESSURE * gaps.iter().sum::<f64>();
        for start in 0..6 {
            let mut distance = 0.0;
            for end in start..6 {
                distance += gaps[end];
                result += COEFFICIENTS[end - start + 1] * kernel_squared(distance);
            }
        }
        result
    };
    let descend = |mut gaps: [f64; 6]| {
        let mut value = objective(&gaps);
        let mut step = 0.25;
        while step >= 0.000_015_258_789_062_5 {
            let mut improved = true;
            while improved {
                improved = false;
                for coordinate in 0..6 {
                    for direction in [-1.0, 1.0] {
                        let proposal_value = gaps[coordinate] + direction * step;
                        if !(0.0..=16.0).contains(&proposal_value) {
                            continue;
                        }
                        let mut proposal = gaps;
                        proposal[coordinate] = proposal_value;
                        let candidate = objective(&proposal);
                        if candidate < value {
                            gaps = proposal;
                            value = candidate;
                            improved = true;
                        }
                    }
                }
            }
            step /= 2.0;
        }
        (value, gaps)
    };

    let mut minimum = f64::INFINITY;
    let mut minimizing_gaps = [0.0; 6];
    for mut encoded in 0..GRID_STARTS {
        let mut gaps = [0.0; 6];
        for gap in &mut gaps {
            *gap = (encoded % 3 + 1) as f64;
            encoded /= 3;
        }
        let (value, candidate) = descend(gaps);
        if value < minimum {
            minimum = value;
            minimizing_gaps = candidate;
        }
    }
    let mut random_state = 0x9e37_79b9_7f4a_7c15_u64
        ^ u64::from_ne_bytes(mode_coefficient_micros.to_ne_bytes())
        ^ u64::from_ne_bytes(second_mode_coefficient_micros.to_ne_bytes()).rotate_left(29);
    for _ in 0..RANDOM_STARTS {
        let mut gaps = [0.0; 6];
        for gap in &mut gaps {
            random_state ^= random_state << 13;
            random_state ^= random_state >> 7;
            random_state ^= random_state << 17;
            *gap = 0.25 + (random_state % 39_001) as f64 / 4_000.0;
        }
        let (value, candidate) = descend(gaps);
        if value < minimum {
            minimum = value;
            minimizing_gaps = candidate;
        }
    }
    if !minimum.is_finite() || minimum <= 0.0 || minimum >= 1.0 {
        return Err("non-finite perturbed-window local minimum".to_string());
    }

    let mut best_block_size = 7;
    let mut best_bound = 0.0;
    for block_size in 7..=4_096 {
        let block = block_size as f64;
        let numerator = block * baseline - 6.0 * PRESSURE * (block - 1.0);
        let denominator = block - (block - 6.0) * minimum;
        let bound = numerator / denominator;
        if bound > best_bound {
            best_bound = bound;
            best_block_size = block_size;
        }
    }
    let scaled = |value: f64| -> i64 { (value * REPORT_SCALE).round() as i64 };
    Ok(PerturbedWindowScoutReport {
        mode_coefficient_micros,
        second_mode_coefficient_micros,
        baseline_scaled: scaled(baseline),
        local_c_scaled: scaled(minimum),
        best_block_size,
        simple_bound_scaled: scaled(best_bound),
        minimizing_gaps_micros: minimizing_gaps
            .iter()
            .map(|gap| (gap * 1_000_000.0).round() as i64)
            .collect(),
        starts_checked: (GRID_STARTS + RANDOM_STARTS) as i64,
    })
}

/// Numerical periodic obstruction scout at simple-zero density 7/10.
///
/// A seven-gap period with four short and three long gaps has total span ten,
/// hence exactly density 0.7.  The mask chooses the three long positions and
/// the long gap is solved from the mean constraint.  The returned energy is
/// the full quadratic Gram defect per point, truncated after 4096 neighbors;
/// it is an explicit-configuration scout, not a directed upper certificate.
pub fn periodic_density_scout(
    long_gap_mask: i64,
    short_gap_micros: i64,
) -> Result<PeriodicDensityScoutReport, String> {
    const PERIOD: usize = 7;
    const LONG_COUNT: usize = 3;
    const NEIGHBOR_TERMS: usize = 4_096;
    const REPORT_SCALE: f64 = 1_000_000_000.0;
    const BASELINE: f64 = 0.672_500_703_679_411_6;

    if !(0..(1_i64 << PERIOD)).contains(&long_gap_mask)
        || long_gap_mask.count_ones() != LONG_COUNT as u32
        || !(800_000..=1_300_000).contains(&short_gap_micros)
    {
        return Err("periodic-density parameters are out of range".to_string());
    }
    // Seven gaps must total ten normalized units.  Keep the construction on
    // the micro-grid by requiring exact divisibility for the three long gaps.
    let long_numerator = 10_000_000_i64 - 4 * short_gap_micros;
    if long_numerator <= 0 || long_numerator % LONG_COUNT as i64 != 0 {
        return Err("periodic-density long gap is not on the micro-grid".to_string());
    }
    let long_gap_micros = long_numerator / LONG_COUNT as i64;
    let mut gaps = [0.0; PERIOD];
    for (index, gap) in gaps.iter_mut().enumerate() {
        let micros = if long_gap_mask & (1_i64 << index) != 0 {
            long_gap_micros
        } else {
            short_gap_micros
        };
        *gap = micros as f64 / 1_000_000.0;
    }

    let inv_sqrt_two = std::f64::consts::FRAC_1_SQRT_2;
    let normalization = 2.0_f64.sqrt() * inv_sqrt_two.sin();
    let sinc = |value: f64| {
        if value.abs() < 1.0e-12 {
            1.0
        } else {
            value.sin() / value
        }
    };
    let kernel_squared = |x: f64| {
        let raw = 0.5
            * (sinc(std::f64::consts::PI * x - inv_sqrt_two)
                + sinc(std::f64::consts::PI * x + inv_sqrt_two));
        let kernel = raw / normalization;
        kernel * kernel
    };

    let mut energy = 0.0;
    for start in 0..PERIOD {
        let mut distance = 0.0;
        for neighbor in 0..NEIGHBOR_TERMS {
            distance += gaps[(start + neighbor) % PERIOD];
            energy += 2.0 * kernel_squared(distance) / PERIOD as f64;
        }
    }
    if !energy.is_finite() || energy <= 0.0 {
        return Err("non-finite periodic-density energy".to_string());
    }
    let scaled = |value: f64| -> i64 { (value * REPORT_SCALE).round() as i64 };
    Ok(PeriodicDensityScoutReport {
        long_gap_mask,
        short_gap_micros,
        long_gap_micros,
        energy_per_point_scaled: scaled(energy),
        implied_simple_bound_scaled: scaled(BASELINE + energy),
        neighbor_terms: NEIGHBOR_TERMS as i64,
    })
}
