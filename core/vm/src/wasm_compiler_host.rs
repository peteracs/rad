//! Phase 3: load `compiler.wasm` under wasmtime; host import `env.vfs_read` + guest exports.

use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
use std::sync::{Arc, Mutex};

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
use wasmtime::{Caller, Engine, Linker, Memory, Module, Store, TypedFunc};

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
use crate::compiler_abi::{
    pack_u64, unpack_u64, WasmDiagnosticRecord, DIAG_BLOB_HEADER, EXPORT_RAD_CHECK,
    EXPORT_RAD_INIT, EXPORT_RAD_QUERY_LSP, EXPORT_RAD_UPDATE_BUFFER, MEMORY_EXPORT,
    VFS_READ_IMPORT,
};

/// Resolved diagnostic for LSP / CLI (message bytes read from guest memory).
#[derive(Clone, Debug)]
pub struct WasmDiagnostic {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

/// Bytes to load as `compiler.wasm`: path from `RAD_COMPILER_WASM`, else the in-tree stub.
pub fn compiler_wasm_bytes_from_env() -> Result<Vec<u8>, String> {
    match std::env::var("RAD_COMPILER_WASM") {
        Ok(p) => std::fs::read(&p).map_err(|e| format!("RAD_COMPILER_WASM {p}: {e}")),
        Err(_) => Ok(crate::wasm_binary_emit::emit_compiler_reactor_stub_module()),
    }
}

#[derive(Default, Clone)]
pub struct VfsState {
    pub files: HashMap<String, Vec<u8>>,
    /// When set, unresolved paths try `join(path)` after overlay and absolute `read(path)` fail.
    pub fallback_dir: Option<PathBuf>,
}

impl VfsState {
    pub fn insert_str(&mut self, path: impl Into<String>, contents: impl Into<String>) {
        self.files.insert(path.into(), contents.into().into_bytes());
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|v| v.as_slice())
    }
}

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
struct HostCtx {
    vfs: Arc<Mutex<VfsState>>,
    guest_write_cursor: u32,
}

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
fn vfs_read_impl(mut caller: Caller<'_, HostCtx>, path_ptr: u32, path_len: u32) -> u64 {
    let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m,
        None => return 0,
    };
    let path = read_guest_string(&caller, &mem, path_ptr, path_len).unwrap_or_default();
    let data: Vec<u8> = caller
        .data()
        .vfs
        .lock()
        .ok()
        .map(|g| {
            if let Some(b) = g.files.get(&path).cloned() {
                return b;
            }
            if let Ok(b) = std::fs::read(&path) {
                return b;
            }
            if let Some(dir) = &g.fallback_dir {
                if let Ok(b) = std::fs::read(dir.join(&path)) {
                    return b;
                }
            }
            Vec::new()
        })
        .unwrap_or_default();

    let base = {
        let ctx = caller.data_mut();
        ctx.guest_write_cursor
    };
    let len_u = data.len() as u32;
    let need = base.saturating_add(len_u);
    let cap = mem.data_size(&caller) as u32;
    if need > cap {
        let pages = (need as u64).div_ceil(65536);
        let _ = mem.grow(&mut caller, pages);
    }
    if mem.write(&mut caller, base as usize, &data).is_err() {
        return 0;
    }
    caller.data_mut().guest_write_cursor = need;
    pack_u64(base, len_u)
}

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
fn read_guest_string(
    caller: &Caller<'_, HostCtx>,
    mem: &Memory,
    ptr: u32,
    len: u32,
) -> Result<String, String> {
    let mut buf = vec![0u8; len as usize];
    mem.read(caller, ptr as usize, &mut buf)
        .map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
pub struct WasmCompilerHost {
    store: Store<HostCtx>,
    memory: Memory,
    rad_init: TypedFunc<(), i32>,
    rad_update_buffer: TypedFunc<(i32, i32, i32), ()>,
    rad_check: TypedFunc<(), i64>,
    rad_query_lsp: TypedFunc<(i32, i32, i32), i64>,
}

#[cfg(all(feature = "native-wasm-phase3", not(target_arch = "wasm32")))]
impl WasmCompilerHost {
    pub fn from_bytes(wasm: &[u8], vfs: Arc<Mutex<VfsState>>) -> Result<Self, String> {
        let engine = Engine::default();
        let module = Module::from_binary(&engine, wasm).map_err(|e| e.to_string())?;

        let mut linker = Linker::new(&engine);
        linker
            .func_wrap(
                "env",
                VFS_READ_IMPORT,
                |caller: Caller<'_, HostCtx>, path_ptr: i32, path_len: i32| -> i64 {
                    vfs_read_impl(caller, path_ptr as u32, path_len as u32) as i64
                },
            )
            .map_err(|e| e.to_string())?;

        let mut store = Store::new(
            &engine,
            HostCtx {
                vfs,
                guest_write_cursor: 65536,
            },
        );

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| e.to_string())?;

        let memory = instance
            .get_memory(&mut store, MEMORY_EXPORT)
            .ok_or_else(|| format!("missing export `{}`", MEMORY_EXPORT))?;

        let rad_init = instance
            .get_typed_func::<(), i32>(&mut store, EXPORT_RAD_INIT)
            .map_err(|e| e.to_string())?;
        let rad_update_buffer = instance
            .get_typed_func::<(i32, i32, i32), ()>(&mut store, EXPORT_RAD_UPDATE_BUFFER)
            .map_err(|e| e.to_string())?;
        let rad_check = instance
            .get_typed_func::<(), i64>(&mut store, EXPORT_RAD_CHECK)
            .map_err(|e| e.to_string())?;
        let rad_query_lsp = instance
            .get_typed_func::<(i32, i32, i32), i64>(&mut store, EXPORT_RAD_QUERY_LSP)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            store,
            memory,
            rad_init,
            rad_update_buffer,
            rad_check,
            rad_query_lsp,
        })
    }

    pub fn default_stub() -> Result<Self, String> {
        Self::from_bytes(
            &crate::wasm_binary_emit::emit_compiler_reactor_stub_module(),
            Arc::new(Mutex::new(VfsState::default())),
        )
    }

    pub fn rad_init(&mut self) -> Result<i32, String> {
        self.rad_init
            .call(&mut self.store, ())
            .map_err(|e| e.to_string())
    }

    pub fn rad_update_buffer(&mut self, offset: i32, text: &str) -> Result<(), String> {
        let bytes = text.as_bytes();
        let base = offset as usize;
        let len_u = bytes.len() as u32;
        let need = base.saturating_add(bytes.len());
        let cap = self.memory.data_size(&self.store);
        if need > cap {
            let pages = (need as u64).div_ceil(65536);
            let _ = self.memory.grow(&mut self.store, pages);
        }
        self.memory
            .write(&mut self.store, base, bytes)
            .map_err(|e| e.to_string())?;
        self.rad_update_buffer
            .call(&mut self.store, (offset, len_u as i32, 0))
            .map_err(|e| e.to_string())
    }

    pub fn rad_check(&mut self) -> Result<Vec<WasmDiagnostic>, String> {
        let packed = self
            .rad_check
            .call(&mut self.store, ())
            .map_err(|e| e.to_string())?;
        self.parse_diagnostic_blob(packed as u64)
    }

    pub fn rad_query_lsp(&mut self, a: i32, b: i32, c: i32) -> Result<Vec<u8>, String> {
        let packed = self
            .rad_query_lsp
            .call(&mut self.store, (a, b, c))
            .map_err(|e| e.to_string())?;
        self.read_blob(packed as u64)
    }

    fn read_blob(&mut self, packed: u64) -> Result<Vec<u8>, String> {
        let (ptr, len) = unpack_u64(packed);
        if ptr == 0 || len == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; len as usize];
        self.memory
            .read(&self.store, ptr as usize, &mut buf)
            .map_err(|e| e.to_string())?;
        Ok(buf)
    }

    fn parse_diagnostic_blob(&mut self, packed: u64) -> Result<Vec<WasmDiagnostic>, String> {
        let (ptr, len) = unpack_u64(packed);
        if ptr == 0 || len == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; len as usize];
        self.memory
            .read(&self.store, ptr as usize, &mut buf)
            .map_err(|e| e.to_string())?;
        if buf.len() < DIAG_BLOB_HEADER {
            return Ok(Vec::new());
        }
        let count = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
        let mut out = Vec::new();
        let mut off = DIAG_BLOB_HEADER;
        for _ in 0..count {
            if off + WasmDiagnosticRecord::SIZE > buf.len() {
                break;
            }
            let chunk = &buf[off..off + WasmDiagnosticRecord::SIZE];
            let line = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
            let col = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
            let msg_ptr = u32::from_le_bytes(chunk[8..12].try_into().unwrap());
            let msg_len = u32::from_le_bytes(chunk[12..16].try_into().unwrap());
            off += WasmDiagnosticRecord::SIZE;
            let msg_bytes = self.read_guest_slice(msg_ptr, msg_len)?;
            let message = String::from_utf8_lossy(&msg_bytes).into_owned();
            out.push(WasmDiagnostic { line, col, message });
        }
        Ok(out)
    }

    fn read_guest_slice(&mut self, ptr: u32, len: u32) -> Result<Vec<u8>, String> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; len as usize];
        self.memory
            .read(&mut self.store, ptr as usize, &mut buf)
            .map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

#[cfg(not(all(feature = "native-wasm-phase3", not(target_arch = "wasm32"))))]
pub struct WasmCompilerHost;

#[cfg(not(all(feature = "native-wasm-phase3", not(target_arch = "wasm32"))))]
impl WasmCompilerHost {
    pub fn default_stub() -> Result<Self, String> {
        Err("native-wasm-phase3 is not enabled for this build".to_string())
    }
}
