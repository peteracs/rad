//! Native dynamic library bridge (`rad_extension_init`).
//!
//! C callbacks cannot carry a Rust `&mut GcHeap`, so heap allocations during plugin init use a
//! thread-local `FFI_GC`. When [`load_plugin`] returns, that arena is **merged** into the VM
//! `merge_into` heap so any `Value`s produced by `make_*` live in the same arena as the rest of
//! the VM. Do not interpret raw `u64` handles as valid after a different VM or plugin load unless
//! their heap has been merged.

use crate::gc::GcHeap;
use crate::value::NativeFnInfo;
use crate::value::Value;
use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::ffi::{c_char, c_void, CStr};
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static FFI_GC: RefCell<GcHeap> = const { RefCell::new(GcHeap::new()) };
}

pub type NativeFnPtr = unsafe extern "C" fn(args: *const u64, argc: usize) -> u64;

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct RadPluginApi {
    pub ctx: *mut c_void,
    pub register_fn:
        unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char, func: NativeFnPtr, arity: u32),
    pub make_nil: unsafe extern "C" fn() -> u64,
    pub make_int: unsafe extern "C" fn(i64) -> u64,
    pub make_float: unsafe extern "C" fn(f64) -> u64,
    pub make_bool: unsafe extern "C" fn(bool) -> u64,
    pub make_string: unsafe extern "C" fn(*const c_char) -> u64,
    pub as_int: unsafe extern "C" fn(u64, *mut i64) -> bool,
    pub as_float: unsafe extern "C" fn(u64, *mut f64) -> bool,
    pub as_bool: unsafe extern "C" fn(u64, *mut bool) -> bool,
    pub as_string_ptr: unsafe extern "C" fn(u64) -> *const c_char,
    pub as_string_len: unsafe extern "C" fn(u64) -> usize,
    pub set_error: unsafe extern "C" fn(*const c_char),
}

thread_local! {
    static NATIVE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn take_native_error() -> Option<String> {
    NATIVE_ERROR.with(|e| e.borrow_mut().take())
}

#[cfg(not(target_arch = "wasm32"))]
fn drain_native_values_into(target: &mut GcHeap) {
    FFI_GC.with(|heap| {
        let mut drained = GcHeap::new();
        std::mem::swap(&mut *heap.borrow_mut(), &mut drained);
        target.merge(drained);
    });
}

/// Invoke a registered extension function and adopt every heap value it
/// produced into the calling VM before returning.
///
/// The extension ABI intentionally exposes scalar constructors instead of a
/// VM pointer. Those constructors allocate in a thread-local transfer arena;
/// this function is the single ownership boundary that drains that arena into
/// the current VM for both successful and failed calls.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn invoke_native(
    native: &NativeFnInfo,
    args: &[Value],
    target: &mut GcHeap,
) -> Result<Value, String> {
    // Clear an error left by a misbehaving earlier extension before invoking
    // the next function on this thread.
    let _ = take_native_error();
    let raw_args = args.iter().map(|value| value.to_raw()).collect::<Vec<_>>();
    let result_raw = unsafe { (native.func)(raw_args.as_ptr(), raw_args.len()) };
    let error = take_native_error();
    drain_native_values_into(target);
    if let Some(error) = error {
        return Err(error);
    }
    // SAFETY: the ABI requires results to be immediate values or values made
    // through the supplied constructors. The latter were just adopted by
    // `target`, so the returned handle is now owned by this VM.
    Ok(unsafe { Value::from_raw_unchecked(result_raw) })
}

/// WebAssembly builds retain the callable value shape for bytecode and wire
/// compatibility, but cannot invoke a host dynamic-library pointer. Keeping
/// the unsupported-target policy at this boundary prevents target-specific
/// branches from spreading through the generic VM executor.
#[cfg(target_arch = "wasm32")]
pub(crate) fn invoke_native(
    _native: &NativeFnInfo,
    _args: &[Value],
    _target: &mut GcHeap,
) -> Result<Value, String> {
    Err("native extensions are not supported on wasm32".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_set_error(msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let s = CStr::from_ptr(msg).to_string_lossy().into_owned();
    NATIVE_ERROR.with(|e| *e.borrow_mut() = Some(s));
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_make_nil() -> u64 {
    Value::NIL.to_raw()
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_make_int(v: i64) -> u64 {
    FFI_GC
        .with(|g| Value::from_int(&mut *g.borrow_mut(), v))
        .to_raw()
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_make_float(v: f64) -> u64 {
    Value::from_float(v).to_raw()
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_make_bool(v: bool) -> u64 {
    Value::from_bool(v).to_raw()
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_make_string(s: *const c_char) -> u64 {
    if s.is_null() {
        return Value::NIL.to_raw();
    }
    let str_val = CStr::from_ptr(s).to_string_lossy().into_owned();
    FFI_GC
        .with(|g| Value::from_string(&mut *g.borrow_mut(), str_val))
        .to_raw()
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_as_int(v: u64, out: *mut i64) -> bool {
    // SAFETY: C callers may only pass handles produced by this RAD ABI.
    let val = unsafe { Value::from_raw_unchecked(v) };
    if let Some(i) = val.as_int() {
        if !out.is_null() {
            *out = i;
        }
        true
    } else {
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_as_float(v: u64, out: *mut f64) -> bool {
    // SAFETY: C callers may only pass handles produced by this RAD ABI.
    let val = unsafe { Value::from_raw_unchecked(v) };
    if let Some(f) = val.as_float() {
        if !out.is_null() {
            *out = f;
        }
        true
    } else {
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_as_bool(v: u64, out: *mut bool) -> bool {
    // SAFETY: C callers may only pass handles produced by this RAD ABI.
    let val = unsafe { Value::from_raw_unchecked(v) };
    if let Some(b) = val.as_bool() {
        if !out.is_null() {
            *out = b;
        }
        true
    } else {
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_as_string_ptr(v: u64) -> *const c_char {
    // SAFETY: C callers may only pass handles produced by this RAD ABI.
    let val = unsafe { Value::from_raw_unchecked(v) };
    if let Some(s) = val.as_str() {
        s.as_ptr() as *const c_char
    } else {
        std::ptr::null()
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_as_string_len(v: u64) -> usize {
    // SAFETY: C callers may only pass handles produced by this RAD ABI.
    let val = unsafe { Value::from_raw_unchecked(v) };
    if let Some(s) = val.as_str() {
        s.len()
    } else {
        0
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct RegistrationContext {
    functions: Vec<(String, NativeFnInfo)>,
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_register_fn(
    ctx: *mut c_void,
    name: *const c_char,
    func: NativeFnPtr,
    arity: u32,
) {
    if ctx.is_null() || name.is_null() {
        return;
    }
    let context = &mut *(ctx as *mut RegistrationContext);
    let name_str = CStr::from_ptr(name).to_string_lossy().into_owned();
    context.functions.push((
        name_str.clone(),
        NativeFnInfo {
            name: name_str,
            func,
            arity,
        },
    ));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_plugin(
    path: &str,
    merge_into: &mut GcHeap,
) -> Result<(Vec<(String, NativeFnInfo)>, libloading::Library), String> {
    unsafe {
        let requested = Path::new(path);
        let platform_path = if requested.extension().is_none() {
            let candidate = requested.with_extension(std::env::consts::DLL_EXTENSION);
            candidate.exists().then_some(candidate)
        } else {
            None
        };
        let resolved: PathBuf = platform_path.unwrap_or_else(|| requested.to_path_buf());
        let lib = libloading::Library::new(&resolved)
            .map_err(|e| format!("Failed to load plugin '{}': {}", resolved.display(), e))?;

        type InitFn = unsafe extern "C" fn(*const RadPluginApi);
        let init_fn: libloading::Symbol<InitFn> = lib
            .get(b"rad_extension_init\0")
            .map_err(|e| format!("Failed to find rad_extension_init: {}", e))?;

        let mut context = RegistrationContext {
            functions: Vec::new(),
        };

        let api = RadPluginApi {
            ctx: &mut context as *mut RegistrationContext as *mut c_void,
            register_fn: api_register_fn,
            make_nil: api_make_nil,
            make_int: api_make_int,
            make_float: api_make_float,
            make_bool: api_make_bool,
            make_string: api_make_string,
            as_int: api_as_int,
            as_float: api_as_float,
            as_bool: api_as_bool,
            as_string_ptr: api_as_string_ptr,
            as_string_len: api_as_string_len,
            set_error: api_set_error,
        };

        init_fn(&api);

        drain_native_values_into(merge_into);

        Ok((context.functions, lib))
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_plugin(
    _path: &str,
    _merge_into: &mut GcHeap,
) -> Result<(Vec<(String, NativeFnInfo)>, ()), String> {
    Err("Plugins are not supported on wasm32".to_string())
}
