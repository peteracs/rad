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
use std::fs::{File, OpenOptions};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};

// Lexical sections preserve one private semantic namespace.
include!("ffi/manifest.rs");
include!("ffi/plugin_api.rs");
include!("ffi/loading.rs");
#[cfg(not(target_arch = "wasm32"))]
include!("ffi/replay.rs");
include!("ffi/worker.rs");
include!("ffi/verification.rs");
include!("ffi/tests.rs");
