//! Native dynamic library bridge (`rad_extension_init`).
//!
//! C callbacks cannot carry a Rust `&mut GcHeap`, so heap allocations during plugin init use a
//! thread-local [`FFI_GC`]. When [`load_plugin`] returns, that arena is **merged** into the VM
//! `merge_into` heap so any `Value`s produced by `make_*` live in the same arena as the rest of
//! the VM. Do not interpret raw `u64` handles as valid after a different VM or plugin load unless
//! their heap has been merged.

use crate::gc::GcHeap;
use crate::value::NativeFnInfo;
#[cfg(not(target_arch = "wasm32"))]
use crate::value::Value;
use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::ffi::{c_char, c_void, CStr};

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
        let lib =
            libloading::Library::new(path).map_err(|e| format!("Failed to load plugin: {}", e))?;

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

        FFI_GC.with(|g| {
            let mut drained = GcHeap::new();
            std::mem::swap(&mut *g.borrow_mut(), &mut drained);
            merge_into.merge(drained);
        });

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
