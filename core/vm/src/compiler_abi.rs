//! Phase 3: stable ABI between the Rust host and `compiler.wasm`.
//!
//! Memory contract:
//! - The guest exports linear memory (`"memory"`). The host reads/writes guest pointers only
//!   through that memory.
//! - `vfs_read` returns a **packed `u64`**: high 32 bits = pointer into guest memory, low 32 bits
//!   = byte length. A return value of `0` means “not found / empty”.
//! - `rad_check` / `rad_query_lsp` return packed `u64` the same way: ptr to a blob in guest
//!   memory, len in bytes. `(0, 0)` means no data (e.g. zero diagnostics).
//!
//! Diagnostic blob layout (little-endian, version 1):
//! - `u32` `count` — number of [`WasmDiagnosticRecord`] entries.
//! - `count` × [`WasmDiagnosticRecord`].
//!
//! All multi-byte integers in blobs are **little-endian** (WASM default). String payloads for
//! messages are UTF-8 bytes at `msg_ptr`..`msg_ptr + msg_len` in guest memory.

/// Import module / field names (WebAssembly import `"env" "vfs_read"`).
pub const ENV_MODULE: &str = "env";
pub const VFS_READ_IMPORT: &str = "vfs_read";

/// Exported memory name.
pub const MEMORY_EXPORT: &str = "memory";

/// Exported reactor entrypoints (guest functions).
pub const EXPORT_RAD_INIT: &str = "rad_init";
pub const EXPORT_RAD_UPDATE_BUFFER: &str = "rad_update_buffer";
pub const EXPORT_RAD_CHECK: &str = "rad_check";
pub const EXPORT_RAD_QUERY_LSP: &str = "rad_query_lsp";

/// Pack two `u32` values into one `u64` (high: `hi`, low: `lo`).
#[inline]
pub fn pack_u64(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

/// Unpack a `u64` from WASM `i64` return values.
#[inline]
pub fn unpack_u64(packed: u64) -> (u32, u32) {
    let hi = (packed >> 32) as u32;
    let lo = packed as u32;
    (hi, lo)
}

/// One diagnostic line in the guest ABI blob.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WasmDiagnosticRecord {
    pub line: u32,
    pub col: u32,
    pub msg_ptr: u32,
    pub msg_len: u32,
}

/// Header size in bytes: single little-endian `u32` count.
pub const DIAG_BLOB_HEADER: usize = 4;

impl WasmDiagnosticRecord {
    pub const SIZE: usize = std::mem::size_of::<Self>();
}
