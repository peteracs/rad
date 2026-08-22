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
mod transaction;
pub(crate) use transaction::{PendingTransactionWrite, PostCommitContext, TransactionContext};
#[cfg(not(target_arch = "wasm32"))]
mod io_pool;
mod nested_native_replay;
mod parallel;
mod program_manifest;
pub use program_manifest::CompiledProgramManifest;
pub(crate) use program_manifest::{
    BYTECODE_SEMANTIC_VERSION, COMPILER_SEMANTIC_VERSION, PROGRAM_MANIFEST_VERSION,
};

#[cfg(test)]
mod builtins_tests;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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

const EVENT_LOG_CAP: usize = 4096;

/// A capture cell is a GC-managed mutable slot.
pub(crate) type CaptureCell = *mut crate::gc::CaptureCell;

/// Declared field types per component/resource type name, shared across VM
/// clones (checker-derived; empty on checker-less compiles).
pub type ComponentFieldTypes = Arc<HashMap<String, Arc<Vec<(String, crate::types::Ty)>>>>;

#[derive(Clone)]
pub(crate) struct VmSharedState {
    pub(crate) chunks: Arc<Vec<SealedChunk>>,
    pub(crate) view_kernels: Arc<Vec<Arc<crate::view_kernel::ViewKernelPlan>>>,
    pub(crate) globals: Vec<Value>,
    pub(crate) global_names: Arc<Vec<String>>,
    pub(crate) program_source_identity: Option<Arc<str>>,
    pub(crate) relation_runtime_manifest:
        Option<Arc<crate::relation::runtime::RelationRuntimeManifest>>,
    pub(crate) state_machines: Arc<HashMap<String, HashMap<String, Vec<StateTransitionInfo>>>>,
    pub(crate) event_handlers: Arc<HashMap<String, Vec<HandlerEntry>>>,
    pub(crate) systems: Arc<HashMap<String, SystemRuntimeInfo>>,
    pub(crate) intent_registry: Arc<HashMap<String, IntentRuntimeInfo>>,
    pub(crate) resolver_registry: Arc<HashMap<String, ResolverRuntimeInfo>>,
    pub(crate) constraint_registry: Arc<Vec<ConstraintRuntimeInfo>>,
    pub(crate) native_extension_manifests: Arc<Vec<Arc<crate::ffi::NativeExtensionManifest>>>,
    pub(crate) component_layouts: Arc<HashMap<String, Arc<Vec<String>>>>,
    pub(crate) native_layouts: Arc<HashMap<String, crate::native_types::NativeLayout>>,
    pub(crate) component_field_types: ComponentFieldTypes,
    /// Declared schema versions (`component X v2`), nonzero entries only
    /// (dogfood feature seq 69 IDEA 03).
    pub(crate) component_versions: Arc<HashMap<String, u32>>,
    pub(crate) variant_layouts: Arc<HashMap<(String, String), Vec<String>>>,
    pub(crate) transient_resources: Arc<HashSet<String>>,
    pub(crate) rng_state: u64,
    pub(crate) suppress_output: bool,
    pub(crate) profile_copies: bool,
    pub(crate) collect_system_metrics: bool,
    pub(crate) causal_value_limits: crate::CausalValueLimits,
    pub(crate) constraint_limit_profile: crate::constraint_types::ConstraintLimitProfile,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemExecutionMetrics {
    pub invocations: u64,
    pub total_instructions: u64,
    pub max_instructions: u64,
    pub total_guest_allocations: u64,
    pub max_guest_allocations: u64,
    pub total_guest_allocated_bytes: u64,
    pub max_guest_allocated_bytes: u64,
    /// Exact process-allocator calls while executing VM code outside direct
    /// GC backing allocations and native host boundaries.
    pub total_runtime_allocations: u64,
    pub max_runtime_allocations: u64,
    pub total_runtime_allocated_bytes: u64,
    pub max_runtime_allocated_bytes: u64,
    /// Direct process allocations for GC tracking-header + payload boxes.
    pub total_managed_backing_allocations: u64,
    pub max_managed_backing_allocations: u64,
    pub total_managed_backing_bytes: u64,
    pub max_managed_backing_bytes: u64,
    /// Allocations performed through RAD's process allocator while inside a
    /// native-call boundary. A plugin's private allocator is not included.
    pub total_host_boundary_allocations: u64,
    pub max_host_boundary_allocations: u64,
    pub total_host_boundary_allocated_bytes: u64,
    pub max_host_boundary_allocated_bytes: u64,
    /// False means the embedding executable did not install the metered
    /// allocator; native counts must then be reported as unavailable.
    pub native_allocation_meter_supported: bool,
    pub instruction_budget: Option<u64>,
}

impl SystemExecutionMetrics {
    fn record(
        &mut self,
        instructions: u64,
        guest_allocations: u64,
        guest_allocated_bytes: u64,
        native: crate::allocation_meter::AllocationMeasurement,
        instruction_budget: Option<u64>,
    ) {
        let first_invocation = self.invocations == 0;
        self.invocations = self.invocations.saturating_add(1);
        self.total_instructions = self.total_instructions.saturating_add(instructions);
        self.max_instructions = self.max_instructions.max(instructions);
        self.total_guest_allocations = self
            .total_guest_allocations
            .saturating_add(guest_allocations);
        self.max_guest_allocations = self.max_guest_allocations.max(guest_allocations);
        self.total_guest_allocated_bytes = self
            .total_guest_allocated_bytes
            .saturating_add(guest_allocated_bytes);
        self.max_guest_allocated_bytes = self.max_guest_allocated_bytes.max(guest_allocated_bytes);
        let native_supported = native.supported;
        let native = native.counters;
        self.total_runtime_allocations = self
            .total_runtime_allocations
            .saturating_add(native.runtime_calls);
        self.max_runtime_allocations = self.max_runtime_allocations.max(native.runtime_calls);
        self.total_runtime_allocated_bytes = self
            .total_runtime_allocated_bytes
            .saturating_add(native.runtime_bytes);
        self.max_runtime_allocated_bytes =
            self.max_runtime_allocated_bytes.max(native.runtime_bytes);
        self.total_managed_backing_allocations = self
            .total_managed_backing_allocations
            .saturating_add(native.managed_calls);
        self.max_managed_backing_allocations = self
            .max_managed_backing_allocations
            .max(native.managed_calls);
        self.total_managed_backing_bytes = self
            .total_managed_backing_bytes
            .saturating_add(native.managed_bytes);
        self.max_managed_backing_bytes = self.max_managed_backing_bytes.max(native.managed_bytes);
        self.total_host_boundary_allocations = self
            .total_host_boundary_allocations
            .saturating_add(native.host_boundary_calls);
        self.max_host_boundary_allocations = self
            .max_host_boundary_allocations
            .max(native.host_boundary_calls);
        self.total_host_boundary_allocated_bytes = self
            .total_host_boundary_allocated_bytes
            .saturating_add(native.host_boundary_bytes);
        self.max_host_boundary_allocated_bytes = self
            .max_host_boundary_allocated_bytes
            .max(native.host_boundary_bytes);
        if first_invocation {
            self.native_allocation_meter_supported = native_supported;
        } else {
            self.native_allocation_meter_supported &= native_supported;
        }
        self.instruction_budget = instruction_budget;
    }

    fn merge(&mut self, other: &Self) {
        let first_invocation = self.invocations == 0;
        self.invocations = self.invocations.saturating_add(other.invocations);
        self.total_instructions = self
            .total_instructions
            .saturating_add(other.total_instructions);
        self.max_instructions = self.max_instructions.max(other.max_instructions);
        self.total_guest_allocations = self
            .total_guest_allocations
            .saturating_add(other.total_guest_allocations);
        self.max_guest_allocations = self.max_guest_allocations.max(other.max_guest_allocations);
        self.total_guest_allocated_bytes = self
            .total_guest_allocated_bytes
            .saturating_add(other.total_guest_allocated_bytes);
        self.max_guest_allocated_bytes = self
            .max_guest_allocated_bytes
            .max(other.max_guest_allocated_bytes);
        self.total_runtime_allocations = self
            .total_runtime_allocations
            .saturating_add(other.total_runtime_allocations);
        self.max_runtime_allocations = self
            .max_runtime_allocations
            .max(other.max_runtime_allocations);
        self.total_runtime_allocated_bytes = self
            .total_runtime_allocated_bytes
            .saturating_add(other.total_runtime_allocated_bytes);
        self.max_runtime_allocated_bytes = self
            .max_runtime_allocated_bytes
            .max(other.max_runtime_allocated_bytes);
        self.total_managed_backing_allocations = self
            .total_managed_backing_allocations
            .saturating_add(other.total_managed_backing_allocations);
        self.max_managed_backing_allocations = self
            .max_managed_backing_allocations
            .max(other.max_managed_backing_allocations);
        self.total_managed_backing_bytes = self
            .total_managed_backing_bytes
            .saturating_add(other.total_managed_backing_bytes);
        self.max_managed_backing_bytes = self
            .max_managed_backing_bytes
            .max(other.max_managed_backing_bytes);
        self.total_host_boundary_allocations = self
            .total_host_boundary_allocations
            .saturating_add(other.total_host_boundary_allocations);
        self.max_host_boundary_allocations = self
            .max_host_boundary_allocations
            .max(other.max_host_boundary_allocations);
        self.total_host_boundary_allocated_bytes = self
            .total_host_boundary_allocated_bytes
            .saturating_add(other.total_host_boundary_allocated_bytes);
        self.max_host_boundary_allocated_bytes = self
            .max_host_boundary_allocated_bytes
            .max(other.max_host_boundary_allocated_bytes);
        if first_invocation {
            self.native_allocation_meter_supported = other.native_allocation_meter_supported;
        } else {
            self.native_allocation_meter_supported &= other.native_allocation_meter_supported;
        }
        self.instruction_budget = self.instruction_budget.or(other.instruction_budget);
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
pub struct ModelTraceStep {
    pub command: usize,
    pub label: String,
    pub flush_before: bool,
    pub flush_after: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ModelCheckConfig {
    pub model: Option<String>,
    pub runs: Option<u32>,
    pub max_commands: Option<u32>,
    pub seed: Option<u64>,
    /// Exact generated trace supplied by replay/shrink tooling. When set,
    /// random generation is bypassed completely.
    pub trace: Option<Vec<ModelTraceStep>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ModelCheckReport {
    pub model: String,
    pub seed: u64,
    pub histories_requested: u32,
    pub histories_executed: u32,
    pub commands_executed: u64,
    pub counterexamples: u32,
    pub minimum_trace_length: Option<usize>,
    pub shrink_ns: u64,
    pub median_history_ns: u64,
    pub p95_history_ns: u64,
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
    pub(crate) system_metrics: BTreeMap<String, SystemExecutionMetrics>,
}
// VM state, program state, and lifecycle share one private implementation namespace.
include!("state.rs");
include!("program_state.rs");
include!("causal_recording.rs");
include!("lifecycle.rs");
