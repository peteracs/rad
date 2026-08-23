#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct CyclicPairLaneProfile {
    pub(crate) symmetry_classes: i64,
    pub(crate) evaluated: i64,
    pub(crate) equality_families: i64,
    pub(crate) diagonal_equality_families: i64,
    pub(crate) off_diagonal_equality_families: i64,
    pub(crate) full_cube_families: i64,
    pub(crate) equality_non_full_families: i64,
    pub(crate) positive_families: i64,
    pub(crate) negative_families: i64,
    pub(crate) separating_families: i64,
    pub(crate) best_generators: Vec<i64>,
    pub(crate) best_frequencies: Vec<i64>,
    pub(crate) best_members: i64,
    pub(crate) best_max_frequency: i64,
    pub(crate) best_margin: i64,
    pub(crate) best_signature: i64,
}

struct CyclicOrbit {
    generators: Vec<u64>,
    separated_pairs: Vec<u64>,
}

struct DenseClosureScratch {
    family: Vec<u64>,
    present: Vec<u64>,
    total_incidence: i64,
}

impl DenseClosureScratch {
    fn new(cube_size: usize) -> Self {
        Self {
            family: Vec::with_capacity(cube_size),
            present: vec![0; cube_size.div_ceil(64)],
            total_incidence: 0,
        }
    }

    fn reset(&mut self) {
        self.family.clear();
        self.present.fill(0);
        self.family.push(0);
        self.present[0] = 1;
        self.total_incidence = 0;
    }

    fn reset_from(&mut self, seed: &DenseClosureSeed) {
        self.family.clear();
        self.family.extend_from_slice(&seed.family);
        self.present.copy_from_slice(&seed.present);
        self.total_incidence = seed.total_incidence;
    }

    fn add_generators(&mut self, generators: &[u64]) {
        for generator in generators {
            let generator = *generator;
            let generator_word = generator as usize / 64;
            let generator_mask = 1u64 << (generator % 64);
            if self.present[generator_word] & generator_mask != 0 {
                continue;
            }
            let before = self.family.len();
            for index in 0..before {
                let joined = self.family[index] | generator;
                let word = joined as usize / 64;
                let mask = 1u64 << (joined % 64);
                if self.present[word] & mask == 0 {
                    self.present[word] |= mask;
                    self.family.push(joined);
                    self.total_incidence += i64::from(joined.count_ones());
                }
            }
        }
    }
}

struct DenseClosureSeed {
    family: Vec<u64>,
    present: Vec<u64>,
    total_incidence: i64,
}

impl DenseClosureSeed {
    fn contains(&self, member: u64) -> bool {
        self.present[member as usize / 64] & (1u64 << (member % 64)) != 0
    }

    fn missing_generators(&self, orbit: &CyclicOrbit) -> usize {
        orbit
            .generators
            .iter()
            .filter(|generator| !self.contains(**generator))
            .count()
    }
}

fn separated_coordinate_pairs(generators: &[u64], width: usize) -> Vec<u64> {
    let pair_count = width * (width - 1) / 2;
    let mut separated = vec![0u64; pair_count.div_ceil(64)];
    let mut pair = 0;
    for left in 0..width {
        for right in (left + 1)..width {
            if generators
                .iter()
                .any(|member| ((member >> left) & 1) != ((member >> right) & 1))
            {
                separated[pair / 64] |= 1u64 << (pair % 64);
            }
            pair += 1;
        }
    }
    separated
}

fn pair_is_separating(left: &CyclicOrbit, right: &CyclicOrbit, pair_count: usize) -> bool {
    left.separated_pairs
        .iter()
        .zip(&right.separated_pairs)
        .enumerate()
        .all(|(word, (left, right))| {
            let remaining = pair_count.saturating_sub(word * 64).min(64);
            let required = if remaining == 64 {
                u64::MAX
            } else {
                (1u64 << remaining) - 1
            };
            (left | right) & required == required
        })
}

fn closure_signature(family: &[u64]) -> i64 {
    let mut ordered = family.to_vec();
    ordered.sort_unstable();
    let mut digest = blake3::Hasher::new();
    digest.update(b"rad-or-closure/v1\0");
    for member in ordered {
        digest.update(&member.to_le_bytes());
    }
    let digest = digest.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest.as_bytes()[..8]);
    (u64::from_le_bytes(prefix) & i64::MAX as u64) as i64
}

fn empty_cyclic_pair_profile() -> CyclicPairLaneProfile {
    CyclicPairLaneProfile {
        symmetry_classes: 0,
        evaluated: 0,
        equality_families: 0,
        diagonal_equality_families: 0,
        off_diagonal_equality_families: 0,
        full_cube_families: 0,
        equality_non_full_families: 0,
        positive_families: 0,
        negative_families: 0,
        separating_families: 0,
        best_generators: Vec::new(),
        best_frequencies: Vec::new(),
        best_members: 0,
        best_max_frequency: 0,
        best_margin: i64::MAX,
        best_signature: 0,
    }
}

fn greatest_common_divisor(mut left: usize, mut right: usize) -> usize {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn multiply_coordinates(mask: usize, multiplier: usize, width: usize) -> usize {
    let mut transformed = 0;
    for coordinate in 0..width {
        if mask & (1usize << coordinate) != 0 {
            transformed |= 1usize << ((coordinate * multiplier) % width);
        }
    }
    transformed
}

fn canonical_rotation_mask(mask: usize, width: usize) -> usize {
    let full = (1usize << width) - 1;
    let mut current = mask;
    let mut canonical = mask;
    for _ in 1..width {
        current = ((current << 1) & full) | (current >> (width - 1));
        canonical = canonical.min(current);
    }
    canonical
}

#[derive(Clone, Copy)]
struct CyclicPairClass {
    left: usize,
    right: usize,
    multiplicity: i64,
}

fn profile_pair_classes(
    classes: &[CyclicPairClass],
    orbits: &[CyclicOrbit],
    seeds: &[DenseClosureSeed],
    width: usize,
    cube_size: usize,
    minimum_members: i64,
) -> Result<CyclicPairLaneProfile, String> {
    let pair_count = width * (width - 1) / 2;
    let mut scratch = DenseClosureScratch::new(cube_size);
    let mut profile = empty_cyclic_pair_profile();
    for class in classes {
        let left = &orbits[class.left];
        let right = &orbits[class.right];
        let separating = pair_is_separating(left, right, pair_count);
        let left_seed = &seeds[class.left];
        let right_seed = &seeds[class.right];
        let right_missing = left_seed.missing_generators(right);
        let left_missing = right_seed.missing_generators(left);
        if left_seed.family.len() * right_missing <= right_seed.family.len() * left_missing {
            scratch.reset_from(left_seed);
            if right_missing != 0 {
                scratch.add_generators(&right.generators);
            }
        } else {
            scratch.reset_from(right_seed);
            if left_missing != 0 {
                scratch.add_generators(&left.generators);
            }
        }
        let members = scratch.family.len() as i64;
        if scratch.total_incidence % width as i64 != 0 {
            return Err("cyclic closure lost rotational frequency symmetry".to_string());
        }
        let maximum = scratch.total_incidence / width as i64;
        let margin = maximum * 2 - members;
        profile.symmetry_classes += 1;
        profile.evaluated += class.multiplicity;
        profile.separating_families += class.multiplicity * i64::from(separating);
        match margin.cmp(&0) {
            std::cmp::Ordering::Less => profile.negative_families += class.multiplicity,
            std::cmp::Ordering::Equal => {
                profile.equality_families += class.multiplicity;
                if class.left == class.right {
                    profile.diagonal_equality_families += class.multiplicity;
                } else {
                    profile.off_diagonal_equality_families += class.multiplicity;
                }
                if scratch.family.len() == cube_size {
                    profile.full_cube_families += class.multiplicity;
                } else {
                    profile.equality_non_full_families += class.multiplicity;
                }
            }
            std::cmp::Ordering::Greater => profile.positive_families += class.multiplicity,
        }

        let better = separating
            && members >= minimum_members
            && (profile.best_members == 0
                || maximum * profile.best_members < profile.best_max_frequency * members
                || (maximum * profile.best_members == profile.best_max_frequency * members
                    && members > profile.best_members));
        if better {
            profile.best_generators.clear();
            profile
                .best_generators
                .extend(left.generators.iter().map(|member| *member as i64));
            profile
                .best_generators
                .extend(right.generators.iter().map(|member| *member as i64));
            profile.best_frequencies = vec![maximum; width];
            profile.best_members = members;
            profile.best_max_frequency = maximum;
            profile.best_margin = margin;
            profile.best_signature = closure_signature(&scratch.family);
        }
    }
    Ok(profile)
}

/// Exhaust all unordered pairs of complete cyclic orbits, quotienting only by
/// proven coordinate multipliers that normalize the cyclic action. Every
/// symmetry class carries its exact orbit multiplicity, so the returned lane
/// totals still account for every original pair rather than sampling them.
/// One batched call replaces 198,765 JSON/ABI round trips while the RAD program
/// retains fork isolation, settlement, constraints, provenance, and the final
/// independently verified certificate.
pub(crate) fn cyclic_pair_profiles(
    representatives: &[i64],
    width: i64,
    lane_count: i64,
    minimum_members: i64,
) -> Result<Vec<CyclicPairLaneProfile>, String> {
    let width = checked_rotation_width(width, "cyclic_pair_profiles")?;
    let cube_size = 1usize << width;
    let lane_count = usize::try_from(lane_count)
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| "cyclic_pair_profiles lane_count must be positive".to_string())?;
    if minimum_members < 0 {
        return Err("cyclic_pair_profiles minimum_members must be non-negative".to_string());
    }

    let mut previous = None;
    let mut orbits = Vec::with_capacity(representatives.len());
    let mut representative_indices = std::collections::HashMap::with_capacity(representatives.len());
    for representative in representatives {
        let representative = usize::try_from(*representative)
            .ok()
            .filter(|value| *value > 0 && *value + 1 < cube_size)
            .ok_or_else(|| {
                format!(
                    "cyclic_pair_profiles representative {representative} is outside the nontrivial width-{width} cube"
                )
            })?;
        if previous.is_some_and(|prior| prior >= representative) {
            return Err(
                "cyclic_pair_profiles representatives must be strictly ascending".to_string(),
            );
        }
        previous = Some(representative);
        representative_indices.insert(representative, orbits.len());
        let generators = bitmask_rotation_orbit(representative as i64, width as i64)?
            .into_iter()
            .map(|member| member as u64)
            .collect::<Vec<_>>();
        let separated_pairs = separated_coordinate_pairs(&generators, width);
        orbits.push(CyclicOrbit {
            generators,
            separated_pairs,
        });
    }

    let multipliers = (1..width)
        .filter(|multiplier| greatest_common_divisor(*multiplier, width) == 1)
        .collect::<Vec<_>>();
    let transformed_indices = representatives
        .iter()
        .map(|representative| {
            multipliers
                .iter()
                .map(|multiplier| {
                    let transformed = multiply_coordinates(*representative as usize, *multiplier, width);
                    let canonical = canonical_rotation_mask(transformed, width);
                    representative_indices.get(&canonical).copied().ok_or_else(|| {
                        format!(
                            "cyclic_pair_profiles representatives are not closed under multiplier {multiplier}"
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut scratch = DenseClosureScratch::new(cube_size);
    let seeds = orbits
        .iter()
        .map(|orbit| {
            scratch.reset();
            scratch.add_generators(&orbit.generators);
            DenseClosureSeed {
                family: scratch.family.clone(),
                present: scratch.present.clone(),
                total_incidence: scratch.total_incidence,
            }
        })
        .collect::<Vec<_>>();
    let mut lane_classes = (0..lane_count).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut symmetry_class = 0usize;
    let mut images = [usize::MAX; MAX_TRANSFORM_WIDTH];
    let mut visited_pairs = vec![false; orbits.len() * orbits.len()];

    for left_index in 0..orbits.len() {
        for right_index in left_index..orbits.len() {
            let pair = left_index * orbits.len() + right_index;
            if visited_pairs[pair] {
                continue;
            }
            let mut image_count = 0usize;
            for multiplier_index in 0..multipliers.len() {
                let transformed_left = transformed_indices[left_index][multiplier_index];
                let transformed_right = transformed_indices[right_index][multiplier_index];
                let pair_left = transformed_left.min(transformed_right);
                let pair_right = transformed_left.max(transformed_right);
                let encoded = pair_left * orbits.len() + pair_right;
                if !images[..image_count].contains(&encoded) {
                    images[image_count] = encoded;
                    visited_pairs[encoded] = true;
                    image_count += 1;
                }
            }

            lane_classes[symmetry_class % lane_count].push(CyclicPairClass {
                left: left_index,
                right: right_index,
                multiplicity: image_count as i64,
            });
            symmetry_class += 1;
        }
    }

    std::thread::scope(|scope| {
        let orbits = &orbits;
        let seeds = &seeds;
        let handles = lane_classes
            .into_iter()
            .map(|classes| {
                scope.spawn(move || {
                    profile_pair_classes(
                        &classes,
                        &orbits,
                        &seeds,
                        width,
                        cube_size,
                        minimum_members,
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "cyclic pair profile worker panicked".to_string())?
            })
            .collect()
    })
}
