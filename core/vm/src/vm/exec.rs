#![allow(clippy::arc_with_non_send_sync)]

use super::*;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::opcode::{Chunk, Op};
use crate::value::{
    set_profile_copy_context, ClosureValue, ComponentData, MapKey, MapStorage, Value,
};

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

thread_local! {
    static WORKER_VM: std::cell::RefCell<Option<crate::vm::VM>> = const { std::cell::RefCell::new(None) };
}

/// Run `f` against this thread's pooled worker VM, creating it on first use.
/// Used by parallel system batches and `simulate_par` fork exploration.
pub(crate) fn with_worker_vm<R>(
    shared: &crate::vm::VmSharedState,
    f: impl FnOnce(&mut crate::vm::VM) -> R,
) -> R {
    WORKER_VM.with(|cell| {
        let mut worker = cell.borrow_mut();
        if worker.is_none() {
            *worker = Some(crate::vm::VM::from_shared_state(shared.clone()));
        }
        let worker = worker.as_mut().expect("worker VM was initialized");
        worker.sync_from_shared(shared);
        f(worker)
    })
}

// Lexical sections preserve one private semantic namespace.
include!("exec/frame_loop.rs");
include!("exec/core_opcodes.rs");
include!("exec/collection_and_ecs_opcodes.rs");
include!("exec/state_scheduling_and_stack.rs");
include!("exec/calls_buffers_and_vectors.rs");
include!("exec/helpers_and_tests.rs");
