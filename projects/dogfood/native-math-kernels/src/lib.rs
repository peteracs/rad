//! Project-owned acceleration for the computational-mathematics dogfoods.
//!
//! RAD's VM intentionally knows nothing about these algorithms.  They cross
//! the existing generic native-extension ABI as canonical JSON so projects
//! can optimize a tight loop without turning one experiment into language
//! semantics.

mod affine_frontier;
mod affine_parity;
mod boolean_lattice;
mod column_quotient;
mod sparse_slope;
mod zeta_cover;

// Public lane receipts must remain stable across the independent production
// and differential-oracle implementations. Monolithic searches may choose a
// different internal split without changing externally assigned lane work.
const AFFINE_SPARSE_LANE_SPLIT_WEIGHT: u32 = 6;

use rayon::prelude::*;
use serde_json::{json, Value as JsonValue};
use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

type NativeFnPtr = unsafe extern "C" fn(args: *const u64, argc: usize) -> u64;

#[repr(C)]
pub struct RadExtensionDescriptor {
    abi_version: u32,
    extension_id: *const c_char,
    extension_version: *const c_char,
    abi_contract_json: *const c_char,
}

unsafe impl Sync for RadExtensionDescriptor {}

#[repr(C)]
pub struct RadNativeFunctionDecl {
    name: *const c_char,
    function: NativeFnPtr,
    arity: u32,
    effects: *const *const c_char,
    effect_count: usize,
    signature: *const c_char,
    deterministic: bool,
    replayable: bool,
}

#[repr(C)]
pub struct RadPluginApi {
    ctx: *mut c_void,
    register_fn: unsafe extern "C" fn(*mut c_void, *const RadNativeFunctionDecl),
    make_nil: unsafe extern "C" fn() -> u64,
    make_int: unsafe extern "C" fn(i64) -> u64,
    make_float: unsafe extern "C" fn(f64) -> u64,
    make_bool: unsafe extern "C" fn(bool) -> u64,
    make_string: unsafe extern "C" fn(*const c_char) -> u64,
    make_host_handle: unsafe extern "C" fn(*const c_char, u64) -> u64,
    as_int: unsafe extern "C" fn(u64, *mut i64) -> bool,
    as_float: unsafe extern "C" fn(u64, *mut f64) -> bool,
    as_bool: unsafe extern "C" fn(u64, *mut bool) -> bool,
    as_string_ptr: unsafe extern "C" fn(u64) -> *const c_char,
    as_string_len: unsafe extern "C" fn(u64) -> usize,
    as_host_handle: unsafe extern "C" fn(u64, *mut u64) -> bool,
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
// Host ABI plumbing and domain bindings remain separate responsibilities while
// sharing the private, process-local registration state above.
include!("lib/host_api.rs");
include!("lib/lattice_bindings.rs");
include!("lib/quotient_bindings.rs");
include!("lib/affine_bindings.rs");
include!("lib/zeta_bindings.rs");
include!("lib/register.rs");
