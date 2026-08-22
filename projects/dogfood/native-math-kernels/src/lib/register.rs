

unsafe extern "C" fn affine_cycle_profile(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 4, "affine_cycle_profile")?;
        let profile = affine_parity::affine_cycle_profile(
            nonnegative_u64(int_arg(args, 0, "affine_cycle_profile")?, "multiplier")?,
            nonnegative_u64(int_arg(args, 1, "affine_cycle_profile")?, "addend")?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 2, "affine_cycle_profile")?,
                "max_odd_steps",
            )?)
            .map_err(|_| "max_odd_steps exceeds u32".to_string())?,
            u32::try_from(nonnegative_u64(
                int_arg(args, 3, "affine_cycle_profile")?,
                "max_total_divisions",
            )?)
            .map_err(|_| "max_total_divisions exceeds u32".to_string())?,
        )?;
        Ok(json!({
            "words_tested": profile.words_tested,
            "positive_denominators": profile.positive_denominators,
            "divisible_candidates": profile.divisible_candidates,
            "exact_cycle_words": profile.exact_cycle_words,
            "nonunit_cycle_words": profile.nonunit_cycle_words,
            "unit_cycle_words": profile.unit_cycle_words,
            "first_nonunit_start": profile.first_nonunit_start.to_string(),
            "first_nonunit_valuations": profile.first_nonunit_valuations,
            "closest_q": profile.closest_q,
            "closest_divisions": profile.closest_divisions,
            "closest_gap": profile.closest_gap.to_string(),
            "signature": profile.signature,
        }))
    })();
    result.map_or_else(fail, return_json)
}

unsafe fn register(api: &RadPluginApi, name: &str, function: NativeFnPtr, arity: u32) {
    let name = CString::new(name).expect("static function name");
    let parameters = (0..arity).map(|_| "any").collect::<Vec<_>>().join(",");
    let signature = CString::new(format!("({parameters})->any")).expect("generated signature");
    let declaration = RadNativeFunctionDecl {
        name: name.as_ptr(),
        function,
        arity,
        effects: std::ptr::null(),
        effect_count: 0,
        signature: signature.as_ptr(),
        deterministic: true,
        replayable: true,
    };
    (api.register_fn)(api.ctx, &declaration);
}

static EXTENSION_ID: &[u8] = b"rad.dogfood.math-kernels\0";
static EXTENSION_VERSION: &[u8] = b"0.1.0\0";
static ABI_CONTRACT: &[u8] =
    b"{\"contract_version\":1,\"calling_convention\":\"C\",\"types\":[]}\0";
static EXTENSION_DESCRIPTOR: RadExtensionDescriptor = RadExtensionDescriptor {
    abi_version: 3,
    extension_id: EXTENSION_ID.as_ptr().cast(),
    extension_version: EXTENSION_VERSION.as_ptr().cast(),
    abi_contract_json: ABI_CONTRACT.as_ptr().cast(),
};

#[no_mangle]
pub extern "C" fn rad_extension_descriptor() -> *const RadExtensionDescriptor {
    &EXTENSION_DESCRIPTOR
}

#[no_mangle]
/// Register this extension's functions with a RAD host.
///
/// # Safety
///
/// `api` must point to a live, ABI-compatible [`RadPluginApi`] for the entire
/// call. Every callback stored by the extension must remain valid while the
/// dynamic library is loaded.
pub unsafe extern "C" fn rad_extension_init(api: *const RadPluginApi) {
    let Some(api) = api.as_ref() else {
        return;
    };
    let _ = HOST.set(HostApi {
        make_nil: api.make_nil,
        make_string: api.make_string,
        as_int: api.as_int,
        as_string_ptr: api.as_string_ptr,
        as_string_len: api.as_string_len,
        set_error: api.set_error,
    });

    register(api, "lattice_closure_json", lattice_closure, 1);
    register(api, "lattice_profile_json", lattice_profile, 2);
    register(api, "lattice_frequencies_json", lattice_frequencies, 2);
    register(
        api,
        "bitmask_rotation_orbit_json",
        bitmask_rotation_orbit,
        2,
    );
    register(
        api,
        "bitmask_rotation_representatives_json",
        bitmask_rotation_representatives,
        1,
    );
    register(api, "column_quotient_scan_json", column_quotient_scan, 5);
    register(
        api,
        "column_quotient_profile_json",
        column_quotient_profile,
        2,
    );
    register(
        api,
        "column_quotient_profiles_json",
        column_quotient_profiles,
        2,
    );
    register(
        api,
        "column_quotient_mutation_lane_json",
        column_quotient_mutation_lane,
        7,
    );
    register(
        api,
        "column_quotient_range_scan_json",
        column_quotient_range_scan,
        6,
    );
    register(api, "lattice_is_closed_json", lattice_is_closed, 1);
    register(
        api,
        "lattice_violation_count_json",
        lattice_violation_count,
        2,
    );
    register(api, "lattice_deletable_json", lattice_deletable, 2);
    register(
        api,
        "lattice_deletion_profile_json",
        lattice_deletion_profile,
        2,
    );
    register(
        api,
        "lattice_deletion_rollout_json",
        lattice_deletion_rollout,
        7,
    );
    register(
        api,
        "lattice_apply_deletion_json",
        lattice_apply_deletion,
        3,
    );
    register(
        api,
        "lattice_exchange_rollout_json",
        lattice_exchange_rollout,
        9,
    );
    register(
        api,
        "affine_residue_profile_json",
        affine_residue_profile,
        6,
    );
    register(
        api,
        "affine_residue_profiles_json",
        affine_residue_profiles,
        5,
    );
    register(
        api,
        "affine_natural_tail_profiles_json",
        affine_natural_tail_profiles,
        6,
    );
    register(
        api,
        "affine_sparse_support_profile_json",
        affine_sparse_support_profile,
        5,
    );
    register(
        api,
        "affine_sparse_support_summary_json",
        affine_sparse_support_summary,
        5,
    );
    register(
        api,
        "affine_sparse_slope_support_summary_json",
        affine_sparse_slope_support_summary,
        4,
    );
    register(
        api,
        "affine_sparse_slope_support_lane_json",
        affine_sparse_slope_support_lane,
        6,
    );
    register(
        api,
        "affine_frontier_profile_json",
        affine_frontier_profile,
        7,
    );
    register(api, "affine_cycle_profile_json", affine_cycle_profile, 4);
    register(api, "zeta_cover_lane_json", zeta_cover_lane, 3);
    register(api, "zeta_prepare_cover_json", zeta_prepare_cover, 1);
    register(
        api,
        "zeta_prepared_cover_lane_json",
        zeta_prepared_cover_lane,
        3,
    );
    register(api, "zeta_equispaced_block_json", zeta_equispaced_block, 2);
    register(api, "zeta_even_spacing_bound_json", zeta_even_spacing_bound, 1);
    register(
        api,
        "zeta_integer_spacing_bound_json",
        zeta_integer_spacing_bound,
        2,
    );
    register(api, "zeta_mt_bandwidth_scout_json", zeta_mt_bandwidth_scout, 1);
    register(
        api,
        "zeta_fourth_moment_transfer_scout_json",
        zeta_fourth_moment_transfer_scout,
        3,
    );
    register(api, "zeta_cosine_pair_scout_json", zeta_cosine_pair_scout, 1);
    register(
        api,
        "zeta_cosine_direct_sum_scout_json",
        zeta_cosine_direct_sum_scout,
        2,
    );
    register(
        api,
        "zeta_perturbed_window_scout_json",
        zeta_perturbed_window_scout,
        1,
    );
    register(
        api,
        "zeta_perturbed_window_two_mode_scout_json",
        zeta_perturbed_window_two_mode_scout,
        2,
    );
    register(
        api,
        "zeta_periodic_density_scout_json",
        zeta_periodic_density_scout,
        2,
    );
}
