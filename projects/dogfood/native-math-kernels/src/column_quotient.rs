//! Exact finite scans of Boolean incidence-column quotients.

#[derive(Debug, Clone)]
pub struct ColumnQuotientScan {
    pub generator_count: u32,
    pub column_count: u32,
    pub minimum_column_count: u32,
    pub maximum_column_count: u32,
    pub maximum_column_weight: u32,
    pub minimum_family_size: u32,
    pub maximum_family_size: u32,
    pub pattern_count: u32,
    pub labelled_configurations: u64,
    pub covered_labelled_configurations: u64,
    pub symmetry_orbits: u64,
    pub frontier_orbits: u64,
    pub minimum_margin: i64,
    pub best_columns: Vec<u32>,
    pub best_family_size: u32,
    pub best_frequencies: Vec<u32>,
    pub counterexample_columns: Vec<u32>,
    pub counterexample_family_size: u32,
    pub counterexample_frequencies: Vec<u32>,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct ColumnQuotientProfile {
    pub generator_count: u32,
    pub columns: Vec<u32>,
    pub family: Vec<u64>,
    pub frequencies: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct ColumnQuotientMutationLane {
    pub best: ColumnQuotientProfile,
    pub evaluated: u64,
    pub in_window: u64,
    pub best_window_margin: i64,
    pub window_counts: Vec<u64>,
    pub window_minimum_margins: Vec<i64>,
    pub excess_sum: i64,
    pub abundant_count: u32,
    pub connected: bool,
    pub has_maximum_weight_column: bool,
}

pub fn profile(columns: &[u32], generator_count: u32) -> Result<ColumnQuotientProfile, String> {
    if !(1..=16).contains(&generator_count) {
        return Err("generator_count must lie in 1..=16".to_string());
    }
    if columns.is_empty() || columns.len() > 63 {
        return Err("column count must lie in 1..=63".to_string());
    }
    let limit = 1u32 << generator_count;
    let mut canonical_columns = columns.to_vec();
    canonical_columns.sort_unstable();
    if canonical_columns[0] == 0
        || canonical_columns.last().copied().unwrap_or(0) >= limit
        || canonical_columns.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err("columns must be distinct nonzero generator-incidence masks".to_string());
    }
    let mut family = (0..limit)
        .map(|selected| {
            canonical_columns
                .iter()
                .enumerate()
                .fold(0u64, |member, (coordinate, column)| {
                    if selected & column != 0 {
                        member | (1u64 << coordinate)
                    } else {
                        member
                    }
                })
        })
        .collect::<Vec<_>>();
    family.sort_unstable();
    family.dedup();
    let frequencies = (0..canonical_columns.len())
        .map(|coordinate| {
            family
                .iter()
                .filter(|member| **member & (1u64 << coordinate) != 0)
                .count() as u32
        })
        .collect();
    Ok(ColumnQuotientProfile {
        generator_count,
        columns: canonical_columns,
        family,
        frequencies,
    })
}

fn next_random(state: &mut u64, upper: usize) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state as usize) % upper
}

fn connected_support(columns: &[u32], generator_count: u32) -> bool {
    let full = (1u32 << generator_count) - 1;
    let mut reached = 1u32;
    loop {
        let previous = reached;
        for column in columns {
            if column & reached != 0 {
                reached |= column;
            }
        }
        if reached == previous {
            return reached == full;
        }
    }
}

fn balance_metrics(profile: &ColumnQuotientProfile) -> (i64, i64, u32) {
    let family_size = profile.family.len() as i64;
    let margin = profile
        .frequencies
        .iter()
        .map(|frequency| 2 * i64::from(*frequency) - family_size)
        .max()
        .unwrap_or(0);
    let mut excess_sum = 0i64;
    let mut abundant_count = 0u32;
    for frequency in &profile.frequencies {
        let excess = 2 * i64::from(*frequency) - family_size + 1;
        if excess > 0 {
            excess_sum += excess;
            abundant_count += 1;
        }
    }
    (margin, excess_sum, abundant_count)
}

fn window_distance(size: usize, minimum: usize, maximum: usize) -> usize {
    if size < minimum {
        minimum - size
    } else {
        size.saturating_sub(maximum)
    }
}

fn mutation_score(
    profile: &ColumnQuotientProfile,
    maximum_column_weight: u32,
    minimum_family_size: usize,
    maximum_family_size: usize,
) -> (u8, usize, i64, i64, u32, usize) {
    let connected = connected_support(&profile.columns, profile.generator_count);
    let has_maximum = profile
        .columns
        .iter()
        .any(|column| column.count_ones() == maximum_column_weight);
    let (margin, excess, abundant) = balance_metrics(profile);
    (
        u8::from(!(connected && has_maximum)),
        window_distance(
            profile.family.len(),
            minimum_family_size,
            maximum_family_size,
        ),
        margin,
        excess,
        abundant,
        profile.family.len(),
    )
}

pub fn search_mutations(
    base_columns: &[u32],
    generator_count: u32,
    maximum_column_weight: u32,
    trials: u32,
    seed: u64,
    minimum_family_size: u32,
    maximum_family_size: u32,
) -> Result<ColumnQuotientMutationLane, String> {
    if maximum_column_weight == 0 || maximum_column_weight > generator_count {
        return Err("maximum_column_weight is invalid".to_string());
    }
    if trials == 0 || trials > 1_000_000 {
        return Err("trials must lie in 1..=1000000".to_string());
    }
    if minimum_family_size == 0 || minimum_family_size > maximum_family_size {
        return Err("family-size interval is invalid".to_string());
    }
    let patterns = (1..(1u32 << generator_count))
        .filter(|pattern| pattern.count_ones() <= maximum_column_weight)
        .collect::<Vec<_>>();
    if base_columns.len() >= patterns.len()
        || base_columns
            .iter()
            .any(|column| column.count_ones() > maximum_column_weight)
    {
        return Err("base columns do not fit the mutable pattern class".to_string());
    }
    let minimum = minimum_family_size as usize;
    let maximum = maximum_family_size as usize;
    let mut best = profile(base_columns, generator_count)?;
    let mut best_score = mutation_score(&best, maximum_column_weight, minimum, maximum);
    let mut rng = seed.max(1);
    let mut in_window = 0u64;
    let mut best_window_margin = i64::MAX;
    let mut window_counts = vec![0u64; maximum - minimum + 1];
    let mut window_minimum_margins = vec![i64::MAX; maximum - minimum + 1];
    for _ in 0..trials {
        let mut columns = base_columns.to_vec();
        let edits = 1 + next_random(&mut rng, 4.min(columns.len()));
        for _ in 0..edits {
            let coordinate = next_random(&mut rng, columns.len());
            loop {
                let replacement = patterns[next_random(&mut rng, patterns.len())];
                if !columns.contains(&replacement) {
                    columns[coordinate] = replacement;
                    break;
                }
            }
        }
        let candidate = profile(&columns, generator_count)?;
        let score = mutation_score(&candidate, maximum_column_weight, minimum, maximum);
        if score.0 == 0 && score.1 == 0 {
            in_window += 1;
            best_window_margin = best_window_margin.min(score.2);
            let window_index = candidate.family.len() - minimum;
            window_counts[window_index] += 1;
            window_minimum_margins[window_index] =
                window_minimum_margins[window_index].min(score.2);
        }
        if score < best_score {
            best = candidate;
            best_score = score;
        }
    }
    let (_, excess_sum, abundant_count) = balance_metrics(&best);
    Ok(ColumnQuotientMutationLane {
        connected: connected_support(&best.columns, generator_count),
        has_maximum_weight_column: best
            .columns
            .iter()
            .any(|column| column.count_ones() == maximum_column_weight),
        best,
        evaluated: u64::from(trials),
        in_window,
        best_window_margin: if in_window == 0 {
            0
        } else {
            best_window_margin
        },
        window_counts,
        window_minimum_margins: window_minimum_margins
            .into_iter()
            .map(|margin| if margin == i64::MAX { 0 } else { margin })
            .collect(),
        excess_sum,
        abundant_count,
    })
}

fn binomial(n: u32, k: u32) -> u64 {
    let k = k.min(n - k);
    (0..k).fold(1u64, |value, index| {
        value * u64::from(n - index) / u64::from(index + 1)
    })
}

fn permutations(size: usize) -> Vec<Vec<usize>> {
    fn visit(prefix: &mut Vec<usize>, remaining: &mut Vec<usize>, result: &mut Vec<Vec<usize>>) {
        if remaining.is_empty() {
            result.push(prefix.clone());
            return;
        }
        for index in 0..remaining.len() {
            let value = remaining.remove(index);
            prefix.push(value);
            visit(prefix, remaining, result);
            prefix.pop();
            remaining.insert(index, value);
        }
    }

    let mut result = Vec::new();
    visit(&mut Vec::new(), &mut (0..size).collect(), &mut result);
    result
}

fn permute_pattern(pattern: u32, permutation: &[usize]) -> u32 {
    let mut result = 0u32;
    for (source, &target) in permutation.iter().enumerate() {
        if pattern & (1u32 << source) != 0 {
            result |= 1u32 << target;
        }
    }
    result
}

fn next_combination(value: u32) -> u32 {
    let lowest = value & value.wrapping_neg();
    let ripple = value.wrapping_add(lowest);
    (((value ^ ripple) >> 2) / lowest) | ripple
}

fn bit_is_set(bits: &[u64], index: u32) -> bool {
    bits[(index >> 6) as usize] & (1u64 << (index & 63)) != 0
}

fn set_bit(bits: &mut [u64], index: u32) {
    bits[(index >> 6) as usize] |= 1u64 << (index & 63);
}

fn transform_configuration(configuration: u32, mapping: &[u8]) -> u32 {
    let mut source = configuration;
    let mut result = 0u32;
    while source != 0 {
        let index = source.trailing_zeros() as usize;
        result |= 1u32 << mapping[index];
        source &= source - 1;
    }
    result
}

fn evaluate(configuration: u32, hit_masks: &[u32], patterns: &[u32]) -> (u32, Vec<u32>, i64) {
    let mut outputs = hit_masks
        .iter()
        .map(|hit_mask| hit_mask & configuration)
        .collect::<Vec<_>>();
    outputs.sort_unstable();
    outputs.dedup();
    let mut frequencies = vec![0u32; patterns.len()];
    for &output in &outputs {
        let mut present = output;
        while present != 0 {
            let index = present.trailing_zeros() as usize;
            frequencies[index] += 1;
            present &= present - 1;
        }
    }
    let selected_frequencies = (0..patterns.len())
        .filter(|index| configuration & (1u32 << index) != 0)
        .map(|index| frequencies[index])
        .collect::<Vec<_>>();
    let family_size = outputs.len() as u32;
    let maximum = selected_frequencies.iter().copied().max().unwrap_or(0);
    (
        family_size,
        selected_frequencies,
        i64::from(2 * maximum) - i64::from(family_size),
    )
}

pub fn scan(
    generator_count: u32,
    column_count: u32,
    maximum_column_weight: u32,
    minimum_family_size: u32,
    maximum_family_size: u32,
) -> Result<ColumnQuotientScan, String> {
    scan_range(
        generator_count,
        column_count,
        column_count,
        maximum_column_weight,
        minimum_family_size,
        maximum_family_size,
    )
}

pub fn scan_range(
    generator_count: u32,
    minimum_column_count: u32,
    maximum_column_count: u32,
    maximum_column_weight: u32,
    minimum_family_size: u32,
    maximum_family_size: u32,
) -> Result<ColumnQuotientScan, String> {
    if !(1..=7).contains(&generator_count) {
        return Err("generator_count must lie in 1..=7".to_string());
    }
    if !(1..=2).contains(&maximum_column_weight) {
        return Err("maximum_column_weight must lie in 1..=2".to_string());
    }
    if minimum_family_size < 2 || minimum_family_size > maximum_family_size {
        return Err("family-size interval is invalid".to_string());
    }
    let patterns = (1..(1u32 << generator_count))
        .filter(|pattern| pattern.count_ones() <= maximum_column_weight)
        .collect::<Vec<_>>();
    if minimum_column_count == 0
        || minimum_column_count > maximum_column_count
        || maximum_column_count as usize > patterns.len()
    {
        return Err("column-count interval exceeds the available distinct patterns".to_string());
    }
    if patterns.len() > 28 {
        return Err("exact symmetry scan supports at most 28 column patterns".to_string());
    }
    let mut pattern_index = vec![u8::MAX; 1usize << generator_count];
    for (index, &pattern) in patterns.iter().enumerate() {
        pattern_index[pattern as usize] = index as u8;
    }
    let mappings = permutations(generator_count as usize)
        .into_iter()
        .map(|permutation| {
            patterns
                .iter()
                .map(|&pattern| {
                    let transformed = permute_pattern(pattern, &permutation);
                    pattern_index[transformed as usize]
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if mappings.iter().flatten().any(|&index| index == u8::MAX) {
        return Err("column pattern class is not permutation invariant".to_string());
    }
    let hit_masks = (0..(1u32 << generator_count))
        .map(|selected| {
            patterns
                .iter()
                .enumerate()
                .fold(0u32, |mask, (index, pattern)| {
                    if selected & pattern != 0 {
                        mask | (1u32 << index)
                    } else {
                        mask
                    }
                })
        })
        .collect::<Vec<_>>();

    let configuration_space = 1usize << patterns.len();
    let mut seen = vec![0u64; configuration_space.div_ceil(64)];
    let limit = 1u32 << patterns.len();
    let labelled_configurations = (minimum_column_count..=maximum_column_count)
        .map(|column_count| binomial(patterns.len() as u32, column_count))
        .sum();
    let mut symmetry_orbits = 0u64;
    let mut covered_labelled_configurations = 0u64;
    let mut frontier_orbits = 0u64;
    let mut minimum_margin = i64::MAX;
    let mut best_configuration = 0u32;
    let mut best_family_size = 0u32;
    let mut best_frequencies = Vec::new();
    let mut counterexample_configuration = 0u32;
    let mut counterexample_family_size = 0u32;
    let mut counterexample_frequencies = Vec::new();
    let mut hasher = blake3::Hasher::new();

    for column_count in minimum_column_count..=maximum_column_count {
        let mut configuration = (1u32 << column_count) - 1;
        while configuration < limit {
            if !bit_is_set(&seen, configuration) {
                symmetry_orbits += 1;
                for mapping in &mappings {
                    let image = transform_configuration(configuration, mapping);
                    if !bit_is_set(&seen, image) {
                        set_bit(&mut seen, image);
                        covered_labelled_configurations += 1;
                    }
                }
                let (family_size, frequencies, margin) =
                    evaluate(configuration, &hit_masks, &patterns);
                hasher.update(&configuration.to_le_bytes());
                hasher.update(&family_size.to_le_bytes());
                hasher.update(&margin.to_le_bytes());
                if (minimum_family_size..=maximum_family_size).contains(&family_size) {
                    frontier_orbits += 1;
                    if margin < minimum_margin {
                        minimum_margin = margin;
                        best_configuration = configuration;
                        best_family_size = family_size;
                        best_frequencies.clone_from(&frequencies);
                    }
                    if margin < 0 && counterexample_configuration == 0 {
                        counterexample_configuration = configuration;
                        counterexample_family_size = family_size;
                        counterexample_frequencies.clone_from(&frequencies);
                    }
                }
            }
            let next = next_combination(configuration);
            if next <= configuration {
                break;
            }
            configuration = next;
        }
    }
    if frontier_orbits == 0 {
        minimum_margin = 0;
    }
    if covered_labelled_configurations != labelled_configurations {
        return Err(format!(
            "symmetry scan covered {covered_labelled_configurations} labelled configurations; expected {labelled_configurations}"
        ));
    }
    let decode_columns = |configuration: u32| {
        patterns
            .iter()
            .enumerate()
            .filter(|(index, _)| configuration & (1u32 << index) != 0)
            .map(|(_, &pattern)| pattern)
            .collect::<Vec<_>>()
    };
    Ok(ColumnQuotientScan {
        generator_count,
        column_count: if minimum_column_count == maximum_column_count {
            minimum_column_count
        } else {
            0
        },
        minimum_column_count,
        maximum_column_count,
        maximum_column_weight,
        minimum_family_size,
        maximum_family_size,
        pattern_count: patterns.len() as u32,
        labelled_configurations,
        covered_labelled_configurations,
        symmetry_orbits,
        frontier_orbits,
        minimum_margin,
        best_columns: decode_columns(best_configuration),
        best_family_size,
        best_frequencies,
        counterexample_columns: decode_columns(counterexample_configuration),
        counterexample_family_size,
        counterexample_frequencies,
        signature: hasher.finalize().to_hex().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_small_column_quotients_against_labelled_count() {
        let profile = scan(3, 3, 2, 2, 8).unwrap();
        assert_eq!(profile.pattern_count, 6);
        assert_eq!(profile.labelled_configurations, 20);
        assert_eq!(profile.covered_labelled_configurations, 20);
        assert!(profile.symmetry_orbits > 0);
        assert!(profile.minimum_margin >= 0);
        assert!(profile.counterexample_columns.is_empty());
    }

    #[test]
    fn profiles_boolean_singleton_columns() {
        let profile = profile(&[1, 2, 4], 3).unwrap();
        assert_eq!(profile.family.len(), 8);
        assert_eq!(profile.frequencies, vec![4, 4, 4]);
    }

    #[test]
    fn seeded_mutation_lanes_are_reproducible() {
        let left = search_mutations(&[1, 2, 4, 3], 3, 2, 64, 1979, 2, 8).unwrap();
        let right = search_mutations(&[1, 2, 4, 3], 3, 2, 64, 1979, 2, 8).unwrap();
        assert_eq!(left.best.columns, right.best.columns);
        assert_eq!(left.best.family, right.best.family);
        assert_eq!(left.best.frequencies, right.best.frequencies);
        assert_eq!(left.evaluated, 64);
    }

    #[test]
    fn quotient_evaluation_is_invariant_under_generator_permutation() {
        let generator_count = 5usize;
        let patterns = (1..(1u32 << generator_count))
            .filter(|pattern| pattern.count_ones() <= 2)
            .collect::<Vec<_>>();
        let mut pattern_index = vec![u8::MAX; 1usize << generator_count];
        for (index, &pattern) in patterns.iter().enumerate() {
            pattern_index[pattern as usize] = index as u8;
        }
        let permutation = vec![2, 4, 1, 0, 3];
        let mapping = patterns
            .iter()
            .map(|&pattern| pattern_index[permute_pattern(pattern, &permutation) as usize])
            .collect::<Vec<_>>();
        let hit_masks = (0..(1u32 << generator_count))
            .map(|selected| {
                patterns
                    .iter()
                    .enumerate()
                    .fold(0u32, |mask, (index, pattern)| {
                        if selected & pattern != 0 {
                            mask | (1u32 << index)
                        } else {
                            mask
                        }
                    })
            })
            .collect::<Vec<_>>();
        let configuration = [0usize, 2, 4, 7, 9, 12]
            .into_iter()
            .fold(0u32, |mask, index| mask | (1u32 << index));
        let transformed = transform_configuration(configuration, &mapping);
        let (family_size, mut frequencies, margin) = evaluate(configuration, &hit_masks, &patterns);
        let (transformed_size, mut transformed_frequencies, transformed_margin) =
            evaluate(transformed, &hit_masks, &patterns);
        frequencies.sort_unstable();
        transformed_frequencies.sort_unstable();
        assert_eq!(family_size, transformed_size);
        assert_eq!(frequencies, transformed_frequencies);
        assert_eq!(margin, transformed_margin);
    }
}
