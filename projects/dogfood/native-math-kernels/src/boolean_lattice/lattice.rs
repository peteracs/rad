fn checked_masks(values: &[i64], operation: &str) -> Result<Vec<u64>, String> {
    values
        .iter()
        .map(|value| {
            u64::try_from(*value)
                .map_err(|_| format!("{operation} expects non-negative integer masks, got {value}"))
        })
        .collect()
}

fn push_member(family: &mut Vec<u64>, member: u64, inserted: bool) -> Result<(), String> {
    if !inserted {
        return Ok(());
    }
    if family.len() >= MAX_CLOSURE_MEMBERS {
        return Err(format!(
            "or_closure exceeds the {MAX_CLOSURE_MEMBERS}-member safety limit"
        ));
    }
    family.push(member);
    Ok(())
}

fn or_closure_masks(generators: &[i64]) -> Result<Vec<u64>, String> {
    let mut generators = checked_masks(generators, "or_closure")?;
    // Sparse generators expose the smallest basis early. Once a generator is
    // already in the partial closure, adjoining it is a mathematical no-op;
    // skipping that pass is decisive for overcomplete bases used by search.
    generators.sort_unstable_by_key(|value| (value.count_ones(), *value));
    generators.dedup();
    let maximum = generators.iter().fold(0u64, |joined, value| joined | value);
    let mut family = vec![0u64];

    if maximum <= DENSE_MAX_MASK {
        let mut present = vec![0u64; maximum as usize / 64 + 1];
        present[0] = 1;
        for generator in generators {
            let generator_word = generator as usize / 64;
            let generator_mask = 1u64 << (generator % 64);
            if present[generator_word] & generator_mask != 0 {
                continue;
            }
            let before = family.len();
            for index in 0..before {
                let joined = family[index] | generator;
                let word = joined as usize / 64;
                let mask = 1u64 << (joined % 64);
                let inserted = present[word] & mask == 0;
                if inserted {
                    present[word] |= mask;
                }
                push_member(&mut family, joined, inserted)?;
            }
        }
    } else {
        let mut present = HashSet::from([0u64]);
        for generator in generators {
            if present.contains(&generator) {
                continue;
            }
            let before = family.len();
            for index in 0..before {
                let joined = family[index] | generator;
                let inserted = present.insert(joined);
                push_member(&mut family, joined, inserted)?;
            }
        }
    }

    family.sort_unstable();
    Ok(family)
}

/// Return the smallest family containing `0` and every generator and closed
/// under bitwise OR.  Output is sorted and duplicate-free.
pub(crate) fn or_closure(generators: &[i64]) -> Result<Vec<i64>, String> {
    or_closure_masks(generators)?
        .into_iter()
        .map(|value| {
            i64::try_from(value)
                .map_err(|_| "or_closure result exceeds RAD's signed integer range".to_string())
        })
        .collect()
}

fn checked_width(width: i64, operation: &str) -> Result<usize, String> {
    let width = usize::try_from(width)
        .ok()
        .filter(|width| *width <= 63)
        .ok_or_else(|| format!("{operation} width must be between 0 and 63"))?;
    Ok(width)
}

fn bit_frequencies_masks(
    values: &[u64],
    width: usize,
    operation: &str,
) -> Result<Vec<i64>, String> {
    let outside_mask = if width == 63 {
        1u64 << 63
    } else {
        !0u64 << width
    };
    let mut frequencies = vec![0i64; width];
    for value in values {
        if value & outside_mask != 0 {
            return Err(format!(
                "{operation} mask {value} has a bit outside width {width}"
            ));
        }
        for (bit, count) in frequencies.iter_mut().enumerate() {
            *count += ((value >> bit) & 1) as i64;
        }
    }
    Ok(frequencies)
}

/// Count the occurrence of each bit in a list of masks.
pub(crate) fn bit_frequencies(values: &[i64], width: i64) -> Result<Vec<i64>, String> {
    let width = checked_width(width, "bit_frequencies")?;
    let values = checked_masks(values, "bit_frequencies")?;
    bit_frequencies_masks(&values, width, "bit_frequencies")
}

fn checked_rotation_width(width: i64, operation: &str) -> Result<usize, String> {
    let width = checked_width(width, operation)?;
    if !(1..=MAX_TRANSFORM_WIDTH).contains(&width) {
        return Err(format!(
            "{operation} width must be between 1 and {MAX_TRANSFORM_WIDTH}"
        ));
    }
    Ok(width)
}

/// Return the sorted orbit of a finite bit mask under cyclic coordinate shift.
pub(crate) fn bitmask_rotation_orbit(mask: i64, width: i64) -> Result<Vec<i64>, String> {
    let width = checked_rotation_width(width, "bitmask_rotation_orbit")?;
    let mask = u64::try_from(mask)
        .map_err(|_| "bitmask_rotation_orbit expects a non-negative mask".to_string())?;
    let full = (1u64 << width) - 1;
    if mask > full {
        return Err(format!(
            "bitmask_rotation_orbit mask {mask} has a bit outside width {width}"
        ));
    }
    let mut orbit = Vec::with_capacity(width);
    let mut current = mask;
    for _ in 0..width {
        orbit.push(current as i64);
        current = ((current << 1) & full) | (current >> (width - 1));
    }
    orbit.sort_unstable();
    orbit.dedup();
    Ok(orbit)
}

/// Return one least representative from every cyclic bitmask orbit.
pub(crate) fn bitmask_rotation_representatives(width: i64) -> Result<Vec<i64>, String> {
    let width = checked_rotation_width(width, "bitmask_rotation_representatives")?;
    let cube_size = 1usize << width;
    let mut seen = vec![false; cube_size];
    let mut representatives = Vec::new();
    for mask in 0..cube_size {
        if seen[mask] {
            continue;
        }
        let orbit = bitmask_rotation_orbit(mask as i64, width as i64)?;
        for member in &orbit {
            seen[*member as usize] = true;
        }
        representatives.push(orbit[0]);
    }
    Ok(representatives)
}

/// Compute closure cardinality and bit frequencies without materializing the
/// complete family as VM values. This is the compact hot path for search and
/// dataflow analyses that only need aggregate lattice statistics.
pub(crate) fn or_closure_stats(
    generators: &[i64],
    width: i64,
) -> Result<(i64, Vec<i64>, bool, i64), String> {
    let width = checked_width(width, "or_closure_stats")?;
    let family = or_closure_masks(generators)?;
    let frequencies = bit_frequencies_masks(&family, width, "or_closure_stats")?;
    let separating = (0..width).all(|left| {
        ((left + 1)..width).all(|right| {
            family
                .iter()
                .any(|member| ((member >> left) & 1) != ((member >> right) & 1))
        })
    });
    let size = i64::try_from(family.len())
        .map_err(|_| "or_closure_stats result exceeds RAD's integer range".to_string())?;
    let mut digest = blake3::Hasher::new();
    digest.update(b"rad-or-closure/v1\0");
    for member in &family {
        digest.update(&member.to_le_bytes());
    }
    let digest = digest.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    let signature = (u64::from_le_bytes(prefix) & i64::MAX as u64) as i64;
    Ok((size, frequencies, separating, signature))
}

/// Test whether a duplicate-free family is closed under bitwise OR.
pub(crate) fn is_or_closed(values: &[i64]) -> Result<bool, String> {
    let values = checked_masks(values, "is_or_closed")?;
    if values.len() > MAX_CLOSURE_MEMBERS {
        return Err(format!(
            "is_or_closed exceeds the {MAX_CLOSURE_MEMBERS}-member safety limit"
        ));
    }
    let maximum = values.iter().copied().max().unwrap_or(0);

    if maximum <= DENSE_MAX_MASK {
        let mut present = vec![false; maximum as usize + 1];
        for value in &values {
            let slot = &mut present[*value as usize];
            if *slot {
                return Ok(false);
            }
            *slot = true;
        }
        for left in &values {
            for right in &values {
                let joined = left | right;
                if joined > maximum || !present[joined as usize] {
                    return Ok(false);
                }
            }
        }
    } else {
        let present = values.iter().copied().collect::<HashSet<_>>();
        if present.len() != values.len() {
            return Ok(false);
        }
        for left in &values {
            for right in &values {
                if !present.contains(&(left | right)) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Count ordered pairs `(A, B)` in `family` whose union is absent.
///
/// A direct audit is quadratic in the family size.  On a Boolean cube the OR
/// zeta transform counts every possible union in `O(width * 2^width)`: zeta
/// transform the membership indicator, square pointwise, then Mobius invert.
/// The result is exact and is useful for measuring *distance* from closure,
/// rather than returning only a boolean verdict.
pub(crate) fn or_violation_count(values: &[i64], width: i64) -> Result<i64, String> {
    let width = usize::try_from(width)
        .ok()
        .filter(|width| *width <= MAX_TRANSFORM_WIDTH)
        .ok_or_else(|| {
            format!("or_violation_count width must be between 0 and {MAX_TRANSFORM_WIDTH}")
        })?;
    let values = checked_masks(values, "or_violation_count")?;
    let cube_size = 1usize << width;
    let mut present = vec![false; cube_size];
    let mut subset_counts = vec![0i64; cube_size];
    for value in values {
        let index = usize::try_from(value)
            .ok()
            .filter(|index| *index < cube_size)
            .ok_or_else(|| {
                format!("or_violation_count mask {value} has a bit outside width {width}")
            })?;
        if std::mem::replace(&mut present[index], true) {
            return Err(format!(
                "or_violation_count expects a duplicate-free family; mask {value} repeats"
            ));
        }
        subset_counts[index] = 1;
    }

    for bit in 0..width {
        for mask in 0..cube_size {
            if mask & (1usize << bit) != 0 {
                subset_counts[mask] += subset_counts[mask ^ (1usize << bit)];
            }
        }
    }
    for count in &mut subset_counts {
        *count = count
            .checked_mul(*count)
            .ok_or_else(|| "or_violation_count pair count overflow".to_string())?;
    }
    for bit in 0..width {
        for mask in 0..cube_size {
            if mask & (1usize << bit) != 0 {
                subset_counts[mask] -= subset_counts[mask ^ (1usize << bit)];
            }
        }
    }

    let mut violations = 0i64;
    for (mask, count) in subset_counts.into_iter().enumerate() {
        if !present[mask] {
            violations = violations
                .checked_add(count)
                .ok_or_else(|| "or_violation_count result overflow".to_string())?;
        }
    }
    Ok(violations)
}

/// Return the members that can be removed while preserving OR closure.
///
/// For a member `U`, every pair involving `U` disappears when `U` is
/// removed.  If `q(U)` family members are subsets of `U`, those account for
/// exactly `2*q(U)-1` ordered pairs with union `U`.  Removal is legal exactly
/// when there are no additional pairs of proper remaining parents.  Both
/// `q(U)` and the exact-union pair counts come from one OR zeta/Mobius pass.
pub(crate) fn or_deletable_members(values: &[i64], width: i64) -> Result<Vec<i64>, String> {
    let width = usize::try_from(width)
        .ok()
        .filter(|width| *width <= MAX_TRANSFORM_WIDTH)
        .ok_or_else(|| {
            format!("or_deletable_members width must be between 0 and {MAX_TRANSFORM_WIDTH}")
        })?;
    let values = checked_masks(values, "or_deletable_members")?;
    let cube_size = 1usize << width;
    let mut present = vec![false; cube_size];
    let mut subset_counts = vec![0i64; cube_size];
    for value in values {
        let index = usize::try_from(value)
            .ok()
            .filter(|index| *index < cube_size)
            .ok_or_else(|| {
                format!("or_deletable_members mask {value} has a bit outside width {width}")
            })?;
        if std::mem::replace(&mut present[index], true) {
            return Err(format!(
                "or_deletable_members expects a duplicate-free family; mask {value} repeats"
            ));
        }
        subset_counts[index] = 1;
    }

    for bit in 0..width {
        for mask in 0..cube_size {
            if mask & (1usize << bit) != 0 {
                subset_counts[mask] += subset_counts[mask ^ (1usize << bit)];
            }
        }
    }
    let mut union_counts = subset_counts
        .iter()
        .map(|count| {
            count
                .checked_mul(*count)
                .ok_or_else(|| "or_deletable_members pair count overflow".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for bit in 0..width {
        for mask in 0..cube_size {
            if mask & (1usize << bit) != 0 {
                union_counts[mask] -= union_counts[mask ^ (1usize << bit)];
            }
        }
    }

    if union_counts
        .iter()
        .enumerate()
        .any(|(mask, count)| !present[mask] && *count != 0)
    {
        return Err("or_deletable_members expects an OR-closed family".to_string());
    }

    Ok(present
        .iter()
        .enumerate()
        .filter_map(|(mask, is_present)| {
            if !is_present {
                return None;
            }
            let pairs_removed_with_member = 2 * subset_counts[mask] - 1;
            (union_counts[mask] == pairs_removed_with_member).then_some(mask as i64)
        })
        .collect::<Vec<_>>())
}
