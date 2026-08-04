pub(crate) struct OrDeletionState {
    pub family_size: i64,
    pub family_frequencies: Vec<i64>,
    pub deletion_surpluses: Vec<i64>,
    pub pair_biases: Vec<i64>,
    pub deletable_members: Vec<i64>,
    pub effective_deletable_members: Vec<i64>,
    pub separating: bool,
}

pub(crate) struct OrDeletionRollout {
    pub deleted: Vec<i64>,
    pub state: OrDeletionState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrDeletionObjective {
    MaxMin,
    MaxMinLowPeak,
    MinPairBias,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrExchangeAcceptance {
    NonWorsening,
    Exploratory,
}

/// Incremental state for deleting members from an OR-closed Boolean family.
///
/// `subset_counts[u]` is the number of surviving subsets of `u`, while
/// `union_counts[u]` is the number of ordered surviving pairs whose OR is
/// exactly `u`.  Removing `x` changes the latter only for pairs containing
/// `x`, so one dense scan replaces a complete zeta/Mobius rebuild.
#[derive(Clone)]
struct IncrementalOrDeletion {
    width: usize,
    cube_size: usize,
    present: Vec<bool>,
    subset_counts: Vec<i64>,
    union_counts: Vec<i64>,
    family_frequencies: Vec<i64>,
    deletion_frequencies: Vec<i64>,
    separation_witnesses: Vec<i64>,
    deleted: Vec<i64>,
    union_delta: Vec<i64>,
    touched_unions: Vec<usize>,
}

impl IncrementalOrDeletion {
    fn new(deleted: &[i64], width: i64) -> Result<Self, String> {
        let width = usize::try_from(width)
            .ok()
            .filter(|width| *width <= MAX_TRANSFORM_WIDTH)
            .ok_or_else(|| {
                format!("or_deletion_rollout width must be between 0 and {MAX_TRANSFORM_WIDTH}")
            })?;
        let deleted_masks = checked_masks(deleted, "or_deletion_rollout")?;
        let cube_size = 1usize << width;
        let mut present = vec![true; cube_size];
        let mut deletion_frequencies = vec![0i64; width];
        for value in &deleted_masks {
            let index = usize::try_from(*value)
                .ok()
                .filter(|index| *index < cube_size)
                .ok_or_else(|| {
                    format!("or_deletion_rollout mask {value} has a bit outside width {width}")
                })?;
            if !std::mem::replace(&mut present[index], false) {
                return Err(format!(
                    "or_deletion_rollout expects duplicate-free deletions; mask {value} repeats"
                ));
            }
            for (bit, count) in deletion_frequencies.iter_mut().enumerate() {
                *count += ((value >> bit) & 1) as i64;
            }
        }

        let mut subset_counts = present
            .iter()
            .map(|is_present| i64::from(*is_present))
            .collect::<Vec<_>>();
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
                    .ok_or_else(|| "or_deletion_rollout pair count overflow".to_string())
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
            return Err(
                "or_deletion_rollout deletions do not leave an OR-closed complement".to_string(),
            );
        }

        let family_frequencies = deletion_frequencies
            .iter()
            .map(|count| (cube_size / 2) as i64 - count)
            .collect::<Vec<_>>();
        let mut separation_witnesses = vec![0i64; width * width];
        for (member, is_present) in present.iter().enumerate() {
            if !is_present {
                continue;
            }
            for left in 0..width {
                for right in (left + 1)..width {
                    if ((member >> left) & 1) != ((member >> right) & 1) {
                        separation_witnesses[left * width + right] += 1;
                    }
                }
            }
        }

        Ok(Self {
            width,
            cube_size,
            present,
            subset_counts,
            union_counts,
            family_frequencies,
            deletion_frequencies,
            separation_witnesses,
            deleted: deleted.to_vec(),
            union_delta: vec![0; cube_size],
            touched_unions: Vec::with_capacity(cube_size),
        })
    }

    fn family_size(&self) -> i64 {
        (self.cube_size - self.deleted.len()) as i64
    }

    fn is_deletable(&self, member: usize) -> bool {
        self.present[member] && self.union_counts[member] == 2 * self.subset_counts[member] - 1
    }

    fn preserves_effective_universe(&self, member: usize) -> bool {
        let coverage = (0..self.width)
            .all(|bit| member & (1usize << bit) == 0 || self.family_frequencies[bit] > 1);
        let separation = (0..self.width).all(|left| {
            ((left + 1)..self.width).all(|right| {
                ((member >> left) & 1) == ((member >> right) & 1)
                    || self.separation_witnesses[left * self.width + right] > 1
            })
        });
        coverage && separation
    }

    fn frontiers(&self) -> (Vec<i64>, Vec<i64>) {
        let mut deletable = Vec::new();
        let mut effective = Vec::new();
        for member in 0..self.cube_size {
            if !self.is_deletable(member) {
                continue;
            }
            deletable.push(member as i64);
            if self.preserves_effective_universe(member) {
                effective.push(member as i64);
            }
        }
        (deletable, effective)
    }

    fn deletion_surpluses(&self) -> Vec<i64> {
        let deleted_size = self.deleted.len() as i64;
        self.deletion_frequencies
            .iter()
            .map(|frequency| 2 * frequency - deleted_size)
            .collect()
    }

    fn coordinate_profile_after(&self, member: usize) -> Vec<i64> {
        let mut profile = self
            .deletion_surpluses()
            .into_iter()
            .enumerate()
            .map(|(bit, surplus)| surplus + if member & (1usize << bit) != 0 { 1 } else { -1 })
            .collect::<Vec<_>>();
        profile.sort_unstable();
        profile
    }

    fn pair_profile_after(&self, member: usize) -> Vec<i64> {
        let mut profile = Vec::with_capacity(self.width * (self.width - 1) / 2);
        let next_size = self.family_size() - 1;
        for left in 0..self.width {
            for right in (left + 1)..self.width {
                let left_frequency =
                    self.family_frequencies[left] - i64::from(member & (1usize << left) != 0);
                let right_frequency =
                    self.family_frequencies[right] - i64::from(member & (1usize << right) != 0);
                let next_bias = left_frequency + right_frequency - next_size;
                // Max-min machinery can minimize the largest bias by
                // maximizing the smallest negated bias.
                profile.push(-next_bias);
            }
        }
        profile.sort_unstable();
        profile.extend(self.coordinate_profile_after(member));
        profile
    }

    fn profile_after(&self, member: usize, objective: OrDeletionObjective) -> Vec<i64> {
        match objective {
            OrDeletionObjective::MaxMin | OrDeletionObjective::MaxMinLowPeak => {
                self.coordinate_profile_after(member)
            }
            OrDeletionObjective::MinPairBias => self.pair_profile_after(member),
        }
    }

    fn remove(&mut self, member: usize) -> Result<(), String> {
        if !self.is_deletable(member) {
            return Err(format!(
                "or_deletion_rollout member {member} is not deletable"
            ));
        }

        self.touched_unions.clear();
        for other in 0..self.cube_size {
            if !self.present[other] {
                continue;
            }
            let joined = member | other;
            if self.union_delta[joined] == 0 {
                self.touched_unions.push(joined);
            }
            self.union_delta[joined] += if other == member { 1 } else { 2 };
        }
        for joined in self.touched_unions.drain(..) {
            self.union_counts[joined] -= self.union_delta[joined];
            self.union_delta[joined] = 0;
        }

        let complement = (self.cube_size - 1) ^ member;
        let mut subset = complement;
        loop {
            self.subset_counts[member | subset] -= 1;
            if subset == 0 {
                break;
            }
            subset = (subset - 1) & complement;
        }

        self.present[member] = false;
        self.deleted.push(member as i64);
        for bit in 0..self.width {
            if member & (1usize << bit) != 0 {
                self.family_frequencies[bit] -= 1;
                self.deletion_frequencies[bit] += 1;
            }
        }
        for left in 0..self.width {
            for right in (left + 1)..self.width {
                if ((member >> left) & 1) != ((member >> right) & 1) {
                    self.separation_witnesses[left * self.width + right] -= 1;
                }
            }
        }
        if self.union_counts[member] != 0 {
            return Err(format!(
                "or_deletion_rollout internal witness mismatch after deleting {member}"
            ));
        }
        Ok(())
    }

    fn snapshot(&self) -> OrDeletionState {
        let (deletable_members, effective_deletable_members) = self.frontiers();
        let separating = (0..self.width).all(|left| {
            ((left + 1)..self.width)
                .all(|right| self.separation_witnesses[left * self.width + right] > 0)
        });
        OrDeletionState {
            family_size: self.family_size(),
            family_frequencies: self.family_frequencies.clone(),
            deletion_surpluses: self.deletion_surpluses(),
            pair_biases: pair_biases_from_frequencies(&self.family_frequencies, self.family_size()),
            deletable_members,
            effective_deletable_members,
            separating,
        }
    }

    /// Return the members that must be restored when `member` is adjoined to
    /// the current OR-closed family.
    ///
    /// If `F` is OR-closed, then `F union {x | a : a in F}` is the least
    /// OR-closed family containing both `F` and `x`: joining two new members
    /// gives `x | (a | b)`, and `a | b` is already in `F`.
    fn closure_additions(&self, member: usize) -> Vec<usize> {
        let mut marked = vec![false; self.cube_size];
        let mut additions = Vec::new();
        for existing in 0..self.cube_size {
            if !self.present[existing] {
                continue;
            }
            let joined = member | existing;
            if !self.present[joined] && !marked[joined] {
                marked[joined] = true;
                additions.push(joined);
            }
        }
        if !self.present[member] && !marked[member] {
            additions.push(member);
        }
        additions.sort_unstable();
        additions
    }
}

fn next_random(seed: &mut u64, upper: usize) -> usize {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed as usize) % upper
}

/// Follow a deterministic, explicitly seeded deletion rollout.
///
/// The objective is generic max-min balancing over coordinate deletion
/// surpluses. `MaxMinLowPeak` uses the smallest maximum surplus as the first
/// tie-breaker, which keeps all coordinates represented more uniformly.
pub(crate) fn or_deletion_rollout(
    deleted: &[i64],
    width: i64,
    steps: usize,
    choices_per_step: usize,
    seed: u64,
    objective: OrDeletionObjective,
    minimum_density_per_mille: i64,
) -> Result<OrDeletionRollout, String> {
    if !(0..=1000).contains(&minimum_density_per_mille) {
        return Err("minimum density must be between 0 and 1000 per mille".to_string());
    }
    let mut machine = IncrementalOrDeletion::new(deleted, width)?;
    let mut rng = seed.max(1);
    for _ in 0..steps {
        let (_, effective) = machine.frontiers();
        let candidates = effective
            .into_iter()
            .filter(|member| *member != 0)
            .map(|member| member as usize)
            .filter(|member| {
                (0..machine.width).all(|bit| {
                    let next_frequency =
                        machine.family_frequencies[bit] - i64::from(member & (1usize << bit) != 0);
                    next_frequency * 1000 >= (machine.family_size() - 1) * minimum_density_per_mille
                })
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            break;
        }

        let start = next_random(&mut rng, candidates.len());
        let trials = choices_per_step.max(1).min(candidates.len());
        let mut best_member = candidates[start];
        let mut best_profile = machine.profile_after(best_member, objective);
        for offset in 1..trials {
            let member = candidates[(start + offset) % candidates.len()];
            let profile = machine.profile_after(member, objective);
            let better_minimum = profile[0] > best_profile[0];
            let same_minimum = profile[0] == best_profile[0];
            let better_peak = profile[profile.len() - 1] < best_profile[best_profile.len() - 1];
            let same_peak = profile[profile.len() - 1] == best_profile[best_profile.len() - 1];
            let better_profile = profile > best_profile;
            let better = better_minimum
                || (same_minimum && objective == OrDeletionObjective::MaxMinLowPeak && better_peak)
                || (same_minimum
                    && (objective != OrDeletionObjective::MaxMinLowPeak || same_peak)
                    && better_profile);
            if better {
                best_member = member;
                best_profile = profile;
            }
        }
        machine.remove(best_member)?;
    }
    Ok(OrDeletionRollout {
        deleted: machine.deleted.clone(),
        state: machine.snapshot(),
    })
}

/// Apply one explicitly selected legal deletion and return the exact next
/// profile. This is the branch-expansion primitive used by exhaustive or beam
/// search; policy remains in the caller.
pub(crate) fn or_apply_deletion(
    deleted: &[i64],
    width: i64,
    member: i64,
) -> Result<OrDeletionRollout, String> {
    let mut machine = IncrementalOrDeletion::new(deleted, width)?;
    let member = usize::try_from(member)
        .ok()
        .filter(|member| *member < machine.cube_size)
        .ok_or_else(|| "or_apply_deletion member lies outside the cube".to_string())?;
    if !machine.preserves_effective_universe(member) {
        return Err(format!(
            "or_apply_deletion member {member} does not preserve the effective universe"
        ));
    }
    machine.remove(member)?;
    Ok(OrDeletionRollout {
        deleted: machine.deleted.clone(),
        state: machine.snapshot(),
    })
}

/// Analyze the OR-closed complement of a sparse deletion list.
///
/// This is the COW-friendly form of [`or_deletable_members`]: speculative
/// worlds retain only their deletion path, while the dense Boolean cube stays
/// inside one native transform instead of becoming thousands of VM values per
/// fork.
pub(crate) fn or_deletion_state(deleted: &[i64], width: i64) -> Result<OrDeletionState, String> {
    let width = usize::try_from(width)
        .ok()
        .filter(|width| *width <= MAX_TRANSFORM_WIDTH)
        .ok_or_else(|| {
            format!("or_deletion_state width must be between 0 and {MAX_TRANSFORM_WIDTH}")
        })?;
    let deleted = checked_masks(deleted, "or_deletion_state")?;
    let cube_size = 1usize << width;
    let mut family_present = vec![true; cube_size];
    let mut deleted_frequencies = vec![0i64; width];
    for value in deleted {
        let index = usize::try_from(value)
            .ok()
            .filter(|index| *index < cube_size)
            .ok_or_else(|| {
                format!("or_deletion_state mask {value} has a bit outside width {width}")
            })?;
        if !std::mem::replace(&mut family_present[index], false) {
            return Err(format!(
                "or_deletion_state expects duplicate-free deletions; mask {value} repeats"
            ));
        }
        for (bit, frequency) in deleted_frequencies.iter_mut().enumerate() {
            *frequency += ((value >> bit) & 1) as i64;
        }
    }

    let mut subset_counts = family_present
        .iter()
        .map(|present| i64::from(*present))
        .collect::<Vec<_>>();
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
                .ok_or_else(|| "or_deletion_state pair count overflow".to_string())
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
        .any(|(mask, count)| !family_present[mask] && *count != 0)
    {
        return Err("or_deletion_state deletions do not leave an OR-closed complement".to_string());
    }

    let deletable_members: Vec<i64> = family_present
        .iter()
        .enumerate()
        .filter_map(|(mask, present)| {
            if !present {
                return None;
            }
            (union_counts[mask] == 2 * subset_counts[mask] - 1).then_some(mask as i64)
        })
        .collect();
    let deleted_size = i64::try_from(family_present.iter().filter(|present| !**present).count())
        .map_err(|_| "or_deletion_state deletion count exceeds RAD integer range".to_string())?;
    let family_size = i64::try_from(cube_size)
        .map_err(|_| "or_deletion_state cube size exceeds RAD integer range".to_string())?
        - deleted_size;
    let half_cube = if width == 0 {
        0
    } else {
        (cube_size / 2) as i64
    };
    let family_frequencies: Vec<i64> = deleted_frequencies
        .iter()
        .map(|frequency| half_cube - frequency)
        .collect();
    let mut separation_witnesses = vec![0i64; width * width];
    for (member, present) in family_present.iter().enumerate() {
        if !present {
            continue;
        }
        for left in 0..width {
            for right in (left + 1)..width {
                if ((member >> left) & 1) != ((member >> right) & 1) {
                    separation_witnesses[left * width + right] += 1;
                }
            }
        }
    }
    let deletion_surpluses = deleted_frequencies
        .into_iter()
        .map(|frequency| 2 * frequency - deleted_size)
        .collect();
    let separating = (0..width).all(|left| {
        ((left + 1)..width).all(|right| {
            family_present.iter().enumerate().any(|(member, present)| {
                *present && ((member >> left) & 1) != ((member >> right) & 1)
            })
        })
    });
    // A removal preserves the effective labelled universe exactly when it
    // removes neither the last carrier of a coordinate nor the unique member
    // distinguishing a pair of coordinates.  This filters a closure-legal
    // frontier without rerunning the lattice transform per candidate.
    let effective_deletable_members = deletable_members
        .iter()
        .copied()
        .filter(|member| {
            let preserves_coverage =
                (0..width).all(|bit| member & (1i64 << bit) == 0 || family_frequencies[bit] > 1);
            let preserves_separation = (0..width).all(|left| {
                ((left + 1)..width).all(|right| {
                    ((member >> left) & 1) == ((member >> right) & 1)
                        || separation_witnesses[left * width + right] > 1
                })
            });
            preserves_coverage && preserves_separation
        })
        .collect();

    let pair_biases = pair_biases_from_frequencies(&family_frequencies, family_size);
    Ok(OrDeletionState {
        family_size,
        family_frequencies,
        deletion_surpluses,
        pair_biases,
        deletable_members,
        effective_deletable_members,
        separating,
    })
}
