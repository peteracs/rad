fn nonnegative_u64(value: i64, name: &str) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| format!("{name} must be non-negative"))
}

fn residue_profile_json(profile: affine_parity::ResidueLaneProfile) -> Result<JsonValue, String> {
    let residue_sum = i64::try_from(profile.residue_sum)
        .map_err(|_| "affine residue profile sum exceeds RAD int".to_string())?;
    Ok(json!({
        "depth": profile.depth,
        "lane_index": profile.lane_index,
        "lane_count": profile.lane_count,
        "classes": profile.classes,
        "residue_sum": residue_sum,
        "pruned_classes": profile.pruned_classes,
        "survivor_classes": profile.survivor_classes,
        "contracting_survivors": profile.contracting_survivors,
        "noncontracting_survivors": profile.noncontracting_survivors,
        "expanded_nodes": profile.expanded_nodes,
        "max_odd_steps": profile.max_odd_steps,
        "max_odd_residue": profile.max_odd_residue,
        "max_threshold": profile.max_threshold.to_string(),
        "max_threshold_residue": profile.max_threshold_residue,
        "prune_histogram": profile.prune_depth_histogram,
        "survivor_odd_histogram": profile.survivor_odd_histogram,
        "survivor_sample": profile.survivor_sample,
        "signature": profile.signature,
    }))
}

unsafe extern "C" fn affine_residue_profile(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 6, "affine_residue_profile")?;
        let profile = affine_parity::residue_lane_profile(
            nonnegative_u64(int_arg(args, 0, "affine_residue_profile")?, "multiplier")?,
            nonnegative_u64(int_arg(args, 1, "affine_residue_profile")?, "addend")?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 2, "affine_residue_profile")?,
                "depth",
            )?)
            .map_err(|_| "depth exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 3, "affine_residue_profile")?,
                "verified_power",
            )?)
            .map_err(|_| "verified_power exceeds u32".to_string())?,
            nonnegative_u64(int_arg(args, 4, "affine_residue_profile")?, "lane_index")?,
            nonnegative_u64(int_arg(args, 5, "affine_residue_profile")?, "lane_count")?,
        )?;
        residue_profile_json(profile)
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn affine_residue_profiles(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 5, "affine_residue_profiles")?;
        let multiplier =
            nonnegative_u64(int_arg(args, 0, "affine_residue_profiles")?, "multiplier")?;
        let addend = nonnegative_u64(int_arg(args, 1, "affine_residue_profiles")?, "addend")?;
        let depth = u32::try_from(nonnegative_u64(
            int_arg(args, 2, "affine_residue_profiles")?,
            "depth",
        )?)
        .map_err(|_| "depth exceeds u32".to_string())?;
        let verified_power = u32::try_from(nonnegative_u64(
            int_arg(args, 3, "affine_residue_profiles")?,
            "verified_power",
        )?)
        .map_err(|_| "verified_power exceeds u32".to_string())?;
        let lane_count =
            nonnegative_u64(int_arg(args, 4, "affine_residue_profiles")?, "lane_count")?;

        let profiles = affine_parity::residue_lane_profiles(
            multiplier,
            addend,
            depth,
            verified_power,
            lane_count,
        )?;
        profiles
            .into_iter()
            .map(residue_profile_json)
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array)
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn affine_sparse_support_profile(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 5, "affine_sparse_support_profile")?;
        let profile = affine_parity::sparse_support_profile(
            nonnegative_u64(
                int_arg(args, 0, "affine_sparse_support_profile")?,
                "multiplier",
            )?,
            nonnegative_u64(int_arg(args, 1, "affine_sparse_support_profile")?, "addend")?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 2, "affine_sparse_support_profile")?,
                "max_depth",
            )?)
            .map_err(|_| "max_depth exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 3, "affine_sparse_support_profile")?,
                "verified_power",
            )?)
            .map_err(|_| "verified_power exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 4, "affine_sparse_support_profile")?,
                "max_input_ones",
            )?)
            .map_err(|_| "max_input_ones exceeds u32".to_string())?,
        )?;
        Ok(json!({
            "max_depth": profile.max_depth,
            "verified_power": profile.verified_power,
            "max_input_ones": profile.max_input_ones,
            "terminated": profile.terminated,
            "termination_depth": profile.termination_depth,
            "expanded_nodes": profile.expanded_nodes,
            "survivors_by_depth": profile.survivors_by_depth,
            "minimum_input_ones_by_depth": profile.minimum_input_ones_by_depth,
            "minimum_weight_witness_by_depth": profile.minimum_weight_witness_by_depth,
            "deepest_survival_by_weight": profile.deepest_survival_by_weight,
            "deepest_witness_by_weight": profile.deepest_witness_by_weight,
            "signature": profile.signature,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn affine_sparse_support_summary(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 5, "affine_sparse_support_summary")?;
        let profile = affine_parity::sparse_support_summary(
            nonnegative_u64(
                int_arg(args, 0, "affine_sparse_support_summary")?,
                "multiplier",
            )?,
            nonnegative_u64(int_arg(args, 1, "affine_sparse_support_summary")?, "addend")?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 2, "affine_sparse_support_summary")?,
                "max_depth",
            )?)
            .map_err(|_| "max_depth exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 3, "affine_sparse_support_summary")?,
                "verified_power",
            )?)
            .map_err(|_| "verified_power exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 4, "affine_sparse_support_summary")?,
                "max_input_ones",
            )?)
            .map_err(|_| "max_input_ones exceeds u32".to_string())?,
        )?;
        Ok(sparse_support_summary_json(profile, "descent_threshold"))
    })();
    result.map_or_else(fail, return_json)
}

fn sparse_support_summary_json(
    profile: affine_parity::SparseSupportSummary,
    criterion: &str,
) -> JsonValue {
    json!({
        "criterion": criterion,
        "max_depth": profile.max_depth,
        "verified_power": profile.verified_power,
        "max_input_ones": profile.max_input_ones,
        "all_budgets_terminated": profile.all_budgets_terminated,
        "termination_depth_by_budget": profile.termination_depth_by_budget,
        "deepest_survival_by_weight": profile.deepest_survival_by_weight,
        "deepest_witness_by_weight": profile.deepest_witness_by_weight,
        "deepest_witness_one_positions_by_weight": profile.deepest_witness_one_positions_by_weight,
        "anchors_by_weight": profile.anchors_by_weight,
        "expanded_nodes": profile.expanded_nodes,
        "signature": profile.signature,
    })
}

unsafe extern "C" fn affine_sparse_slope_support_summary(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 4, "affine_sparse_slope_support_summary")?;
        let profile = sparse_slope::summary(
            nonnegative_u64(
                int_arg(args, 0, "affine_sparse_slope_support_summary")?,
                "multiplier",
            )?,
            nonnegative_u64(
                int_arg(args, 1, "affine_sparse_slope_support_summary")?,
                "addend",
            )?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 2, "affine_sparse_slope_support_summary")?,
                "max_depth",
            )?)
            .map_err(|_| "max_depth exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 3, "affine_sparse_slope_support_summary")?,
                "max_input_ones",
            )?)
            .map_err(|_| "max_input_ones exceeds u32".to_string())?,
        )?;
        Ok(json!({
            "criterion": "prefix_slope",
            "max_depth": profile.max_depth,
            "verified_power": profile.verified_power,
            "max_input_ones": profile.max_input_ones,
            "all_budgets_terminated": profile.all_budgets_terminated,
            "termination_depth_by_budget": profile.termination_depth_by_budget,
            "deepest_survival_by_weight": profile.deepest_survival_by_weight,
            "deepest_witness_by_weight": profile.deepest_witness_by_weight,
            "deepest_witness_one_positions_by_weight": profile.deepest_witness_one_positions_by_weight,
            "anchors_by_weight": profile.anchors_by_weight,
            "expanded_nodes": profile.expanded_nodes,
            "signature": profile.signature,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn affine_sparse_slope_support_lane(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 6, "affine_sparse_slope_support_lane")?;
        let profile = sparse_slope::lane_summary(
            nonnegative_u64(
                int_arg(args, 0, "affine_sparse_slope_support_lane")?,
                "multiplier",
            )?,
            nonnegative_u64(
                int_arg(args, 1, "affine_sparse_slope_support_lane")?,
                "addend",
            )?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 2, "affine_sparse_slope_support_lane")?,
                "max_depth",
            )?)
            .map_err(|_| "max_depth exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 3, "affine_sparse_slope_support_lane")?,
                "max_input_ones",
            )?)
            .map_err(|_| "max_input_ones exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 4, "affine_sparse_slope_support_lane")?,
                "lane_index",
            )?)
            .map_err(|_| "lane_index exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 5, "affine_sparse_slope_support_lane")?,
                "lane_count",
            )?)
            .map_err(|_| "lane_count exceeds u32".to_string())?,
        )?;
        Ok(json!({
            "max_depth": profile.max_depth,
            "max_input_ones": profile.max_input_ones,
            "lane_index": profile.lane_index,
            "lane_count": profile.lane_count,
            "split_weight": profile.split_weight,
            "seed_count": profile.seed_count,
            "assigned_seed_count": profile.assigned_seed_count,
            "deepest_survival_by_weight": profile.deepest_survival_by_weight,
            "deepest_witness_by_weight": profile.deepest_witness_by_weight,
            "deepest_witness_one_positions_by_weight": profile.deepest_witness_one_positions_by_weight,
            "anchors_by_weight": profile.anchors_by_weight,
            "expanded_nodes": profile.expanded_nodes,
            "signature": profile.signature,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn affine_frontier_profile(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 7, "affine_frontier_profile")?;
        let objective = string_arg(args, 5, "affine_frontier_profile")?;
        let profile = affine_frontier::affine_frontier_profile(
            nonnegative_u64(int_arg(args, 0, "affine_frontier_profile")?, "multiplier")?,
            nonnegative_u64(int_arg(args, 1, "affine_frontier_profile")?, "addend")?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 2, "affine_frontier_profile")?,
                "max_depth",
            )?)
            .map_err(|_| "max_depth exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 3, "affine_frontier_profile")?,
                "max_input_ones",
            )?)
            .map_err(|_| "max_input_ones exceeds u32".to_string())?,
            usize::try_from(nonnegative_u64(
                int_arg(args, 4, "affine_frontier_profile")?,
                "beam_per_support",
            )?)
            .map_err(|_| "beam_per_support exceeds usize".to_string())?,
            &objective,
            nonnegative_u64(int_arg(args, 6, "affine_frontier_profile")?, "seed")?,
        )?;
        let frontier_exhausted = profile.terminal_minimum_input_ones.is_none();
        let terminal_minimum_input_ones = profile
            .terminal_minimum_input_ones
            .unwrap_or(profile.max_input_ones + 1);
        let terminal_witness = profile.terminal_witness.unwrap_or_default();
        Ok(json!({
            "objective": profile.objective,
            "max_depth": profile.max_depth,
            "max_input_ones": profile.max_input_ones,
            "beam_per_support": profile.beam_per_support,
            "reached_depth": profile.reached_depth,
            "expanded_nodes": profile.expanded_nodes,
            "peak_frontier": profile.peak_frontier,
            "records": profile.records.into_iter().map(|record| json!({
                "depth": record.depth,
                "minimum_input_ones": record.minimum_input_ones,
                "witness": record.witness,
                "one_positions": record.one_positions,
                "odd_steps": record.odd_steps,
                "coefficient_bits": record.coefficient_bits,
                "probe_bits": record.probe_bits,
                "zero_runway": record.zero_runway,
            })).collect::<Vec<_>>(),
            "deepest_retained_by_support": profile.deepest_retained_by_support.into_iter().map(|record| json!({
                "depth": record.depth,
                "input_ones": record.minimum_input_ones,
                "witness": record.witness,
                "one_positions": record.one_positions,
                "odd_steps": record.odd_steps,
                "coefficient_bits": record.coefficient_bits,
                "probe_bits": record.probe_bits,
                "zero_runway": record.zero_runway,
            })).collect::<Vec<_>>(),
            "frontier_exhausted": frontier_exhausted,
            "terminal_minimum_input_ones": terminal_minimum_input_ones,
            "terminal_witness": terminal_witness,
            "terminal_one_positions": profile.terminal_one_positions,
            "signature": profile.signature,
            "certificate": false,
        }))
    })();
    result.map_or_else(fail, return_json)
}

fn natural_tail_profile_json(profile: affine_parity::NaturalTailLaneProfile) -> JsonValue {
    json!({
        "depth": profile.depth,
        "lane_index": profile.lane_index,
        "lane_count": profile.lane_count,
        "survivor_classes": profile.survivor_classes,
        "coefficient_stops": profile.coefficient_stops,
        "descents": profile.descents,
        "unresolved": profile.unresolved,
        "max_coefficient_stop_step": profile.max_coefficient_stop_step,
        "max_coefficient_stop_residue": profile.max_coefficient_stop_residue,
        "max_descent_step": profile.max_descent_step,
        "max_descent_residue": profile.max_descent_residue,
        "max_additive_delay": profile.max_additive_delay,
        "max_additive_delay_residue": profile.max_additive_delay_residue,
        "max_peak": profile.max_peak.to_string(),
        "max_peak_residue": profile.max_peak_residue,
        "coefficient_stop_histogram": profile.coefficient_stop_histogram,
        "descent_histogram": profile.descent_histogram,
        "signature": profile.signature,
    })
}

unsafe extern "C" fn affine_natural_tail_profiles(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 6, "affine_natural_tail_profiles")?;
        let multiplier = nonnegative_u64(
            int_arg(args, 0, "affine_natural_tail_profiles")?,
            "multiplier",
        )?;
        let addend = nonnegative_u64(int_arg(args, 1, "affine_natural_tail_profiles")?, "addend")?;
        let depth = u32::try_from(nonnegative_u64(
            int_arg(args, 2, "affine_natural_tail_profiles")?,
            "depth",
        )?)
        .map_err(|_| "depth exceeds u32".to_string())?;
        let verified_power = u32::try_from(nonnegative_u64(
            int_arg(args, 3, "affine_natural_tail_profiles")?,
            "verified_power",
        )?)
        .map_err(|_| "verified_power exceeds u32".to_string())?;
        let lane_count = nonnegative_u64(
            int_arg(args, 4, "affine_natural_tail_profiles")?,
            "lane_count",
        )?;
        let max_steps = u32::try_from(nonnegative_u64(
            int_arg(args, 5, "affine_natural_tail_profiles")?,
            "max_steps",
        )?)
        .map_err(|_| "max_steps exceeds u32".to_string())?;

        affine_parity::natural_tail_lane_profiles(
            multiplier,
            addend,
            depth,
            verified_power,
            lane_count,
            max_steps,
        )
        .map(|profiles| {
            JsonValue::Array(
                profiles
                    .into_iter()
                    .map(natural_tail_profile_json)
                    .collect(),
            )
        })
    })();
    result.map_or_else(fail, return_json)
}
