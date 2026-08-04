unsafe extern "C" fn lattice_closure(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "lattice_closure")?;
        Ok(json!(boolean_lattice::or_closure(&integer_list(
            args,
            "lattice_closure"
        )?)?))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_profile(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "lattice_profile")?;
        let generators = integer_list(args, "lattice_profile")?;
        let width = int_arg(args, 1, "lattice_profile")?;
        let (size, frequencies, separating, signature) =
            boolean_lattice::or_closure_stats(&generators, width)?;
        Ok(json!({
            "size": size,
            "frequencies": frequencies,
            "separating": separating,
            "signature": signature,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_frequencies(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "lattice_frequencies")?;
        let values = integer_list(args, "lattice_frequencies")?;
        Ok(json!(boolean_lattice::bit_frequencies(
            &values,
            int_arg(args, 1, "lattice_frequencies")?,
        )?))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn bitmask_rotation_orbit(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "bitmask_rotation_orbit")?;
        Ok(json!(boolean_lattice::bitmask_rotation_orbit(
            int_arg(args, 0, "bitmask_rotation_orbit")?,
            int_arg(args, 1, "bitmask_rotation_orbit")?,
        )?))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn bitmask_rotation_representatives(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "bitmask_rotation_representatives")?;
        Ok(json!(boolean_lattice::bitmask_rotation_representatives(
            int_arg(args, 0, "bitmask_rotation_representatives")?,
        )?))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_is_closed(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "lattice_is_closed")?;
        Ok(json!(boolean_lattice::is_or_closed(&integer_list(
            args,
            "lattice_is_closed"
        )?)?))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_violation_count(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "lattice_violation_count")?;
        let values = integer_list(args, "lattice_violation_count")?;
        Ok(json!(boolean_lattice::or_violation_count(
            &values,
            int_arg(args, 1, "lattice_violation_count")?,
        )?))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_deletable(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "lattice_deletable")?;
        let values = integer_list(args, "lattice_deletable")?;
        Ok(json!(boolean_lattice::or_deletable_members(
            &values,
            int_arg(args, 1, "lattice_deletable")?,
        )?))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_deletion_profile(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "lattice_deletion_profile")?;
        let deleted = integer_list(args, "lattice_deletion_profile")?;
        let state = boolean_lattice::or_deletion_state(
            &deleted,
            int_arg(args, 1, "lattice_deletion_profile")?,
        )?;
        Ok(json!({
            "family_size": state.family_size,
            "frequencies": state.family_frequencies,
            "surpluses": state.deletion_surpluses,
            "pair_biases": state.pair_biases,
            "frontier": state.deletable_members,
            "effective_frontier": state.effective_deletable_members,
            "separating": state.separating,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_deletion_rollout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 7, "lattice_deletion_rollout")?;
        let deleted = integer_list(args, "lattice_deletion_rollout")?;
        let steps = usize::try_from(int_arg(args, 2, "lattice_deletion_rollout")?)
            .map_err(|_| "lattice_deletion_rollout steps must be non-negative".to_string())?;
        let choices = usize::try_from(int_arg(args, 3, "lattice_deletion_rollout")?)
            .map_err(|_| "lattice_deletion_rollout choices must be non-negative".to_string())?;
        let seed = u64::try_from(int_arg(args, 4, "lattice_deletion_rollout")?)
            .map_err(|_| "lattice_deletion_rollout seed must be non-negative".to_string())?;
        let objective = match string_arg(args, 5, "lattice_deletion_rollout")?.as_str() {
            "max_min" => boolean_lattice::OrDeletionObjective::MaxMin,
            "max_min_low_peak" => boolean_lattice::OrDeletionObjective::MaxMinLowPeak,
            "min_pair_bias" => boolean_lattice::OrDeletionObjective::MinPairBias,
            other => {
                return Err(format!(
                    "lattice_deletion_rollout unknown objective '{other}'"
                ))
            }
        };
        let rollout = boolean_lattice::or_deletion_rollout(
            &deleted,
            int_arg(args, 1, "lattice_deletion_rollout")?,
            steps,
            choices,
            seed,
            objective,
            int_arg(args, 6, "lattice_deletion_rollout")?,
        )?;
        Ok(json!({
            "deleted": rollout.deleted,
            "family_size": rollout.state.family_size,
            "frequencies": rollout.state.family_frequencies,
            "surpluses": rollout.state.deletion_surpluses,
            "pair_biases": rollout.state.pair_biases,
            "frontier": rollout.state.deletable_members,
            "effective_frontier": rollout.state.effective_deletable_members,
            "separating": rollout.state.separating,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_apply_deletion(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 3, "lattice_apply_deletion")?;
        let deleted = integer_list(args, "lattice_apply_deletion")?;
        let applied = boolean_lattice::or_apply_deletion(
            &deleted,
            int_arg(args, 1, "lattice_apply_deletion")?,
            int_arg(args, 2, "lattice_apply_deletion")?,
        )?;
        Ok(json!({
            "deleted": applied.deleted,
            "family_size": applied.state.family_size,
            "frequencies": applied.state.family_frequencies,
            "surpluses": applied.state.deletion_surpluses,
            "pair_biases": applied.state.pair_biases,
            "frontier": applied.state.deletable_members,
            "effective_frontier": applied.state.effective_deletable_members,
            "separating": applied.state.separating,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn lattice_exchange_rollout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 9, "lattice_exchange_rollout")?;
        let deleted = integer_list(args, "lattice_exchange_rollout")?;
        let steps = usize::try_from(int_arg(args, 2, "lattice_exchange_rollout")?)
            .map_err(|_| "lattice_exchange_rollout steps must be non-negative".to_string())?;
        let choices = usize::try_from(int_arg(args, 3, "lattice_exchange_rollout")?)
            .map_err(|_| "lattice_exchange_rollout choices must be non-negative".to_string())?;
        let seed = u64::try_from(int_arg(args, 4, "lattice_exchange_rollout")?)
            .map_err(|_| "lattice_exchange_rollout seed must be non-negative".to_string())?;
        let objective = match string_arg(args, 5, "lattice_exchange_rollout")?.as_str() {
            "max_min" => boolean_lattice::OrDeletionObjective::MaxMin,
            "max_min_low_peak" => boolean_lattice::OrDeletionObjective::MaxMinLowPeak,
            "min_pair_bias" => boolean_lattice::OrDeletionObjective::MinPairBias,
            other => {
                return Err(format!(
                    "lattice_exchange_rollout unknown objective '{other}'"
                ))
            }
        };
        let acceptance = match string_arg(args, 7, "lattice_exchange_rollout")?.as_str() {
            "non_worsening" => boolean_lattice::OrExchangeAcceptance::NonWorsening,
            "exploratory" => boolean_lattice::OrExchangeAcceptance::Exploratory,
            other => {
                return Err(format!(
                    "lattice_exchange_rollout unknown acceptance policy '{other}'"
                ))
            }
        };
        let repair_beam_width = usize::try_from(int_arg(args, 8, "lattice_exchange_rollout")?)
            .ok()
            .filter(|width| *width > 0)
            .ok_or_else(|| {
                "lattice_exchange_rollout repair beam width must be positive".to_string()
            })?;
        let rollout = boolean_lattice::or_exchange_rollout(
            &deleted,
            int_arg(args, 1, "lattice_exchange_rollout")?,
            steps,
            choices,
            seed,
            objective,
            int_arg(args, 6, "lattice_exchange_rollout")?,
            acceptance,
            repair_beam_width,
        )?;
        Ok(json!({
            "deleted": rollout.deleted,
            "family_size": rollout.state.family_size,
            "frequencies": rollout.state.family_frequencies,
            "surpluses": rollout.state.deletion_surpluses,
            "pair_biases": rollout.state.pair_biases,
            "frontier": rollout.state.deletable_members,
            "effective_frontier": rollout.state.effective_deletable_members,
            "separating": rollout.state.separating,
        }))
    })();
    result.map_or_else(fail, return_json)
}
