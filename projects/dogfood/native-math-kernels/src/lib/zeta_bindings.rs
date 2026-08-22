
unsafe extern "C" fn zeta_cover_lane(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 3, "zeta_cover_lane")?;
        let encoded = string_arg(args, 0, "zeta_cover_lane")?;
        let lane_index = int_arg(args, 1, "zeta_cover_lane")?;
        let lane_count = int_arg(args, 2, "zeta_cover_lane")?;
        let report = zeta_cover::verify_lane(&encoded, lane_index, lane_count)?;
        Ok(json!({
            "verified": report.verified,
            "certificate_digest": report.digest,
            "lane_index": report.lane_index,
            "lane_count": report.lane_count,
            "initial_boxes": report.stats.initial_boxes,
            "nodes": report.stats.nodes,
            "pruned": report.stats.pruned,
            "splits": report.stats.splits,
            "maximum_depth": report.stats.maximum_depth,
            "pressure_pruned": report.stats.pressure_pruned,
            "interval_pruned": report.stats.interval_pruned,
            "tangent_pruned": report.stats.tangent_pruned,
            "minimum_leaf_lower_scaled": report.minimum_leaf_lower_scaled,
            "minimum_leaf_kind": report.minimum_leaf_kind,
            "minimum_leaf_box": report.minimum_leaf_box,
            "signature": report.signature,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_prepare_cover(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "zeta_prepare_cover")?;
        let encoded = string_arg(args, 0, "zeta_prepare_cover")?;
        Ok(json!({
            "certificate_digest": zeta_cover::prepare_digest(&encoded)?,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_prepared_cover_lane(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 3, "zeta_prepared_cover_lane")?;
        let digest = string_arg(args, 0, "zeta_prepared_cover_lane")?;
        let lane_index = int_arg(args, 1, "zeta_prepared_cover_lane")?;
        let lane_count = int_arg(args, 2, "zeta_prepared_cover_lane")?;
        let report = zeta_cover::verify_prepared_lane(&digest, lane_index, lane_count)?;
        Ok(json!({
            "verified": report.verified,
            "certificate_digest": report.digest,
            "lane_index": report.lane_index,
            "lane_count": report.lane_count,
            "initial_boxes": report.stats.initial_boxes,
            "nodes": report.stats.nodes,
            "pruned": report.stats.pruned,
            "splits": report.stats.splits,
            "maximum_depth": report.stats.maximum_depth,
            "pressure_pruned": report.stats.pressure_pruned,
            "interval_pruned": report.stats.interval_pruned,
            "tangent_pruned": report.stats.tangent_pruned,
            "minimum_leaf_lower_scaled": report.minimum_leaf_lower_scaled,
            "minimum_leaf_kind": report.minimum_leaf_kind,
            "minimum_leaf_box": report.minimum_leaf_box,
            "signature": report.signature,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_equispaced_block(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "zeta_equispaced_block")?;
        let report = zeta_cover::equispaced_block(
            int_arg(args, 0, "zeta_equispaced_block")?,
            int_arg(args, 1, "zeta_equispaced_block")?,
        )?;
        Ok(json!({
            "point_count": report.point_count,
            "gap_micros": report.gap_micros,
            "energy_upper_scaled": report.energy_upper_scaled,
            "span_tax_upper_scaled": report.span_tax_upper_scaled,
            "total_upper_scaled": report.total_upper_scaled,
            "scale": 1_000_000_000_i64,
            "rigorous": false,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_even_spacing_bound(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "zeta_even_spacing_bound")?;
        let report = zeta_cover::even_spacing_block_bound(int_arg(
            args,
            0,
            "zeta_even_spacing_bound",
        )?)?;
        Ok(json!({
            "point_count": report.point_count,
            "gap_micros": report.gap_micros,
            "energy_upper_scaled": report.energy_upper_scaled,
            "span_tax_upper_scaled": report.span_tax_upper_scaled,
            "total_upper_scaled": report.total_upper_scaled,
            "scale": 1_000_000_000_i64,
            "rigorous": true,
            "proof": "k(2r)=-1/(8*pi^2*r^2-1); pi>3",
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_integer_spacing_bound(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "zeta_integer_spacing_bound")?;
        let report = zeta_cover::integer_spacing_block_bound(
            int_arg(args, 0, "zeta_integer_spacing_bound")?,
            int_arg(args, 1, "zeta_integer_spacing_bound")?,
        )?;
        Ok(json!({
            "point_count": report.point_count,
            "gap_micros": report.gap_micros,
            "energy_upper_scaled": report.energy_upper_scaled,
            "span_tax_upper_scaled": report.span_tax_upper_scaled,
            "total_upper_scaled": report.total_upper_scaled,
            "scale": 1_000_000_000_i64,
            "rigorous": true,
            "proof": "|k(n)|=1/(2*pi^2*n^2-1); pi>3",
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_mt_bandwidth_scout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "zeta_mt_bandwidth_scout")?;
        let report = zeta_cover::bandwidth_scout(int_arg(
            args,
            0,
            "zeta_mt_bandwidth_scout",
        )?)?;
        Ok(json!({
            "lambda_micros": report.lambda_micros,
            "baseline_lower_scaled": report.baseline_lower_scaled,
            "scale": 1_000_000_000_i64,
            "rigorous": false,
            "formula": "H(lambda)=2-(cot(lambda/sqrt(2))+lambda/sqrt(2))/sqrt(2)",
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_fourth_moment_transfer_scout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 3, "zeta_fourth_moment_transfer_scout")?;
        let report = zeta_cover::fourth_moment_transfer_scout(
            int_arg(args, 0, "zeta_fourth_moment_transfer_scout")?,
            int_arg(args, 1, "zeta_fourth_moment_transfer_scout")?,
            int_arg(args, 2, "zeta_fourth_moment_transfer_scout")?,
        )?;
        Ok(json!({
            "limit": report.limit,
            "max_shift": report.max_shift,
            "small_factor_cap": report.small_factor_cap,
            "nonzero_coefficients": report.nonzero_coefficients,
            "mean_coefficient_scaled": report.mean_coefficient_scaled,
            "aggregate_ratio_scaled": report.aggregate_ratio_scaled,
            "sampled_aggregate_ratio_scaled": report.sampled_aggregate_ratio_scaled,
            "fejer_aggregate_ratio_scaled": report.fejer_aggregate_ratio_scaled,
            "ascending_aggregate_ratio_scaled": report.ascending_aggregate_ratio_scaled,
            "even_ratio_scaled": report.even_ratio_scaled,
            "odd_ratio_scaled": report.odd_ratio_scaled,
            "rms_error_scaled": report.rms_error_scaled,
            "minimum_ratio_scaled": report.minimum_ratio_scaled,
            "maximum_ratio_scaled": report.maximum_ratio_scaled,
            "residue_block_rms_scaled": report.residue_block_rms_scaled,
            "maximum_residue_block_error_scaled": report.maximum_residue_block_error_scaled,
            "typical_aggregate_ratio_scaled": report.typical_aggregate_ratio_scaled,
            "balanced_aggregate_ratio_scaled": report.balanced_aggregate_ratio_scaled,
            "cross_aggregate_ratio_scaled": report.cross_aggregate_ratio_scaled,
            "excluded_mean_fraction_scaled": report.excluded_mean_fraction_scaled,
            "tail_l2_fraction_scaled": report.tail_l2_fraction_scaled,
            "inside_relative_slack": report.inside_relative_slack,
            "scale": 1_000_000_000_i64,
            "required_relative_slack_denominator": 117,
            "rigorous": false,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_cosine_pair_scout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "zeta_cosine_pair_scout")?;
        let report = zeta_cover::cosine_pair_scout(int_arg(
            args,
            0,
            "zeta_cosine_pair_scout",
        )?)?;
        Ok(json!({
            "a_micros": report.a_micros,
            "baseline_scaled": report.baseline_scaled,
            "best_q_scaled": report.best_q_scaled,
            "local_a_scaled": report.local_a_scaled,
            "minimizing_gap_micros": report.minimizing_gap_micros,
            "simple_bound_scaled": report.simple_bound_scaled,
            "scale": 1_000_000_000_i64,
            "rigorous": false,
            "formula": "B=(H(a)-q/2)/(1-inf_g(2*K_a(g)^2+q*g)/2)",
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_cosine_direct_sum_scout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "zeta_cosine_direct_sum_scout")?;
        let report = zeta_cover::cosine_direct_sum_scout(
            int_arg(args, 0, "zeta_cosine_direct_sum_scout")?,
            int_arg(args, 1, "zeta_cosine_direct_sum_scout")?,
        )?;
        Ok(json!({
            "companion_a_micros": report.companion_a_micros,
            "companion_weight_ppm": report.companion_weight_ppm,
            "baseline_scaled": report.baseline_scaled,
            "best_q_scaled": report.best_q_scaled,
            "local_a_scaled": report.local_a_scaled,
            "minimizing_gap_micros": report.minimizing_gap_micros,
            "simple_bound_scaled": report.simple_bound_scaled,
            "scale": 1_000_000_000_i64,
            "rigorous": false,
            "formula": "weighted bandwidth-one baselines and two-point Gram defects",
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_perturbed_window_scout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "zeta_perturbed_window_scout")?;
        let report = zeta_cover::perturbed_window_scout(int_arg(
            args,
            0,
            "zeta_perturbed_window_scout",
        )?)?;
        Ok(json!({
            "mode_coefficient_micros": report.mode_coefficient_micros,
            "baseline_scaled": report.baseline_scaled,
            "local_c_scaled": report.local_c_scaled,
            "best_block_size": report.best_block_size,
            "simple_bound_scaled": report.simple_bound_scaled,
            "minimizing_gaps_micros": report.minimizing_gaps_micros,
            "starts_checked": report.starts_checked,
            "scale": 1_000_000_000_i64,
            "rigorous": false,
            "formula": "v(s)=cos(sqrt(2)s)+c*cos(2*pi*s); seven-point pressure=1/3000",
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_perturbed_window_two_mode_scout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "zeta_perturbed_window_two_mode_scout")?;
        let report = zeta_cover::perturbed_window_two_mode_scout(
            int_arg(args, 0, "zeta_perturbed_window_two_mode_scout")?,
            int_arg(args, 1, "zeta_perturbed_window_two_mode_scout")?,
        )?;
        Ok(json!({
            "mode_coefficient_micros": report.mode_coefficient_micros,
            "second_mode_coefficient_micros": report.second_mode_coefficient_micros,
            "baseline_scaled": report.baseline_scaled,
            "local_c_scaled": report.local_c_scaled,
            "best_block_size": report.best_block_size,
            "simple_bound_scaled": report.simple_bound_scaled,
            "minimizing_gaps_micros": report.minimizing_gaps_micros,
            "starts_checked": report.starts_checked,
            "scale": 1_000_000_000_i64,
            "rigorous": false,
            "formula": "v(s)=cos(sqrt(2)s)+c1*cos(2*pi*s)+c2*cos(4*pi*s)",
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn zeta_periodic_density_scout(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "zeta_periodic_density_scout")?;
        let report = zeta_cover::periodic_density_scout(
            int_arg(args, 0, "zeta_periodic_density_scout")?,
            int_arg(args, 1, "zeta_periodic_density_scout")?,
        )?;
        Ok(json!({
            "long_gap_mask": report.long_gap_mask,
            "short_gap_micros": report.short_gap_micros,
            "long_gap_micros": report.long_gap_micros,
            "energy_per_point_scaled": report.energy_per_point_scaled,
            "implied_simple_bound_scaled": report.implied_simple_bound_scaled,
            "neighbor_terms": report.neighbor_terms,
            "scale": 1_000_000_000_i64,
            "density_numerator": 7,
            "density_denominator": 10,
            "rigorous": false,
        }))
    })();
    result.map_or_else(fail, return_json)
}
