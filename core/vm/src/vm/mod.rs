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
    pub coverage: Vec<ModelCommandCoverage>,
}

/// What one model command did across a campaign. `wrote` and `reached_property`
/// count executions, so each is at most `executed`. Writes are attempted writes:
/// a command that stores the value already present still counts.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ModelCommandCoverage {
    pub command: String,
    pub executed: u64,
    pub wrote: u64,
    pub reached_property: u64,
}

impl ModelCheckReport {
    /// A pass in which no command touched anything a property reads. Such a
    /// campaign only re-checked the initial state, so it is not a pass.
    pub fn is_vacuous(&self) -> bool {
        self.counterexamples == 0
            && self
                .coverage
                .iter()
                .all(|command| command.reached_property == 0)
    }

    /// Reasons a passing campaign may have tested less than it appears to.
    /// A failed campaign already has its answer, so it has no warnings.
    pub fn coverage_warnings(&self) -> Vec<String> {
        if self.counterexamples != 0 {
            return Vec::new();
        }
        let mut warnings = Vec::new();
        for command in &self.coverage {
            if command.executed == 0 {
                warnings.push(format!("command '{}' never ran", command.command));
            } else if command.wrote == 0 {
                warnings.push(format!(
                    "command '{}' ran {} time(s) and never wrote state",
                    command.command, command.executed
                ));
            }
        }
        if self.is_vacuous() {
            warnings.push(
                "no command wrote anything an invariant or temporal clause reads, so this pass proves nothing"
                    .to_string(),
            );
        }
        warnings
    }
}

#[cfg(test)]
mod model_coverage_tests {
    use super::{ModelCheckReport, ModelCommandCoverage};

    fn report(counterexamples: u32, coverage: &[(&str, u64, u64, u64)]) -> ModelCheckReport {
        ModelCheckReport {
            model: "M".to_string(),
            seed: 1,
            histories_requested: 1,
            histories_executed: 1,
            commands_executed: 1,
            counterexamples,
            minimum_trace_length: None,
            shrink_ns: 0,
            median_history_ns: 0,
            p95_history_ns: 0,
            coverage: coverage
                .iter()
                .map(
                    |&(command, executed, wrote, reached_property)| ModelCommandCoverage {
                        command: command.to_string(),
                        executed,
                        wrote,
                        reached_property,
                    },
                )
                .collect(),
        }
    }

    /// Run a program's models and return the failure of model `M`, if any.
    fn campaign_failure(source: &str) -> Option<String> {
        let mut vm = crate::replay_compile::compile_trace_vm(
            source,
            "model fixture",
            &[],
            &crate::source_bundle::SourceLayout::default(),
        )
        .expect("model fixture compiles");
        vm.run(0).expect("model fixture initializes");
        crate::test_runner::run_tests(&mut vm)
            .into_iter()
            .find(|outcome| outcome.name == "model_M")
            .expect("model M ran")
            .error
    }

    /// Each of these violates its invariant on the first or a later step, and
    /// passes if the write or read is missing from the dependency trace.
    fn assert_campaign_fails(setup: &str, command: &str, invariant: &str) {
        let source = format!(
            "{setup}\nfn step() -> nil {{ {command} }}\nmodel M {{\n commands [step]\n invariant {{ {invariant} }}\n runs 3\n max_commands 8\n seed 1\n}}\n"
        );
        let failure = campaign_failure(&source);
        assert!(
            failure.is_some(),
            "invariant was not re-evaluated:\n{source}"
        );
    }

    #[test]
    fn spawn_reaches_a_query_invariant() {
        assert_campaign_fails(
            "component F { on: bool = false }",
            "let _e = spawn(F { on: true })",
            "return len(query { F }) <= 3",
        );
    }

    #[test]
    fn set_reaches_a_query_invariant() {
        assert_campaign_fails(
            "component F { n: int = 0 }\nlet e = spawn(F { n: 0 })",
            "set(e, F { n: 1 })\n let _x = spawn(F { n: 2 })",
            "let mut total = 0\n for row in query { F } { total = total + 1 }\n return total <= 2",
        );
    }

    #[test]
    fn a_named_spawn_reaches_a_name_lookup_invariant() {
        assert_campaign_fails(
            "component F { on: bool = false }",
            "if get_entity(\"x\") == nil { let _e = spawn(\"x\", F { on: true }) }",
            "return get_entity(\"x\") == nil",
        );
    }

    #[test]
    fn commit_reaches_an_invariant() {
        assert_campaign_fails(
            "resource R { n: int = 0 }\nset_resource(R, R { n: 0 })",
            "commit(fork_with(fork(), R { n: 5 }))",
            "return (get_resource(R) |> unwrap).n == 0",
        );
    }

    #[test]
    fn load_world_reaches_an_invariant() {
        assert_campaign_fails(
            "resource R { n: int = 0 }\nset_resource(R, R { n: 5 })\nlet saved = save_world()\nset_resource(R, R { n: 0 })",
            "load_world(saved)",
            "return (get_resource(R) |> unwrap).n == 0",
        );
    }

    #[test]
    fn a_pass_no_command_reached_is_vacuous() {
        let report = report(0, &[("add", 5, 0, 0), ("other", 5, 5, 0)]);
        assert!(report.is_vacuous());
        let warnings = report.coverage_warnings();
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("'add'"));
        assert!(warnings[1].contains("proves nothing"));
    }

    #[test]
    fn a_dead_command_warns_without_making_the_pass_vacuous() {
        let report = report(
            0,
            &[("add", 5, 5, 5), ("dead", 5, 0, 0), ("unrun", 0, 0, 0)],
        );
        assert!(!report.is_vacuous());
        assert_eq!(report.coverage_warnings().len(), 2);
    }

    #[test]
    fn a_counterexample_is_never_vacuous() {
        let report = report(1, &[("add", 5, 0, 0)]);
        assert!(!report.is_vacuous());
        assert!(report.coverage_warnings().is_empty());
    }
}

/// Dynamic dependency evidence for one stateful-model operation.
///
/// The ordinary sandbox checks already form the single authoritative path for
/// component/resource access. Model checking temporarily records those names
/// there so an invariant whose dependencies were untouched need not be
/// interpreted again. `*` is the fail-closed identity for bulk or otherwise
/// unbounded access.
#[derive(Clone, Copy, Default)]
pub(crate) struct ModelAccessSet {
    identities: [u64; 32],
    len: u8,
    wildcard: bool,
}

impl ModelAccessSet {
    #[inline]
    pub(crate) fn clear(&mut self) {
        // `len` makes old hashes unreachable. Clearing the full fixed array
        // here turned every model command into hundreds of bytes of memset;
        // resetting the logical extent is exact and constant-time.
        self.len = 0;
        self.wildcard = false;
    }

    pub(crate) fn insert(&mut self, identity: &str) {
        use std::hash::{Hash, Hasher};
        if identity == "*" {
            self.wildcard = true;
            return;
        }
        let mut hasher = rustc_hash::FxHasher::default();
        identity.hash(&mut hasher);
        let hash = hasher.finish();
        let occupied = usize::from(self.len);
        if self.identities[..occupied].contains(&hash) {
            return;
        }
        if occupied == self.identities.len() {
            // Overflow broadens the dependency to all state. It can cost an
            // optimization, but can never skip a required invariant check.
            self.wildcard = true;
            return;
        }
        self.identities[occupied] = hash;
        self.len += 1;
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        if other.wildcard {
            self.wildcard = true;
        }
        for hash in &other.identities[..usize::from(other.len)] {
            let occupied = usize::from(self.len);
            if self.identities[..occupied].contains(hash) {
                continue;
            }
            if occupied == self.identities.len() {
                self.wildcard = true;
                return;
            }
            self.identities[occupied] = *hash;
            self.len += 1;
        }
    }

    pub(crate) fn intersects(&self, other: &Self) -> bool {
        self.wildcard
            || other.wildcard
            || self.identities[..usize::from(self.len)]
                .iter()
                .any(|hash| other.identities[..usize::from(other.len)].contains(hash))
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.wildcard && self.len == 0
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ModelAccessTrace {
    pub(crate) reads: ModelAccessSet,
    pub(crate) writes: ModelAccessSet,
}

impl ModelAccessTrace {
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.reads.clear();
        self.writes.clear();
    }
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
include!("binary_publications.rs");
include!("program_state.rs");
include!("causal_recording.rs");
include!("lifecycle.rs");
