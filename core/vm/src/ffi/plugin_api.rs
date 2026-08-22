// The C ABI a plugin sees: the descriptor and function-table layout, the
// `make_*`/`as_*` value accessors, export registration, and native invocation.
#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct RadExtensionDescriptor {
    pub abi_version: u32,
    pub extension_id: *const c_char,
    pub extension_version: *const c_char,
    pub abi_contract_json: *const c_char,
}

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct RadNativeFunctionDecl {
    pub name: *const c_char,
    pub func: NativeFnPtr,
    pub arity: u32,
    pub effects: *const *const c_char,
    pub effect_count: usize,
    pub signature: *const c_char,
    pub deterministic: bool,
    pub replayable: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct RadPluginApi {
    pub ctx: *mut c_void,
    pub register_fn: unsafe extern "C" fn(ctx: *mut c_void, decl: *const RadNativeFunctionDecl),
    pub make_nil: unsafe extern "C" fn() -> u64,
    pub make_int: unsafe extern "C" fn(i64) -> u64,
    pub make_float: unsafe extern "C" fn(f64) -> u64,
    pub make_bool: unsafe extern "C" fn(bool) -> u64,
    pub make_string: unsafe extern "C" fn(*const c_char) -> u64,
    pub make_host_handle: unsafe extern "C" fn(*const c_char, u64) -> u64,
    pub as_int: unsafe extern "C" fn(u64, *mut i64) -> bool,
    pub as_float: unsafe extern "C" fn(u64, *mut f64) -> bool,
    pub as_bool: unsafe extern "C" fn(u64, *mut bool) -> bool,
    pub as_string_ptr: unsafe extern "C" fn(u64) -> *const c_char,
    pub as_string_len: unsafe extern "C" fn(u64) -> usize,
    pub as_host_handle: unsafe extern "C" fn(u64, *mut u64) -> bool,
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
    let func = match &native.execution {
        NativeExecution::InProcess(func) => func,
        NativeExecution::Isolated(worker) => return worker.invoke(&native.name, args, target),
        NativeExecution::Replay => {
            return Err(format!(
                "replay stub {}() reached live native execution",
                native.name
            ))
        }
    };
    // Clear an error left by a misbehaving earlier extension before invoking
    // the next function on this thread.
    let _ = take_native_error();
    let raw_args = args.iter().map(|value| value.to_raw()).collect::<Vec<_>>();
    NATIVE_CALL_OWNER.with(|owner| {
        *owner.borrow_mut() = Some(native.extension.digest().to_string());
    });
    let result_raw = unsafe { (*func)(raw_args.as_ptr(), raw_args.len()) };
    NATIVE_CALL_OWNER.with(|owner| {
        owner.borrow_mut().take();
    });
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

/// WebAssembly builds retain the callable value shape but cannot invoke a
/// host dynamic-library pointer. Keeping
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
unsafe extern "C" fn api_make_host_handle(type_name: *const c_char, token: u64) -> u64 {
    if type_name.is_null() {
        api_set_error(c"make_host_handle requires a non-null type name".as_ptr());
        return Value::NIL.to_raw();
    }
    let Some(owner_digest) = NATIVE_CALL_OWNER.with(|owner| owner.borrow().clone()) else {
        api_set_error(c"host handles may only be minted during a native call".as_ptr());
        return Value::NIL.to_raw();
    };
    let type_name = CStr::from_ptr(type_name).to_string_lossy().into_owned();
    if type_name.is_empty() {
        api_set_error(c"host handle type name cannot be empty".as_ptr());
        return Value::NIL.to_raw();
    }
    let handle = HostHandleToken {
        owner_digest,
        type_name,
        token,
    };
    FFI_GC
        .with(|gc| Value::from_host_handle(&mut *gc.borrow_mut(), handle))
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
unsafe extern "C" fn api_as_host_handle(v: u64, out: *mut u64) -> bool {
    let value = unsafe { Value::from_raw_unchecked(v) };
    let Some(handle) = value.as_host_handle() else {
        return false;
    };
    let owned = NATIVE_CALL_OWNER.with(|owner| {
        owner
            .borrow()
            .as_deref()
            .is_some_and(|digest| digest == handle.owner_digest)
    });
    if !owned {
        return false;
    }
    if !out.is_null() {
        *out = handle.token;
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
struct RegistrationContext {
    functions: Vec<(
        String,
        NativeFnPtr,
        u32,
        NativeEffectSet,
        String,
        bool,
        bool,
    )>,
    errors: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" fn api_register_fn(ctx: *mut c_void, decl: *const RadNativeFunctionDecl) {
    if ctx.is_null() || decl.is_null() {
        return;
    }
    let context = &mut *(ctx as *mut RegistrationContext);
    let decl = &*decl;
    if decl.name.is_null() {
        context
            .errors
            .push("native export has a null name".to_string());
        return;
    }
    if decl.effect_count > 0 && decl.effects.is_null() {
        context
            .errors
            .push("native export has a null effects array".to_string());
        return;
    }
    let name = CStr::from_ptr(decl.name).to_string_lossy().into_owned();
    if name.is_empty() {
        context
            .errors
            .push("native export name cannot be empty".to_string());
        return;
    }
    if decl.signature.is_null() {
        context
            .errors
            .push(format!("native export `{name}` has a null signature"));
        return;
    }
    let signature = CStr::from_ptr(decl.signature)
        .to_string_lossy()
        .into_owned();
    if signature.trim().is_empty() || !signature.contains("->") {
        context
            .errors
            .push(format!("native export `{name}` has an invalid signature"));
        return;
    }
    if context
        .functions
        .iter()
        .any(|(existing, ..)| existing == &name)
    {
        context
            .errors
            .push(format!("duplicate native export `{name}`"));
        return;
    }
    let effect_ptrs = if decl.effect_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(decl.effects, decl.effect_count)
    };
    let mut raw_effects = Vec::with_capacity(effect_ptrs.len());
    for effect in effect_ptrs {
        if effect.is_null() {
            context
                .errors
                .push(format!("native export `{name}` has a null effect"));
            return;
        }
        raw_effects.push(CStr::from_ptr(*effect).to_string_lossy().into_owned());
    }
    match NativeEffectSet::parse(&raw_effects) {
        Ok(effects) => context.functions.push((
            name,
            decl.func,
            decl.arity,
            effects,
            signature,
            decl.deterministic,
            decl.replayable,
        )),
        Err(error) => context
            .errors
            .push(format!("native export `{name}`: {error}")),
    }
}
