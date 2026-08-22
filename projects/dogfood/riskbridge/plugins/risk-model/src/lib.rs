use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

type NativeFn = unsafe extern "C" fn(*const u64, usize) -> u64;

#[repr(C)]
pub struct RadExtensionDescriptor {
    abi_version: u32,
    extension_id: *const c_char,
    extension_version: *const c_char,
    abi_contract_json: *const c_char,
}

unsafe impl Sync for RadExtensionDescriptor {}

#[repr(C)]
struct RadNativeFunctionDecl {
    name: *const c_char,
    function: NativeFn,
    arity: u32,
    effects: *const *const c_char,
    effect_count: usize,
    signature: *const c_char,
    deterministic: bool,
    replayable: bool,
}

#[repr(C)]
pub struct RadPluginApi {
    context: *mut c_void,
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
    as_string_ptr: unsafe extern "C" fn(u64) -> *const c_char,
    as_string_len: unsafe extern "C" fn(u64) -> usize,
    set_error: unsafe extern "C" fn(*const c_char),
}

static HOST: OnceLock<HostApi> = OnceLock::new();
static EXTENSION_ID: &[u8] = b"rad.dogfood.riskbridge.model\0";
static EXTENSION_VERSION: &[u8] = b"17.0.0\0";
static ABI_CONTRACT: &str = concat!(
    r#"{"contract_version":1,"calling_convention":"C","types":[{"name":"RiskInput","size":48,"alignment":8,"fields":[{"name":"customer","type_name":"CustomerId","offset":0,"size":8},{"name":"transaction","type_name":"TransactionId","offset":8,"size":8},{"name":"amount","type_name":"MoneyCents","offset":16,"size":8},{"name":"merchant_category","type_name":"u16","offset":24,"size":2},{"name":"country","type_name":"u16","offset":26,"size":2},{"name":"signals","type_name":"RiskSignals","offset":32,"size":8},{"name":"model","type_name":"ModelId","offset":40,"size":4}]},{"name":"RiskOutput","size":24,"alignment":8,"fields":[{"name":"score","type_name":"f32","offset":0,"size":4},{"name":"reason_mask","type_name":"u64","offset":8,"size":8},{"name":"model_version","type_name":"u32","offset":16,"size":4},{"name":"disposition","type_name":"u8","offset":20,"size":1}]}]}"#,
    "\0"
);
static DESCRIPTOR: RadExtensionDescriptor = RadExtensionDescriptor {
    abi_version: 3,
    extension_id: EXTENSION_ID.as_ptr().cast(),
    extension_version: EXTENSION_VERSION.as_ptr().cast(),
    abi_contract_json: ABI_CONTRACT.as_ptr().cast(),
};

fn host() -> &'static HostApi {
    HOST.get().expect("RAD initialized the risk plugin")
}

fn fail(message: &str) -> u64 {
    let encoded = CString::new(message).expect("static plugin error has no NUL");
    unsafe {
        (host().set_error)(encoded.as_ptr());
        (host().make_nil)()
    }
}

unsafe fn argument_bytes(args: *const u64, argc: usize) -> Result<Vec<u8>, String> {
    if argc != 1 || args.is_null() {
        return Err(format!("score_transaction expects one RiskInput, got {argc}"));
    }
    let value = *args;
    let pointer = (host().as_string_ptr)(value);
    if pointer.is_null() {
        return Err("score_transaction transport must be encoded bytes".to_string());
    }
    let length = (host().as_string_len)(value);
    let encoded = std::slice::from_raw_parts(pointer.cast::<u8>(), length);
    serde_json::from_slice(encoded)
        .map_err(|error| format!("score_transaction transport is malformed: {error}"))
}

fn little_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let field: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| "RiskInput is truncated".to_string())?
        .try_into()
        .expect("slice has exact width");
    Ok(u64::from_le_bytes(field))
}

unsafe extern "C" fn score_transaction(args: *const u64, argc: usize) -> u64 {
    let input = match argument_bytes(args, argc) {
        Ok(input) if input.len() == 48 => input,
        Ok(input) => return fail(&format!("RiskInput size mismatch: expected 48, got {}", input.len())),
        Err(error) => return fail(&error),
    };
    let amount = match little_u64(&input, 16) {
        Ok(value) => i64::from_le_bytes(value.to_le_bytes()),
        Err(error) => return fail(&error),
    };
    let signals = match little_u64(&input, 32) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    let mut points = 5_u32;
    if signals & 0x0001 != 0 { points += 10; }
    if signals & 0x0002 != 0 { points += 65; }
    if signals & 0x0004 != 0 { points += 15; }
    if signals & 0x0008 != 0 { points += 80; }
    if amount > 100_000 { points += 10; }
    let score = (points.min(99) as f32) / 100.0_f32;
    let disposition = if score >= 0.80 { 2_u8 } else if score >= 0.50 { 1_u8 } else { 0_u8 };

    let mut output = vec![0_u8; 24];
    output[0..4].copy_from_slice(&score.to_bits().to_le_bytes());
    output[8..16].copy_from_slice(&signals.to_le_bytes());
    output[16..20].copy_from_slice(&17_u32.to_le_bytes());
    output[20] = disposition;
    let encoded = CString::new(serde_json::to_string(&output).expect("byte list serializes"))
        .expect("JSON has no NUL");
    (host().make_string)(encoded.as_ptr())
}

unsafe extern "C" fn determinism_probe(_args: *const u64, argc: usize) -> u64 {
    if argc != 0 { return fail("risk_determinism_probe expects no arguments"); }
    let value = CString::new("riskbridge-generation-17-deterministic").unwrap();
    (host().make_string)(value.as_ptr())
}

unsafe fn register(
    api: &RadPluginApi,
    name: &'static [u8],
    signature: &'static [u8],
    function: NativeFn,
    arity: u32,
) {
    let declaration = RadNativeFunctionDecl {
        name: name.as_ptr().cast(),
        function,
        arity,
        effects: std::ptr::null(),
        effect_count: 0,
        signature: signature.as_ptr().cast(),
        deterministic: true,
        replayable: true,
    };
    (api.register_fn)(api.context, &declaration);
}

#[no_mangle]
pub extern "C" fn rad_extension_descriptor() -> *const RadExtensionDescriptor {
    &DESCRIPTOR
}

#[no_mangle]
pub unsafe extern "C" fn rad_extension_init(api: *const RadPluginApi) {
    let Some(api) = api.as_ref() else { return };
    let _ = HOST.set(HostApi {
        make_nil: api.make_nil,
        make_string: api.make_string,
        as_string_ptr: api.as_string_ptr,
        as_string_len: api.as_string_len,
        set_error: api.set_error,
    });
    register(
        api,
        b"score_transaction\0",
        b"(RiskInput)->RiskOutput\0",
        score_transaction,
        1,
    );
    register(
        api,
        b"risk_determinism_probe\0",
        b"()->str\0",
        determinism_probe,
        0,
    );
}
