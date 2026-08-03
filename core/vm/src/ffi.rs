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
#[cfg(not(target_arch = "wasm32"))]
use sha2::{Digest, Sha256};
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
pub type LoadedPlugin<L> = (
    Vec<(String, NativeFnInfo)>,
    L,
    std::sync::Arc<NativeExtensionManifest>,
);

pub const RAD_EXTENSION_ABI_VERSION: u32 = 1;
pub const NATIVE_RESOURCE_CONTRACT_VERSION: u32 = 0;

/// Stable, pointer-free identity of one loaded native implementation.
///
/// ABI v1 does not yet let a library self-declare a package version or
/// fine-grained effects, so those facts are represented honestly: the binary
/// content digest is authoritative, the version is absent, and the extension
/// is conservatively classified as host-effecting and constraint-unsafe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeExtensionManifest {
    extension_id: String,
    extension_version: Option<String>,
    abi_version: u32,
    content_digest: String,
    target: String,
    exported_functions: std::sync::Arc<[(String, u32)]>,
    declared_effects: std::sync::Arc<[String]>,
    resource_contract_version: u32,
    digest: String,
}

impl NativeExtensionManifest {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_binary(path: &Path, bytes: &[u8], exports: &[(String, u32)]) -> Self {
        let extension_id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unnamed-extension>")
            .to_string();
        let content_digest = hex::encode(Sha256::digest(bytes));
        let target = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
        let mut exported_functions = exports.to_vec();
        exported_functions.sort();
        let declared_effects = vec!["native.host".to_string()];

        let mut out =
            crate::canonical::CanonicalWriter::with_domain("rad-native-extension-manifest/v1");
        out.text(&extension_id);
        out.optional_text(None);
        out.u32(RAD_EXTENSION_ABI_VERSION);
        out.text(&content_digest);
        out.text(&target);
        out.usize(exported_functions.len());
        for (name, arity) in &exported_functions {
            out.text(name);
            out.u32(*arity);
        }
        out.usize(declared_effects.len());
        for effect in &declared_effects {
            out.text(effect);
        }
        out.u32(NATIVE_RESOURCE_CONTRACT_VERSION);
        let digest = hex::encode(Sha256::digest(out.finish()));

        Self {
            extension_id,
            extension_version: None,
            abi_version: RAD_EXTENSION_ABI_VERSION,
            content_digest,
            target,
            exported_functions: exported_functions.into(),
            declared_effects: declared_effects.into(),
            resource_contract_version: NATIVE_RESOURCE_CONTRACT_VERSION,
            digest,
        }
    }

    pub fn extension_id(&self) -> &str {
        &self.extension_id
    }

    pub fn extension_version(&self) -> Option<&str> {
        self.extension_version.as_deref()
    }

    pub fn abi_version(&self) -> u32 {
        self.abi_version
    }

    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn exported_functions(&self) -> &[(String, u32)] {
        &self.exported_functions
    }

    pub fn declared_effects(&self) -> &[String] {
        &self.declared_effects
    }

    pub fn resource_contract_version(&self) -> u32 {
        self.resource_contract_version
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn encode_manifest(&self, out: &mut crate::canonical::CanonicalWriter) {
        out.text(&self.extension_id);
        out.optional_text(self.extension_version.as_deref());
        out.u32(self.abi_version);
        out.text(&self.content_digest);
        out.text(&self.target);
        out.usize(self.exported_functions.len());
        for (name, arity) in self.exported_functions.iter() {
            out.text(name);
            out.u32(*arity);
        }
        out.usize(self.declared_effects.len());
        for effect in self.declared_effects.iter() {
            out.text(effect);
        }
        out.u32(self.resource_contract_version);
    }
}

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
    functions: Vec<(String, NativeFnPtr, u32)>,
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
    context.functions.push((name_str, func, arity));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_plugin(
    path: &str,
    merge_into: &mut GcHeap,
) -> Result<LoadedPlugin<libloading::Library>, String> {
    unsafe {
        let requested = Path::new(path);
        let platform_path = if requested.extension().is_none() {
            let candidate = requested.with_extension(std::env::consts::DLL_EXTENSION);
            candidate.exists().then_some(candidate)
        } else {
            None
        };
        let resolved: PathBuf = platform_path.unwrap_or_else(|| requested.to_path_buf());
        let binary = std::fs::read(&resolved).map_err(|error| {
            format!(
                "Failed to fingerprint plugin '{}': {}",
                resolved.display(),
                error
            )
        })?;
        let lib = libloading::Library::new(&resolved)
            .map_err(|e| format!("Failed to load plugin '{}': {}", resolved.display(), e))?;
        let loaded_binary = std::fs::read(&resolved).map_err(|error| {
            format!(
                "Failed to seal loaded plugin '{}': {}",
                resolved.display(),
                error
            )
        })?;
        if loaded_binary != binary {
            return Err(format!(
                "Plugin '{}' changed while it was being loaded",
                resolved.display()
            ));
        }

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
        let exports = context
            .functions
            .iter()
            .map(|(name, _, arity)| (name.clone(), *arity))
            .collect::<Vec<_>>();
        let manifest = std::sync::Arc::new(NativeExtensionManifest::from_binary(
            &resolved,
            &loaded_binary,
            &exports,
        ));
        let functions = context
            .functions
            .into_iter()
            .map(|(name, func, arity)| {
                let info = NativeFnInfo {
                    name: name.clone(),
                    func,
                    arity,
                    extension: std::sync::Arc::clone(&manifest),
                };
                (name, info)
            })
            .collect();

        Ok((functions, lib, manifest))
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_plugin(_path: &str, _merge_into: &mut GcHeap) -> Result<LoadedPlugin<()>, String> {
    Err("Plugins are not supported on wasm32".to_string())
}
