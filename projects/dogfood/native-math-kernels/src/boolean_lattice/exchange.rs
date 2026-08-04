fn pair_biases_from_frequencies(frequencies: &[i64], family_size: i64) -> Vec<i64> {
    let mut biases =
        Vec::with_capacity(frequencies.len() * frequencies.len().saturating_sub(1) / 2);
    for left in 0..frequencies.len() {
        for right in (left + 1)..frequencies.len() {
            biases.push(frequencies[left] + frequencies[right] - family_size);
        }
    }
    biases
}

fn objective_profile(machine: &IncrementalOrDeletion, objective: OrDeletionObjective) -> Vec<i64> {
    let mut coordinate_profile = machine.deletion_surpluses();
    coordinate_profile.sort_unstable();
    if objective != OrDeletionObjective::MinPairBias {
        return coordinate_profile;
    }
    let mut pair_profile =
        pair_biases_from_frequencies(&machine.family_frequencies, machine.family_size())
            .into_iter()
            .map(|bias| -bias)
            .collect::<Vec<_>>();
    pair_profile.sort_unstable();
    pair_profile.extend(coordinate_profile);
    pair_profile
}

fn profile_is_better(profile: &[i64], incumbent: &[i64], objective: OrDeletionObjective) -> bool {
    let better_minimum = profile[0] > incumbent[0];
    let same_minimum = profile[0] == incumbent[0];
    let better_peak = profile[profile.len() - 1] < incumbent[incumbent.len() - 1];
    let same_peak = profile[profile.len() - 1] == incumbent[incumbent.len() - 1];
    better_minimum
        || (same_minimum && objective == OrDeletionObjective::MaxMinLowPeak && better_peak)
        || (same_minimum
            && (objective != OrDeletionObjective::MaxMinLowPeak || same_peak)
            && profile > incumbent)
}

fn density_allows_removal(
    machine: &IncrementalOrDeletion,
    member: usize,
    minimum_density_per_mille: i64,
) -> bool {
    (0..machine.width).all(|bit| {
        let next_frequency =
            machine.family_frequencies[bit] - i64::from(member & (1usize << bit) != 0);
        next_frequency * 1000 >= (machine.family_size() - 1) * minimum_density_per_mille
    })
}

fn insert_repair_state(
    beam: &mut Vec<(Vec<i64>, IncrementalOrDeletion)>,
    candidate: IncrementalOrDeletion,
    width: usize,
    objective: OrDeletionObjective,
) {
    if beam
        .iter()
        .any(|(_, incumbent)| incumbent.present == candidate.present)
    {
        return;
    }
    let profile = objective_profile(&candidate, objective);
    let position = beam
        .iter()
        .position(|(incumbent, _)| profile_is_better(&profile, incumbent, objective))
        .unwrap_or(beam.len());
    beam.insert(position, (profile, candidate));
    if beam.len() > width.max(1) {
        beam.pop();
    }
}

struct RepairMove {
    profile: Vec<i64>,
    parent: usize,
    member: usize,
}

fn insert_repair_move(
    moves: &mut Vec<RepairMove>,
    candidate: RepairMove,
    width: usize,
    objective: OrDeletionObjective,
) {
    let position = moves
        .iter()
        .position(|incumbent| {
            profile_is_better(&candidate.profile, &incumbent.profile, objective)
                || (candidate.profile == incumbent.profile
                    && (candidate.member, candidate.parent) < (incumbent.member, incumbent.parent))
        })
        .unwrap_or(moves.len());
    moves.insert(position, candidate);
    // A few extra ranked moves let realization skip duplicate states reached
    // by different deletion orders without ever cloning the full frontier.
    let retained = width.max(1).saturating_mul(4);
    if moves.len() > retained {
        moves.pop();
    }
}

/// Explore fixed-cardinality neighbors of an OR-closed Boolean family.
///
/// One exchange adjoins a missing member, computes its exact least OR closure,
/// and then performs the same number of legal deletions.  The explicitly
/// inserted member is retained, so a successful exchange cannot be a no-op.
/// This is a domain-neutral branch primitive for finite join-semilattices; the
/// caller owns the interpretation of coordinate balance.
pub(crate) fn or_exchange_rollout(
    deleted: &[i64],
    width: i64,
    steps: usize,
    choices_per_step: usize,
    seed: u64,
    objective: OrDeletionObjective,
    minimum_density_per_mille: i64,
    acceptance: OrExchangeAcceptance,
    repair_beam_width: usize,
) -> Result<OrDeletionRollout, String> {
    if !(0..=1000).contains(&minimum_density_per_mille) {
        return Err("minimum density must be between 0 and 1000 per mille".to_string());
    }
    let mut machine = IncrementalOrDeletion::new(deleted, width)?;
    let target_size = machine.family_size();
    let mut rng = seed.max(1);

    for _ in 0..steps {
        let mut insertion_pool = machine
            .deleted
            .iter()
            .map(|member| *member as usize)
            .collect::<Vec<_>>();
        insertion_pool.sort_unstable();
        if insertion_pool.is_empty() {
            break;
        }
        let start = next_random(&mut rng, insertion_pool.len());
        let trials = choices_per_step.max(1).min(insertion_pool.len());
        let mut best: Option<(Vec<i64>, IncrementalOrDeletion)> = None;

        for offset in 0..trials {
            let inserted = insertion_pool[(start + offset) % insertion_pool.len()];
            let additions = machine.closure_additions(inserted);
            if additions.is_empty() {
                continue;
            }
            let mut restored = vec![false; machine.cube_size];
            for member in &additions {
                restored[*member] = true;
            }
            let next_deleted = machine
                .deleted
                .iter()
                .copied()
                .filter(|member| !restored[*member as usize])
                .collect::<Vec<_>>();
            let restored = IncrementalOrDeletion::new(&next_deleted, width)?;
            let mut repair_beam = vec![(objective_profile(&restored, objective), restored)];

            for _ in 0..additions.len() {
                let mut ranked_moves = Vec::new();
                for (parent, (_, partial)) in repair_beam.iter().enumerate() {
                    let (_, effective) = partial.frontiers();
                    for member in effective
                        .into_iter()
                        .map(|member| member as usize)
                        .filter(|member| *member != 0 && *member != inserted)
                        .filter(|member| {
                            density_allows_removal(&partial, *member, minimum_density_per_mille)
                        })
                    {
                        insert_repair_move(
                            &mut ranked_moves,
                            RepairMove {
                                profile: partial.profile_after(member, objective),
                                parent,
                                member,
                            },
                            repair_beam_width,
                            objective,
                        );
                    }
                }
                if ranked_moves.is_empty() {
                    repair_beam = Vec::new();
                    break;
                }

                let mut next_repair_beam = Vec::new();
                for repair_move in ranked_moves {
                    let mut branch = repair_beam[repair_move.parent].1.clone();
                    branch.remove(repair_move.member)?;
                    insert_repair_state(
                        &mut next_repair_beam,
                        branch,
                        repair_beam_width,
                        objective,
                    );
                    if next_repair_beam.len() >= repair_beam_width.max(1) {
                        break;
                    }
                }
                repair_beam = next_repair_beam;
            }

            for (_, mut candidate) in repair_beam {
                if candidate.family_size() != target_size || !candidate.present[inserted] {
                    continue;
                }
                candidate.deleted.sort_unstable();
                let profile = objective_profile(&candidate, objective);
                let replace = best
                    .as_ref()
                    .map(|(incumbent, _)| profile_is_better(&profile, incumbent, objective))
                    .unwrap_or(true);
                let tied = best
                    .as_ref()
                    .is_some_and(|(incumbent, _)| *incumbent == profile);
                if replace || (tied && next_random(&mut rng, 2) == 0) {
                    best = Some((profile, candidate));
                }
            }
        }

        let Some((best_profile, next)) = best else {
            break;
        };
        let current_profile = objective_profile(&machine, objective);
        // Equal-profile exchanges are useful plateau moves. Exploratory walks
        // may also cross a strict local regression and return that endpoint so
        // an external branch/search policy can continue from the far side of
        // a valley. This is domain-neutral: every accepted intermediate is an
        // exact OR-closed, fixed-cardinality family.
        if acceptance == OrExchangeAcceptance::NonWorsening
            && profile_is_better(&current_profile, &best_profile, objective)
        {
            break;
        }
        machine = next;
    }

    Ok(OrDeletionRollout {
        deleted: machine.deleted.clone(),
        state: machine.snapshot(),
    })
}
