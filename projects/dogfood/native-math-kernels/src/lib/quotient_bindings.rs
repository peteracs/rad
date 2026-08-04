fn column_quotient_json(profile: column_quotient::ColumnQuotientScan) -> JsonValue {
    json!({
        "generator_count": profile.generator_count,
        "column_count": profile.column_count,
        "minimum_column_count": profile.minimum_column_count,
        "maximum_column_count": profile.maximum_column_count,
        "maximum_column_weight": profile.maximum_column_weight,
        "minimum_family_size": profile.minimum_family_size,
        "maximum_family_size": profile.maximum_family_size,
        "pattern_count": profile.pattern_count,
        "labelled_configurations": profile.labelled_configurations,
        "covered_labelled_configurations": profile.covered_labelled_configurations,
        "symmetry_orbits": profile.symmetry_orbits,
        "frontier_orbits": profile.frontier_orbits,
        "minimum_margin": profile.minimum_margin,
        "best_columns": profile.best_columns,
        "best_family_size": profile.best_family_size,
        "best_frequencies": profile.best_frequencies,
        "counterexample_columns": profile.counterexample_columns,
        "counterexample_family_size": profile.counterexample_family_size,
        "counterexample_frequencies": profile.counterexample_frequencies,
        "signature": profile.signature,
    })
}

unsafe extern "C" fn column_quotient_scan(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 5, "column_quotient_scan")?;
        let profile = column_quotient::scan(
            u32::try_from(int_arg(args, 0, "column_quotient_scan")?)
                .map_err(|_| "generator_count must be nonnegative u32".to_string())?,
            u32::try_from(int_arg(args, 1, "column_quotient_scan")?)
                .map_err(|_| "column_count must be nonnegative u32".to_string())?,
            u32::try_from(int_arg(args, 2, "column_quotient_scan")?)
                .map_err(|_| "maximum_column_weight must be nonnegative u32".to_string())?,
            u32::try_from(int_arg(args, 3, "column_quotient_scan")?)
                .map_err(|_| "minimum_family_size must be nonnegative u32".to_string())?,
            u32::try_from(int_arg(args, 4, "column_quotient_scan")?)
                .map_err(|_| "maximum_family_size must be nonnegative u32".to_string())?,
        )?;
        Ok(column_quotient_json(profile))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn column_quotient_profile(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "column_quotient_profile")?;
        let encoded_columns = string_arg(args, 0, "column_quotient_profile")?;
        let columns: Vec<u32> = serde_json::from_str(&encoded_columns)
            .map_err(|error| format!("column_quotient_profile columns: {error}"))?;
        let generator_count = u32::try_from(int_arg(args, 1, "column_quotient_profile")?)
            .map_err(|_| "generator_count must be nonnegative u32".to_string())?;
        let profile = column_quotient::profile(&columns, generator_count)?;
        Ok(json!({
            "generator_count": profile.generator_count,
            "columns": profile.columns,
            "family": profile.family,
            "size": profile.family.len(),
            "frequencies": profile.frequencies,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn column_quotient_profiles(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 2, "column_quotient_profiles")?;
        let encoded_batches = string_arg(args, 0, "column_quotient_profiles")?;
        let batches: Vec<Vec<u32>> = serde_json::from_str(&encoded_batches)
            .map_err(|error| format!("column_quotient_profiles columns: {error}"))?;
        if batches.len() > 4096 {
            return Err("column_quotient_profiles accepts at most 4096 batches".to_string());
        }
        let generator_count = u32::try_from(int_arg(args, 1, "column_quotient_profiles")?)
            .map_err(|_| "generator_count must be nonnegative u32".to_string())?;
        let profiles = batches
            .iter()
            .map(|columns| {
                let profile = column_quotient::profile(columns, generator_count)?;
                Ok(json!({
                    "columns": profile.columns,
                    "size": profile.family.len(),
                    "frequencies": profile.frequencies,
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(json!(profiles))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn column_quotient_mutation_lane(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 7, "column_quotient_mutation_lane")?;
        let encoded_columns = string_arg(args, 0, "column_quotient_mutation_lane")?;
        let columns: Vec<u32> = serde_json::from_str(&encoded_columns)
            .map_err(|error| format!("column_quotient_mutation_lane columns: {error}"))?;
        let nonnegative = |index, name| {
            u32::try_from(int_arg(args, index, "column_quotient_mutation_lane")?)
                .map_err(|_| format!("{name} must be nonnegative u32"))
        };
        let seed = u64::try_from(int_arg(args, 4, "column_quotient_mutation_lane")?)
            .map_err(|_| "seed must be nonnegative".to_string())?;
        let lane = column_quotient::search_mutations(
            &columns,
            nonnegative(1, "generator_count")?,
            nonnegative(2, "maximum_column_weight")?,
            nonnegative(3, "trials")?,
            seed,
            nonnegative(5, "minimum_family_size")?,
            nonnegative(6, "maximum_family_size")?,
        )?;
        let (margin, _, _) = {
            let family_size = lane.best.family.len() as i64;
            let margin = lane
                .best
                .frequencies
                .iter()
                .map(|frequency| 2 * i64::from(*frequency) - family_size)
                .max()
                .unwrap_or(0);
            (margin, lane.excess_sum, lane.abundant_count)
        };
        Ok(json!({
            "columns": lane.best.columns,
            "size": lane.best.family.len(),
            "frequencies": lane.best.frequencies,
            "margin": margin,
            "excess_sum": lane.excess_sum,
            "abundant_count": lane.abundant_count,
            "connected": lane.connected,
            "has_maximum_weight_column": lane.has_maximum_weight_column,
            "evaluated": lane.evaluated,
            "in_window": lane.in_window,
            "best_window_margin": lane.best_window_margin,
            "window_counts": lane.window_counts,
            "window_minimum_margins": lane.window_minimum_margins,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe extern "C" fn column_quotient_range_scan(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 6, "column_quotient_range_scan")?;
        let value = |index, name| {
            u32::try_from(int_arg(args, index, "column_quotient_range_scan")?)
                .map_err(|_| format!("{name} must be nonnegative u32"))
        };
        Ok(column_quotient_json(column_quotient::scan_range(
            value(0, "generator_count")?,
            value(1, "minimum_column_count")?,
            value(2, "maximum_column_count")?,
            value(3, "maximum_column_weight")?,
            value(4, "minimum_family_size")?,
            value(5, "maximum_family_size")?,
        )?))
    })();
    result.map_or_else(fail, return_json)
}
