pub(crate) mod attempt_replay;
mod builtins_impl;
pub(crate) use builtins_impl::value_to_json;
mod constraint_runtime;
mod exec;
mod helpers;
mod range_plan;
pub(crate) mod replay_clone;
mod settlement;
pub(crate) use settlement::{
    ConstraintRuntimeInfo, IntentRuntimeInfo, ResolverRuntimeInfo, SettlementContext,
};
#[cfg(not(target_arch = "wasm32"))]
mod io_pool;
mod parallel;
mod program_manifest;
pub use program_manifest::CompiledProgramManifest;
pub(crate) use program_manifest::{
    BYTECODE_SEMANTIC_VERSION, COMPILER_SEMANTIC_VERSION, PROGRAM_MANIFEST_VERSION,
};

#[cfg(test)]
mod builtins_tests;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
use std::net::{TcpListener, TcpStream, UdpSocket};

use crate::arena::BumpArena;
use crate::compiler::{CompileResult, StateTransitionInfo};
use crate::gc::GcHeap;
use crate::opcode::{Chunk, SealedChunk};
use crate::value::{Builtin, Value};
use crate::world::{World, WorldSnapshot};
#[cfg(not(target_arch = "wasm32"))]
use io_pool::IoPool;

#[derive(Clone, Debug)]
pub struct EventLogEntry {
    pub tick: u64,
    pub event_name: String,
    pub(crate) payload: Value,
}

/// A capture cell is a GC-managed mutable slot.
pub(crate) type CaptureCell = *mut crate::gc::CaptureCell;

/// Declared field types per component/resource type name, shared across VM
/// clones (checker-derived; empty on checker-less compiles).
pub type ComponentFieldTypes = Arc<HashMap<String, Arc<Vec<(String, crate::types::Ty)>>>>;

#[derive(Clone)]
pub(crate) struct VmSharedState {
    pub(crate) chunks: Arc<Vec<SealedChunk>>,
    pub(crate) globals: Vec<Value>,
    pub(crate) global_names: Arc<Vec<String>>,
    pub(crate) program_source_identity: Option<Arc<str>>,
    pub(crate) relation_runtime_manifest:
        Option<Arc<crate::relation_runtime::RelationRuntimeManifest>>,
    pub(crate) state_machines: Arc<HashMap<String, HashMap<String, Vec<StateTransitionInfo>>>>,
    pub(crate) event_handlers: Arc<HashMap<String, Vec<HandlerEntry>>>,
    pub(crate) systems: Arc<HashMap<String, SystemRuntimeInfo>>,
    pub(crate) intent_registry: Arc<HashMap<String, IntentRuntimeInfo>>,
    pub(crate) resolver_registry: Arc<HashMap<String, ResolverRuntimeInfo>>,
    pub(crate) constraint_registry: Arc<Vec<ConstraintRuntimeInfo>>,
    pub(crate) native_extension_manifests: Arc<Vec<Arc<crate::ffi::NativeExtensionManifest>>>,
    pub(crate) component_layouts: Arc<HashMap<String, Arc<Vec<String>>>>,
    pub(crate) component_field_types: ComponentFieldTypes,
    /// Declared schema versions (`component X v2`), nonzero entries only
    /// (dogfood feature seq 69 IDEA 03).
    pub(crate) component_versions: Arc<HashMap<String, u32>>,
    pub(crate) variant_layouts: Arc<HashMap<(String, String), Vec<String>>>,
    pub(crate) transient_resources: Arc<HashSet<String>>,
    pub(crate) rng_state: u64,
    pub(crate) suppress_output: bool,
    pub(crate) profile_copies: bool,
    pub(crate) causal_value_limits: crate::CausalValueLimits,
    pub(crate) constraint_limit_profile: crate::constraint_types::ConstraintLimitProfile,
}

pub struct CallFrame {
    pub(crate) frame_id: u64,
    pub(crate) chunk_id: usize,
    pub(crate) ip: usize,
    pub(crate) stack_base: usize,
    /// Shared across repeated filter iterations so `exec_query_filter` does not clone the vec per entity.
    pub(crate) captures: Option<Arc<Vec<CaptureCell>>>,
    pub(crate) system_writeback: Option<SystemWriteback>,
}

pub(crate) const MAX_CALL_DEPTH: usize = 512;
pub(crate) type SystemSignature = Vec<(String, bool, String)>;

/// Parallel ECS worker output: commands and deferred events.
///
/// # Safety (`Send`)
///
/// This type is `Send` because payloads are copied into persistent/main-thread
/// storage before they are applied to global state.
pub struct WorkerResult {
    pub cmds: Vec<EcsCommand>,
    pub(crate) evts: Vec<(String, Value, u64)>,
}
// Lexical sections preserve one private semantic namespace.
include!("mod/model.rs");
include!("mod/program_and_state.rs");
include!("mod/loading_and_execution.rs");
