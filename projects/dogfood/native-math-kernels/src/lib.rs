//! Project-owned acceleration for the computational-mathematics dogfoods.
//!
//! RAD's VM intentionally knows nothing about these algorithms.  They cross
//! the existing generic native-extension ABI as canonical JSON so projects
//! can optimize a tight loop without turning one experiment into language
//! semantics.

mod affine_parity;
mod boolean_lattice;

use serde_json::{json, Value as JsonValue};
use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

type NativeFnPtr = unsafe extern "C" fn(args: *const u64, argc: usize) -> u64;

#[repr(C)]
pub struct RadPluginApi {
    ctx: *mut c_void,
    register_fn:
        unsafe extern "C" fn(*mut c_void, *const c_char, NativeFnPtr, u32),
    make_nil: unsafe extern "C" fn() -> u64,
    make_int: unsafe extern "C" fn(i64) -> u64,
    make_float: unsafe extern "C" fn(f64) -> u64,
    make_bool: unsafe extern "C" fn(bool) -> u64,
    make_string: unsafe extern "C" fn(*const c_char) -> u64,
    as_int: unsafe extern "C" fn(u64, *mut i64) -> bool,
    as_float: unsafe extern "C" fn(u64, *mut f64) -> bool,
    as_bool: unsafe extern "C" fn(u64, *mut bool) -> bool,
    as_string_ptr: unsafe extern "C" fn(u64) -> *const c_char,
    as_string_len: unsafe extern "C" fn(u64) -> usize,
    set_error: unsafe extern "C" fn(*const c_char),
}

#[derive(Clone, Copy)]
struct HostApi {
    make_nil: unsafe extern "C" fn() -> u64,
    make_string: unsafe extern "C" fn(*const c_char) -> u64,
    as_int: unsafe extern "C" fn(u64, *mut i64) -> bool,
    as_string_ptr: unsafe extern "C" fn(u64) -> *const c_char,
    as_string_len: unsafe extern "C" fn(u64) -> usize,
    set_error: unsafe extern "C" fn(*const c_char),
}

static HOST: OnceLock<HostApi> = OnceLock::new();

fn host() -> &'static HostApi {
    HOST.get().expect("RAD initialized the extension")
}

fn fail(message: impl AsRef<str>) -> u64 {
    let message = CString::new(message.as_ref())
        .unwrap_or_else(|_| CString::new("native kernel error contained NUL").unwrap());
    unsafe {
        (host().set_error)(message.as_ptr());
        (host().make_nil)()
    }
}

fn arg_slice<'a>(args: *const u64, argc: usize) -> Result<&'a [u64], String> {
    if argc == 0 {
        return Ok(&[]);
    }
    if args.is_null() {
        return Err("native kernel received a null argument array".to_string());
    }
    Ok(unsafe { std::slice::from_raw_parts(args, argc) })
}

fn int_arg(args: &[u64], index: usize, operation: &str) -> Result<i64, String> {
    let raw = *args
        .get(index)
        .ok_or_else(|| format!("{operation} is missing argument {}", index + 1))?;
    let mut value = 0i64;
    if unsafe { (host().as_int)(raw, &mut value) } {
        Ok(value)
    } else {
        Err(format!("{operation} argument {} must be int", index + 1))
    }
}

fn string_arg(args: &[u64], index: usize, operation: &str) -> Result<String, String> {
    let raw = *args
        .get(index)
        .ok_or_else(|| format!("{operation} is missing argument {}", index + 1))?;
    let pointer = unsafe { (host().as_string_ptr)(raw) };
    if pointer.is_null() {
        return Err(format!("{operation} argument {} must be str", index + 1));
    }
    let length = unsafe { (host().as_string_len)(raw) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) };
    String::from_utf8(bytes.to_vec())
        .map_err(|_| format!("{operation} argument {} is not UTF-8", index + 1))
}

fn integer_list(args: &[u64], operation: &str) -> Result<Vec<i64>, String> {
    let encoded = string_arg(args, 0, operation)?;
    serde_json::from_str::<Vec<i64>>(&encoded)
        .map_err(|error| format!("{operation} expects a JSON list<int>: {error}"))
}

fn return_json(value: JsonValue) -> u64 {
    match CString::new(value.to_string()) {
        Ok(encoded) => unsafe { (host().make_string)(encoded.as_ptr()) },
        Err(_) => fail("native kernel produced JSON containing NUL"),
    }
}

fn exact_arity(args: &[u64], expected: usize, operation: &str) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "{operation} expects {expected} arguments, got {}",
            args.len()
        ))
    }
}

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
            "frontier": state.deletable_members,
            "effective_frontier": state.effective_deletable_members,
            "separating": state.separating,
        }))
    })();
    result.map_or_else(fail, return_json)
}

fn nonnegative_u64(value: i64, name: &str) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| format!("{name} must be non-negative"))
}

fn residue_profile_json(
    profile: affine_parity::ResidueLaneProfile,
) -> Result<JsonValue, String> {
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

        let profiles = std::thread::scope(|scope| {
            let handles = (0..lane_count)
                .map(|lane_index| {
                    scope.spawn(move || {
                        affine_parity::residue_lane_profile(
                            multiplier,
                            addend,
                            depth,
                            verified_power,
                            lane_index,
                            lane_count,
                        )
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| "affine residue worker panicked".to_string())?
                })
                .collect::<Result<Vec<_>, String>>()
        })?;
        profiles
            .into_iter()
            .map(residue_profile_json)
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array)
    })();
    result.map_or_else(fail, return_json)
}

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
    (api.register_fn)(api.ctx, name.as_ptr(), function, arity);
}

#[no_mangle]
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
    register(api, "affine_residue_profile_json", affine_residue_profile, 6);
    register(
        api,
        "affine_residue_profiles_json",
        affine_residue_profiles,
        5,
    );
    register(api, "affine_cycle_profile_json", affine_cycle_profile, 4);
}
