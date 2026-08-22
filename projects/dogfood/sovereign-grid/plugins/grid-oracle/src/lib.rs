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
static EXTENSION_ID: &[u8] = b"rad.dogfood.sovereign-grid.oracle\0";
static EXTENSION_VERSION: &[u8] = b"1.0.0\0";
static ABI_CONTRACT: &str = concat!(
    r#"{"contract_version":1,"calling_convention":"C","types":[{"name":"DispatchPacket","size":40,"alignment":8,"fields":[{"name":"mission","type_name":"MissionId","offset":0,"size":8},{"name":"asset","type_name":"AssetId","offset":8,"size":8},{"name":"zone","type_name":"ZoneId","offset":16,"size":2},{"name":"credits","type_name":"Credits","offset":24,"size":8},{"name":"flags","type_name":"AssetFlags","offset":32,"size":2}]},{"name":"DispatchScore","size":8,"alignment":4,"fields":[{"name":"score","type_name":"u32","offset":0,"size":4},{"name":"generation","type_name":"u32","offset":4,"size":4}]}]}"#,
    "\0"
);
static DESCRIPTOR: RadExtensionDescriptor = RadExtensionDescriptor {
    abi_version: 3,
    extension_id: EXTENSION_ID.as_ptr().cast(),
    extension_version: EXTENSION_VERSION.as_ptr().cast(),
    abi_contract_json: ABI_CONTRACT.as_ptr().cast(),
};

fn host() -> &'static HostApi {
    HOST.get().expect("RAD initialized the grid oracle")
}

fn fail(message: &str) -> u64 {
    let encoded = CString::new(message).expect("plugin error contains no NUL");
    unsafe {
        (host().set_error)(encoded.as_ptr());
        (host().make_nil)()
    }
}

unsafe fn bytes_argument(args: *const u64, argc: usize) -> Result<Vec<u8>, String> {
    if argc != 1 || args.is_null() {
        return Err(format!("score_dispatch expects one DispatchPacket, got {argc}"));
    }
    let pointer = (host().as_string_ptr)(*args);
    if pointer.is_null() {
        return Err("score_dispatch transport must be an encoded byte list".to_string());
    }
    let encoded = std::slice::from_raw_parts(pointer.cast::<u8>(), (host().as_string_len)(*args));
    serde_json::from_slice(encoded)
        .map_err(|error| format!("score_dispatch transport is malformed: {error}"))
}

unsafe extern "C" fn score_dispatch(args: *const u64, argc: usize) -> u64 {
    let input = match bytes_argument(args, argc) {
        Ok(input) if input.len() == 40 => input,
        Ok(input) => return fail(&format!("DispatchPacket size mismatch: expected 40, got {}", input.len())),
        Err(error) => return fail(&error),
    };
    let priority = input[32] & 0x08 != 0;
    let score = 10_u32 + if priority { 50 } else { 0 };
    let mut output = vec![0_u8; 8];
    output[0..4].copy_from_slice(&score.to_le_bytes());
    output[4..8].copy_from_slice(&1_u32.to_le_bytes());
    let encoded = CString::new(serde_json::to_string(&output).expect("byte list serializes"))
        .expect("JSON contains no NUL");
    (host().make_string)(encoded.as_ptr())
}

unsafe extern "C" fn generation_probe(_args: *const u64, argc: usize) -> u64 {
    if argc != 0 {
        return fail("grid_generation expects no arguments");
    }
    let value = CString::new("grid-oracle-generation-1").unwrap();
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
    register(api, b"score_dispatch\0", b"(DispatchPacket)->DispatchScore\0", score_dispatch, 1);
    register(api, b"grid_generation\0", b"()->str\0", generation_probe, 0);
}
