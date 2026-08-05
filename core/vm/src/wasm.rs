#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::ast::Decl;
use crate::checker::{Checker, CheckerOptions};
use crate::compiler::{Compiler, StateTransitionInfo};
use crate::gc::GcHeap;
use crate::lexer::Lexer;
use crate::opcode::{Chunk, Op};
use crate::parser::Parser;
use crate::value::Value;
use crate::vm::VM;

mod presentation;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct RadRuntime {
    vm: VM,
    output: Vec<String>,
    /// Exact word-oriented presentation packet. Integer identities stay
    /// lossless; floating fields are stored as IEEE-754 `f32::to_bits()`.
    render_buffer: Vec<u32>,
    /// Reused canonical entity selection for presentation encoding.
    render_entity_scratch: Vec<u32>,
    /// Runtime-local lineage for presentation packets. A successful session
    /// start creates a new stream; GPU hosts must never apply deltas across it.
    presentation_stream_id: u64,
    /// Sequence assigned to the next packet in the active stream. `None`
    /// means the active stream exhausted its sequence space.
    presentation_next_sequence: Option<u64>,
    presentation_stream_active: bool,
    /// Streaming-session state (D4). `session_base` is the snapshot the next
    /// `session_delta()` diffs against — held as a bare `Arc`, NOT a gc
    /// `Value`: the collector cannot see RadRuntime fields as roots, and a
    /// swept fork here would be a use-after-free with extra steps.
    session_base: Option<std::sync::Arc<crate::world::WorldSnapshot>>,
    /// How much of `vm.print_buffer` earlier pumps already returned.
    session_cursor: usize,
    /// RADGUI undo ring: one CoW fork per user interaction, capped. Undo is
    /// `commit(pop())` — no app participates, the world itself is the
    /// undo record. Bare Arcs for the same GC-root reason as session_base.
    undo_stack: Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
    /// Worlds undone FROM — Ctrl+Shift+Z walks back up. Any new
    /// checkpoint (a fresh user action) invalidates the redo branch,
    /// standard undo-tree-pruned-to-a-line semantics.
    redo_stack: Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
    /// What the renderer last saw — `session_render_delta()` diffs against
    /// this, so unchanged widgets cost zero serialization.
    render_base: Option<std::sync::Arc<crate::world::WorldSnapshot>>,
}
// Lexical sections preserve one private semantic namespace.
include!("wasm/runtime_api.rs");
include!("wasm/execution.rs");
include!("wasm/rendering_and_tests.rs");
