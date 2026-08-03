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

unsafe impl Send for WorkerResult {}

#[derive(Clone, Debug)]
pub enum EcsCommand {
    SetComponent(u32, crate::value::ComponentData),
    SetResource(String, crate::value::ComponentData),
    SpawnEntity(Option<String>, Vec<crate::value::ComponentData>, u32), // The u32 is the local ID assigned by the worker
    RemoveComponent(u32, String),
    DespawnEntity(u32),
}

impl EcsCommand {
    /// Buffered commands own persisted payloads (they must survive worker
    /// GC until end-of-frame apply). Call this when a command is discarded
    /// without being applied, or its values leak from the persistent store.
    pub(crate) fn release_payload(&self) {
        match self {
            EcsCommand::SetComponent(_, data) | EcsCommand::SetResource(_, data) => {
                Value::release_component_data(data);
            }
            EcsCommand::SpawnEntity(_, comps, _) => {
                for c in comps {
                    Value::release_component_data(c);
                }
            }
            EcsCommand::RemoveComponent(..) | EcsCommand::DespawnEntity(..) => {}
        }
    }
}

#[derive(Clone, Debug)]
pub enum TaskStatus {
    Ready,
    Completed(Value),
    Failed(String),
}

#[derive(Clone, Debug)]
pub enum IoTaskPayload {
    String(String),
    Nil,
    Int(i64),
    StringList(Vec<String>),
    Bytes(Vec<u8>),
    ValueMap(Vec<(String, IoTaskPayload)>),
}

#[derive(Clone, Debug)]
pub struct TaskRecord {
    pub id: u64,
    pub status: TaskStatus,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum NetHandle {
    TcpStream(TcpStream),
    TcpListener(TcpListener),
    UdpSocket(UdpSocket),
}

pub struct VM {
    pub(crate) chunks: Arc<Vec<SealedChunk>>,
    pub(crate) stack: Vec<Value>,
    pub(crate) globals: Vec<Value>,
    pub(crate) global_names: Arc<Vec<String>>,
    /// Authenticated identity of the source units and resolved import graph
    /// that produced the installed program. Kept immutable with the shared
    /// executable tables and included in the compiled-program manifest.
    pub(crate) program_source_identity: Option<Arc<str>>,
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) next_frame_id: u64,
    pub(crate) world: World,
    pub(crate) state_machines: Arc<HashMap<String, HashMap<String, Vec<StateTransitionInfo>>>>,
    pub(crate) event_handlers: Arc<HashMap<String, Vec<HandlerEntry>>>,
    pub(crate) systems: Arc<HashMap<String, SystemRuntimeInfo>>,
    pub(crate) intent_registry: Arc<HashMap<String, IntentRuntimeInfo>>,
    pub(crate) resolver_registry: Arc<HashMap<String, ResolverRuntimeInfo>>,
    pub(crate) constraint_registry: Arc<Vec<ConstraintRuntimeInfo>>,
    pub(crate) native_extension_manifests: Arc<Vec<Arc<crate::ffi::NativeExtensionManifest>>>,
    pub(crate) settlement: Option<SettlementContext>,
    pub(crate) next_settlement_id: u64,
    pub(crate) causal_value_limits: crate::CausalValueLimits,
    pub(crate) constraint_limit_profile: crate::constraint_types::ConstraintLimitProfile,
    /// Schema migrations (#5): component/resource name → compiled
    /// `migrate X(old)` chunk, invoked by `load_world` on shape drift.
    pub(crate) migrations: HashMap<String, MigrationEntry>,
    pub(crate) events_current: Vec<(String, Value, u64)>,
    pub(crate) events_next: Vec<(String, Value, u64)>,
    pub(crate) events_processing: Vec<(String, Value, u64)>,
    /// `emit E { .. } after N`: (ticks_left, name, payload, emit_id). Aged
    /// at every event flush; entries reaching zero join events_next that
    /// cycle. The emit id is created when the timer is scheduled, so a
    /// delayed handler can still explain who armed it.
    pub(crate) delayed_events: Vec<(i64, String, Value, u64)>,
    pub print_buffer: Vec<String>,
    pub eprint_buffer: Vec<String>,
    pub(crate) suppress_output: bool,
    pub(crate) profile_copies: bool,
    /// `RAD_OP_PROFILE=1`: per-opcode execution histogram, dumped to stderr
    /// when `run()` finishes. The cheapest possible "where does the
    /// interpreter spend its dispatches" answer.
    pub(crate) op_profile: bool,
    pub(crate) op_counts: Vec<u64>,
    /// Live timeline tracing (RADSCOPE): capture a CoW world snapshot at
    /// every main-timeline frame boundary into `timeline`, capped so a
    /// runaway loop can't eat the heap. Embedders flip this before `run`.
    pub trace_timeline: bool,
    /// Retroactive edit (RADSCOPE "patch & replay"): during a traced run,
    /// when the causality clock reaches `frame`, set `entity.component.field`
    /// to `value` before that frame's handlers fire — then the rest of the
    /// run recomputes from the edited past. (frame, entity, component,
    /// field, value as JSON scalar.)
    pub trace_patch: Option<(u64, String, String, String, String)>,
    pub(crate) component_layouts: Arc<HashMap<String, Arc<Vec<String>>>>,
    /// Declared field types per component/resource (checker-derived; empty
    /// on checker-less compiles). The deserialization boundary validates
    /// loaded/migrated rows against these so persisted type drift is a loud
    /// error instead of silent corruption of statically-typed fields.
    pub(crate) component_field_types: ComponentFieldTypes,
    /// Declared schema versions (`component X v2` / `resource Y v3`),
    /// nonzero entries only. `save_world()` embeds them per type;
    /// `load_world` hands the save's value to `migrate X(old, from_version)`
    /// (dogfood feature seq 69 IDEA 03).
    pub(crate) component_versions: Arc<HashMap<String, u32>>,
    pub(crate) variant_layouts: Arc<HashMap<(String, String), Vec<String>>>,
    /// `transient resource` names — schema-level (like `indexed_decl`),
    /// excluded from world_digest()/save_world(): command tapes, derived
    /// caches, spatial indexes. Forks/commits still carry their values.
    pub(crate) transient_resources: Arc<HashSet<String>>,
    /// The program's `indexed` field declarations — the source of truth for
    /// world indices. Snapshots only carry derived state; `commit()`
    /// reconciles the restored world against this (a foreign snapshot from a
    /// wire decode or an old save must not wipe the program's indexes).
    pub(crate) indexed_decl: Arc<HashMap<String, HashSet<String>>>,
    pub(crate) gc: GcHeap,
    pub(crate) arena: BumpArena,
    pub(crate) timeline: Vec<WorldSnapshot>,
    pub(crate) event_log: Vec<EventLogEntry>,
    pub(crate) rng_state: u64,
    pub(crate) tasks: HashMap<u64, TaskRecord>,
    pub(crate) next_task_id: u64,
    pub(crate) pending_io: HashMap<u64, Receiver<Result<IoTaskPayload, String>>>,
    pub(crate) in_async_context: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) io_pool: IoPool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) loaded_libraries: Vec<libloading::Library>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) net_handles: HashMap<u64, NetHandle>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) next_net_handle_id: u64,
    pub(crate) current_trace_id: Option<u64>,
    pub(crate) next_trace_id: u64,
    pub in_simulation_fork: u32,
    /// Non-zero while a builtin holds heap values in Rust locals across a
    /// nested rad execution (simulate's saved timeline, a decode path running
    /// `migrate` blocks). Auto-GC cannot see those locals as roots, so it
    /// must not run until the window closes.
    pub(crate) gc_pause: u32,
    pub(crate) is_worker: bool,
    /// Program arguments (everything after `--` on the CLI). `sys_args()`
    /// returns exactly these — never the host process argv.
    pub sys_args: Vec<String>,
    pub(crate) command_buffer: Vec<EcsCommand>,
    pub(crate) once_guard_passed: bool,
    /// Remaining instruction budget. `u64::MAX` means unmetered (trusted code).
    /// Charged on loop back-edges and calls, so straight-line code pays nothing.
    pub fuel: u64,
    /// GC allocation ceiling in bytes. `usize::MAX` means uncapped.
    pub mem_limit: usize,
    /// Present only while one RFC-0002 constraint invocation is running.
    /// Unlike the ordinary sandbox counters this meter charges every opcode.
    pub(crate) constraint_meter: Option<constraint_runtime::ConstraintExecutionMeter>,
    /// Capability set for sandboxed execution. `None` for trusted code.
    pub(crate) sandbox_caps: Option<std::sync::Arc<crate::sandbox::SandboxCaps>>,
    /// Data-only input for sandboxed guests, as JSON. Read via `sandbox_input()`.
    pub(crate) sandbox_input_json: Option<String>,
    /// Data-only output from sandboxed guests, as JSON. Set via `sandbox_output(v)`.
    pub(crate) sandbox_output_json: Option<String>,
    /// Host-side telemetry from the most recent `sandbox_run` call on THIS vm:
    /// the guest's last `sandbox_output(v)` as JSON (read via
    /// `sandbox_last_output()`) and the fuel it consumed (via
    /// `sandbox_last_fuel()`). `run_sandbox_guest` computes both and they were
    /// dropped on the in-language path before this existed (dogfood feature
    /// seq 62). `None`/`0` until the first `sandbox_run`.
    pub(crate) last_sandbox_output_json: Option<String>,
    pub(crate) last_sandbox_fuel_spent: u64,
    /// When true, `schedule`/phases run every system serially in topological
    /// order instead of partitioning into parallel batches — the
    /// `rad run --serial-schedule` lever (dogfood feature seq 83). A
    /// correctness-critical/latency-insensitive escape hatch and a one-flag
    /// differential test against the parallel scheduler. Explicit
    /// `simulate_par`/`simulate_many` are unaffected — this steers only the
    /// implicit schedule parallelism.
    pub(crate) serial_schedule: bool,
    /// Record & replay: when set, every replay-managed builtin result is
    /// logged (see `replay.rs`). `None` for normal execution.
    pub(crate) recorder: Option<crate::replay::TraceRecorder>,
    /// Replay mode: when set, replay-managed builtins consume the trace
    /// instead of executing — a replay never re-fires io.
    pub(crate) replayer: Option<crate::replay::TraceReplayer>,
    /// True only in the discarded child used for failed-attempt replay.
    /// Native/FFI and irreversible host effects are disabled there even
    /// before the replayed request reaches its settlement.
    pub(crate) observational_attempt_replay: bool,
    /// Causality (#4): provenance ledger of main-timeline writes and emits.
    pub(crate) ledger: crate::causality::CausalityLedger,
    /// Who is currently executing — top-level code, a system, or a handler.
    /// Writes recorded in the ledger carry this as their cause.
    pub(crate) current_cause: crate::causality::Cause,
    /// Main-timeline frame counter, advanced in lockstep with the
    /// record/replay frame convention (k-th flush starts frame k).
    pub(crate) causality_frame: u64,
    /// Per-instance emit-record ids, aligned index-for-index with the event
    /// buffers (`events_*`). 0 = no record (worker-emitted, linked at merge).
    pub(crate) emit_ids_current: Vec<u64>,
    pub(crate) emit_ids_next: Vec<u64>,
}

#[derive(Clone)]
pub struct HandlerEntry {
    pub(crate) chunk_id: usize,
    pub(crate) param_slot: u16,
    pub(crate) once: bool,
    pub(crate) fired: bool,
    pub(crate) has_guard: bool,
}

#[derive(Clone, Copy)]
pub struct MigrationEntry {
    pub(crate) chunk_id: usize,
    pub(crate) param_slot: u16,
    /// Slot of the optional `from_version` param (dogfood seq 69).
    pub(crate) version_slot: Option<u16>,
}

#[derive(Clone)]
pub struct SystemWriteback {
    pub(crate) entity_id: u32,
    pub(crate) mutable_params: Vec<(u16, String)>,
    pub(crate) mutable_resources: Vec<(u16, String)>,
}

#[derive(Clone)]
pub struct SystemRuntimeInfo {
    pub(crate) params: SystemSignature,
    pub(crate) resource_params: SystemSignature,
    pub(crate) chunk_id: usize,
    pub(crate) after: Vec<String>,
    pub(crate) before: Vec<String>,
    /// Systems sharing a `serial phase` group id never share a parallel
    /// batch — they run in separate batches, in schedule order (dogfood
    /// feature seq 83).
    pub(crate) serial_group: Option<u32>,
    /// Resource type names this system declared `accum` (dogfood seq 83
    /// IDEA 02): the batch merge folds each worker's per-field delta into
    /// the base instead of last-write-wins, and the conflict analysis lets
    /// accum-writers of the same resource share a batch.
    pub(crate) accum_resources: std::collections::HashSet<String>,
}

impl VM {
    /// Immutable global symbol table in exact slot order.
    pub fn global_symbols(&self) -> &[String] {
        self.global_names.as_slice()
    }

    /// Capture the canonical immutable identity of the currently installed
    /// executable program. Runtime values are intentionally excluded.
    pub fn compiled_program_manifest(&self) -> CompiledProgramManifest {
        CompiledProgramManifest::capture(self)
    }

    /// Content-addressed manifests for native implementations installed in
    /// this executable program. The returned slice is immutable; loading or
    /// replacing an implementation produces a new program identity.
    pub fn native_extension_manifests(&self) -> &[Arc<crate::ffi::NativeExtensionManifest>] {
        &self.native_extension_manifests
    }

    #[inline(always)]
    pub fn get_world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    #[inline(always)]
    pub fn get_world(&self) -> &World {
        &self.world
    }

    /// Walk only closure/stack roots and sweep unreachable GC allocations.
    ///
    /// This backup collector intentionally skips ECS world state, timeline, and
    /// event logs. Persistent ECS data is not managed by `GcHeap`.
    pub fn collect_cycles(&mut self) -> usize {
        let mut marked = HashSet::new();

        for val in &self.stack {
            val.trace(&mut marked);
        }
        for val in &self.globals {
            val.trace(&mut marked);
        }
        // In-flight event payloads are live state (latent-bug fix found
        // during the #7 composition pass: queued payloads were not roots).
        for (_, payload, _) in self
            .events_current
            .iter()
            .chain(self.events_next.iter())
            .chain(self.events_processing.iter())
        {
            payload.trace(&mut marked);
        }
        for (_, _, payload, _) in &self.delayed_events {
            payload.trace(&mut marked);
        }
        for frame in &self.frames {
            if let Some(captures) = &frame.captures {
                for &cell in captures.as_ref() {
                    let ptr = cell as usize;
                    if marked.insert(ptr) {
                        unsafe { (*cell).get().trace(&mut marked) };
                    }
                }
            }
        }
        // Completed-but-unawaited task results and recorded event payloads
        // are reachable from rad code, so they are roots too.
        for task in self.tasks.values() {
            if let TaskStatus::Completed(val) = &task.status {
                val.trace(&mut marked);
            }
        }
        for entry in &self.event_log {
            entry.payload.trace(&mut marked);
        }
        if let Some(settlement) = &self.settlement {
            for proposal in &settlement.proposals {
                proposal.payload.trace(&mut marked);
            }
            for patch in &settlement.patches {
                for write in &patch.writes {
                    for value in &write.component.values {
                        value.trace(&mut marked);
                    }
                }
            }
            if let Some(active) = &settlement.active {
                for write in &active.writes {
                    for value in &write.component.values {
                        value.trace(&mut marked);
                    }
                }
            }
        }
        for chunk in self.chunks.iter() {
            for val in &chunk.constants {
                val.trace(&mut marked);
            }
        }
        unsafe { self.gc.sweep(&marked) }
    }

    #[inline]
    pub(crate) fn allocate_frame_id(&mut self) -> u64 {
        let id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1).max(1);
        id
    }

    #[inline]
    pub(crate) fn allocate_settlement_id(&mut self) -> u64 {
        let id = self.next_settlement_id;
        self.next_settlement_id = self.next_settlement_id.wrapping_add(1).max(1);
        id
    }

    pub(crate) fn shared_state(&self) -> VmSharedState {
        VmSharedState {
            chunks: self.chunks.clone(),
            globals: self.globals.clone(),
            global_names: self.global_names.clone(),
            program_source_identity: self.program_source_identity.clone(),
            state_machines: self.state_machines.clone(),
            event_handlers: self.event_handlers.clone(),
            systems: self.systems.clone(),
            intent_registry: self.intent_registry.clone(),
            resolver_registry: self.resolver_registry.clone(),
            constraint_registry: self.constraint_registry.clone(),
            native_extension_manifests: self.native_extension_manifests.clone(),
            component_layouts: self.component_layouts.clone(),
            component_field_types: self.component_field_types.clone(),
            component_versions: self.component_versions.clone(),
            variant_layouts: self.variant_layouts.clone(),
            transient_resources: self.transient_resources.clone(),
            rng_state: self.rng_state,
            suppress_output: self.suppress_output,
            profile_copies: self.profile_copies,
            causal_value_limits: self.causal_value_limits,
            constraint_limit_profile: self.constraint_limit_profile.clone(),
        }
    }

    pub(crate) fn from_shared_state(shared: VmSharedState) -> Self {
        VM {
            chunks: shared.chunks,
            stack: Vec::with_capacity(1024),
            globals: shared.globals,
            global_names: shared.global_names,
            program_source_identity: shared.program_source_identity,
            frames: Vec::with_capacity(256),
            next_frame_id: 1,
            world: World::new(),
            state_machines: shared.state_machines,
            event_handlers: shared.event_handlers,
            systems: shared.systems,
            intent_registry: shared.intent_registry,
            resolver_registry: shared.resolver_registry,
            constraint_registry: shared.constraint_registry,
            native_extension_manifests: shared.native_extension_manifests,
            settlement: None,
            next_settlement_id: 1,
            causal_value_limits: shared.causal_value_limits,
            constraint_limit_profile: shared.constraint_limit_profile,
            migrations: HashMap::new(),
            events_current: Vec::new(),
            events_next: Vec::new(),
            events_processing: Vec::new(),
            delayed_events: Vec::new(),
            print_buffer: Vec::new(),
            eprint_buffer: Vec::new(),
            suppress_output: shared.suppress_output,
            profile_copies: shared.profile_copies,
            op_profile: false, // worker VMs don't profile (histogram is per main VM)
            op_counts: Vec::new(),
            trace_timeline: false,
            trace_patch: None,
            component_layouts: shared.component_layouts,
            component_field_types: shared.component_field_types,
            component_versions: shared.component_versions,
            variant_layouts: shared.variant_layouts,
            transient_resources: shared.transient_resources,
            indexed_decl: Arc::new(HashMap::new()),
            timeline: Vec::new(),
            event_log: Vec::new(),
            rng_state: shared.rng_state,
            tasks: HashMap::new(),
            next_task_id: 1,
            pending_io: HashMap::new(),
            in_async_context: false,
            #[cfg(not(target_arch = "wasm32"))]
            // Worker VMs are effect-isolated and may not perform I/O. Keeping
            // this disabled also matters on Windows: WORKER_VM is thread-local
            // to a Rayon thread, and joining a nested I/O thread from that
            // TLS destructor can race process teardown and abort after an
            // otherwise successful simulate_par() run.
            io_pool: IoPool::disabled(),
            #[cfg(not(target_arch = "wasm32"))]
            loaded_libraries: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            net_handles: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            next_net_handle_id: 1,
            current_trace_id: None,
            next_trace_id: 1,
            in_simulation_fork: 0,
            gc_pause: 0,
            is_worker: true,
            sys_args: Vec::new(),
            command_buffer: Vec::new(),
            once_guard_passed: false,
            fuel: u64::MAX,
            mem_limit: usize::MAX,
            constraint_meter: None,
            sandbox_caps: None,
            sandbox_input_json: None,
            sandbox_output_json: None,
            last_sandbox_output_json: None,
            last_sandbox_fuel_spent: 0,
            serial_schedule: false,
            recorder: None,
            replayer: None,
            observational_attempt_replay: false,
            ledger: crate::causality::CausalityLedger::default(),
            current_cause: crate::causality::Cause::Main,
            causality_frame: 0,
            emit_ids_current: Vec::new(),
            emit_ids_next: Vec::new(),
            gc: GcHeap::new(),
            arena: BumpArena::new(),
        }
    }

    pub(crate) fn sync_from_shared(&mut self, shared: &VmSharedState) {
        // Arc identity, not length: two different programs can have the same
        // chunk count, and a pooled worker VM must never run one program's
        // schedule against another program's system table.
        if !Arc::ptr_eq(&self.chunks, &shared.chunks) {
            self.chunks = Arc::clone(&shared.chunks);
            self.global_names = Arc::clone(&shared.global_names);
            self.program_source_identity = shared.program_source_identity.clone();
            self.state_machines = Arc::clone(&shared.state_machines);
            self.event_handlers = Arc::clone(&shared.event_handlers);
            self.systems = Arc::clone(&shared.systems);
            self.intent_registry = Arc::clone(&shared.intent_registry);
            self.resolver_registry = Arc::clone(&shared.resolver_registry);
            self.constraint_registry = Arc::clone(&shared.constraint_registry);
            self.component_layouts = Arc::clone(&shared.component_layouts);
            self.component_field_types = Arc::clone(&shared.component_field_types);
            self.component_versions = Arc::clone(&shared.component_versions);
            self.variant_layouts = Arc::clone(&shared.variant_layouts);
            self.transient_resources = Arc::clone(&shared.transient_resources);
        }
        // Globals refresh EVERY sync, not only on program change: global
        // values are handles into the MAIN VM's GC heap, and top-level
        // `let mut` rebinding makes the old objects garbage that the main
        // collector frees between parallel calls. A pooled worker that kept
        // its creation-time copy would then hold dangling pointers — its own
        // collector TRACES every global as a root, so the very next worker
        // GC dereferenced freed memory (A2's simulate_par 0xC0000005, ~1 in
        // 3 runs; allocation-shape dependent, which is why a str field
        // modulated it).
        self.globals = shared.globals.clone();
        self.native_extension_manifests = Arc::clone(&shared.native_extension_manifests);
        self.suppress_output = shared.suppress_output;
        self.profile_copies = shared.profile_copies;
        self.constraint_limit_profile = shared.constraint_limit_profile.clone();
        self.stack.clear();
        self.frames.clear();
        self.events_next.clear();
        self.emit_ids_next.clear();
        for cmd in &self.command_buffer {
            cmd.release_payload();
        }
        self.command_buffer.clear();
    }

    pub fn new() -> Self {
        Self::new_with_seed(Self::initial_rng_seed())
    }

    /// Construct a VM without consulting host time for its initial RNG seed.
    ///
    /// This is the deterministic embedding entry point for tests, replay
    /// harnesses, and isolated interpreters such as Miri. A zero seed is
    /// normalized to the same non-zero fallback used by `set_random_seed`.
    pub fn new_with_seed(seed: u64) -> Self {
        let mut gc = GcHeap::new();
        let mut globals = Vec::new();
        let mut global_names = Vec::new();
        for builtin in Builtin::ALL {
            global_names.push(builtin.name().to_string());
            globals.push(Value::from_builtin(&mut gc, builtin));
        }
        VM {
            chunks: Arc::new(Vec::new()),
            stack: Vec::new(),
            globals,
            global_names: Arc::new(global_names),
            program_source_identity: None,
            frames: Vec::new(),
            next_frame_id: 1,
            world: World::new(),
            state_machines: Arc::new(HashMap::new()),
            event_handlers: Arc::new(HashMap::new()),
            systems: Arc::new(HashMap::new()),
            intent_registry: Arc::new(HashMap::new()),
            resolver_registry: Arc::new(HashMap::new()),
            constraint_registry: Arc::new(Vec::new()),
            native_extension_manifests: Arc::new(Vec::new()),
            settlement: None,
            next_settlement_id: 1,
            causal_value_limits: crate::CausalValueLimits::default(),
            constraint_limit_profile: crate::constraint_types::ConstraintLimitProfile::default(),
            migrations: HashMap::new(),
            events_current: Vec::new(),
            events_next: Vec::new(),
            events_processing: Vec::new(),
            delayed_events: Vec::new(),
            print_buffer: Vec::new(),
            eprint_buffer: Vec::new(),
            suppress_output: false,
            profile_copies: false,
            op_profile: std::env::var("RAD_OP_PROFILE").is_ok_and(|v| v == "1"),
            op_counts: vec![0u64; 256],
            trace_timeline: false,
            trace_patch: None,
            component_layouts: Arc::new(HashMap::new()),
            component_field_types: Arc::new(HashMap::new()),
            component_versions: Arc::new(HashMap::new()),
            variant_layouts: Arc::new(HashMap::new()),
            transient_resources: Arc::new(HashSet::new()),
            indexed_decl: Arc::new(HashMap::new()),
            timeline: Vec::new(),
            event_log: Vec::new(),
            rng_state: Self::normalize_random_seed(seed),
            tasks: HashMap::new(),
            next_task_id: 1,
            pending_io: HashMap::new(),
            in_async_context: false,
            #[cfg(not(target_arch = "wasm32"))]
            io_pool: IoPool::new(4),
            #[cfg(not(target_arch = "wasm32"))]
            loaded_libraries: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            net_handles: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            next_net_handle_id: 1,
            current_trace_id: None,
            next_trace_id: 1,
            in_simulation_fork: 0,
            gc_pause: 0,
            is_worker: false,
            sys_args: Vec::new(),
            command_buffer: Vec::new(),
            once_guard_passed: false,
            fuel: u64::MAX,
            mem_limit: usize::MAX,
            constraint_meter: None,
            sandbox_caps: None,
            sandbox_input_json: None,
            sandbox_output_json: None,
            last_sandbox_output_json: None,
            last_sandbox_fuel_spent: 0,
            serial_schedule: false,
            recorder: None,
            replayer: None,
            observational_attempt_replay: false,
            ledger: crate::causality::CausalityLedger::default(),
            current_cause: crate::causality::Cause::Main,
            causality_frame: 0,
            emit_ids_current: Vec::new(),
            emit_ids_next: Vec::new(),
            gc,
            arena: BumpArena::new(),
        }
    }

    /// Start recording a replay trace. Captures the current RNG state as the
    /// trace seed, so call this before `run` and after any seed override.
    pub fn enable_recording(&mut self, source: &str) {
        self.recorder = Some(crate::replay::TraceRecorder::new(source, self.rng_state));
    }

    /// Record the exact language-feature contract needed to compile the
    /// embedded source again. The feature list is canonicalized and protected
    /// by its own trace-header digest.
    pub fn enable_recording_with_features(&mut self, source: &str, features: &[String]) {
        self.recorder = Some(crate::replay::TraceRecorder::new_with_features(
            source,
            self.rng_state,
            features,
        ));
    }

    /// Record a self-contained multi-module source bundle. The authenticated
    /// layout is data, not a lexer convention, so arbitrary comments cannot
    /// alter replay locations.
    pub fn enable_recording_with_source_layout(
        &mut self,
        source: &str,
        features: &[String],
        source_layout: &crate::source_bundle::SourceLayout,
    ) {
        self.recorder = Some(crate::replay::TraceRecorder::new_with_features_and_layout(
            source,
            self.rng_state,
            features,
            source_layout,
        ));
    }

    /// Finish recording and return the trace as JSONL, if recording was on.
    /// Appends the end record (world digest at exit or crash point) that
    /// replay verifies itself against.
    pub fn take_trace(&mut self) -> Option<String> {
        self.take_trace_with_outcome(None)
    }

    pub fn take_trace_with_outcome(&mut self, error: Option<&str>) -> Option<String> {
        let digest = self.world.content_digest();
        self.recorder.take().map(|mut r| {
            r.record_end_with_outcome(&digest, error);
            r.to_jsonl()
        })
    }

    /// Enter replay mode: managed builtins are served from the trace, and
    /// the RNG is rewound to the recorded seed. When timeline capture is on,
    /// the (empty) pre-run world becomes `timeline[0]`.
    pub fn enable_replay(&mut self, mut replayer: crate::replay::TraceReplayer) {
        self.set_random_seed(replayer.seed());
        if replayer.capturing_timeline() {
            replayer.push_timeline_snapshot(self.world.snapshot());
        }
        self.replayer = Some(replayer);
    }

    /// Finish replay and report how faithfully the trace was consumed.
    pub fn finish_replay(&mut self) -> Option<crate::replay::ReplayReport> {
        self.finish_replay_with_outcome(None)
    }

    pub fn finish_replay_with_outcome(
        &mut self,
        error: Option<&str>,
    ) -> Option<crate::replay::ReplayReport> {
        let digest = self.world.content_digest();
        self.replayer
            .take()
            .map(|r| r.report_with_outcome(&digest, error))
    }

    /// The causality ledger (#4): provenance of every main-timeline write.
    pub fn causality_ledger(&self) -> &crate::causality::CausalityLedger {
        &self.ledger
    }

    /// Bound retained causal history for long-running embedded VMs.
    /// Settlement proposal fan-in uses the same retention policy as legacy
    /// writes and event ancestry.
    pub fn set_causality_retention_cap(&mut self, cap: usize) {
        self.ledger.set_retention_cap(cap);
    }

    /// Take the ledger out of the VM (the time-travel server keeps it to
    /// answer `why` at any frame).
    pub fn take_causality_ledger(&mut self) -> crate::causality::CausalityLedger {
        std::mem::take(&mut self.ledger)
    }

    /// Record a main-timeline world write in the causality ledger. No-op in
    /// simulation forks and worker VMs — speculative or staged writes get
    /// their provenance when (and if) they land on the main world.
    pub(crate) fn record_causal_write(
        &mut self,
        entity: Option<u32>,
        component: &str,
        kind: crate::causality::WriteKind,
        value: String,
    ) {
        if self.in_simulation_fork > 0 || self.is_worker {
            return;
        }
        let entity_name = entity.and_then(|id| self.world.entity_name(id));
        self.ledger.record_write(
            self.causality_frame,
            entity,
            entity_name,
            component,
            value,
            kind,
            self.current_cause.clone(),
        );
    }

    /// Bounded display summary of a component's fields, for ledger records.
    pub(crate) fn component_summary(data: &crate::value::ComponentData) -> String {
        let mut s = String::from("{ ");
        for (i, (k, v)) in data.layout.iter().zip(data.values.iter()).enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(k);
            s.push_str(": ");
            s.push_str(&v.to_string());
        }
        s.push_str(" }");
        crate::causality::summarize(&s)
    }

    /// CoW snapshot of the live world (pub for the CLI's retroactive-edit
    /// diff, which compares the original and edited timelines).
    pub fn world_snapshot(&self) -> crate::world::WorldSnapshot {
        self.world.snapshot()
    }

    /// Snapshot the world **plus the in-flight event queue** — the full
    /// program state at this instant. Payloads are persisted so the snapshot
    /// outlives any GC epoch. This is what `fork()` hands out: a snapshot
    /// that drops pending events is not a snapshot (composition pass, #7).
    pub(crate) fn snapshot_with_events(&self) -> crate::world::WorldSnapshot {
        let mut snap = self.world.snapshot();
        if !self.events_next.is_empty() {
            let events: Vec<(String, Value, u64)> = self
                .events_next
                .iter()
                .map(|(name, payload, tid)| {
                    let persisted = payload.deep_copy(&mut crate::value::PersistentStore);
                    (name.clone(), persisted, *tid)
                })
                .collect();
            snap.events = std::sync::Arc::new(events);
            snap.emit_ids = std::sync::Arc::new(self.emit_ids_next.clone());
        }
        if !self.delayed_events.is_empty() {
            let delayed: Vec<(i64, String, Value, u64)> = self
                .delayed_events
                .iter()
                .map(|(left, name, payload, emit_id)| {
                    let persisted = payload.deep_copy(&mut crate::value::PersistentStore);
                    (*left, name.clone(), persisted, *emit_id)
                })
                .collect();
            snap.delayed = std::sync::Arc::new(delayed);
        }
        snap
    }

    /// Render an event payload for the causality ledger with entity fields
    /// resolved to their spawn names: `Clawed { monster: ghoul-1, dmg: 2 }`
    /// instead of `Clawed { monster: 1, dmg: 2 }`. The ledger already names
    /// the *written* entity ("Vital of you"); the emitter's payload deserves
    /// the same courtesy. Unnamed entities keep their bare id. Only the
    /// shapes an event payload can carry are walked; everything else
    /// falls back to the plain `Display`.
    pub(crate) fn ledger_payload(&self, v: &Value) -> String {
        use crate::value::Object;
        use std::fmt::Write as _;
        fn walk(world: &crate::world::World, v: &Value, out: &mut String, depth: usize) {
            if depth > 6 {
                out.push('…');
                return;
            }
            match v.as_object() {
                Some(Object::EntityId(id)) => match world.entity_name(*id) {
                    Some(name) => {
                        let _ = write!(out, "{}", name);
                    }
                    None => {
                        let _ = write!(out, "{}", id);
                    }
                },
                Some(Object::Component(c)) => {
                    let _ = write!(out, "{} {{", crate::value::display_type_name(&c.type_name));
                    if !c.layout.is_empty() {
                        out.push(' ');
                        let mut first = true;
                        for (k, fv) in c.layout.iter().zip(c.values.iter()) {
                            if !first {
                                out.push_str(", ");
                            }
                            first = false;
                            let _ = write!(out, "{}: ", k);
                            walk(world, fv, out, depth + 1);
                        }
                        out.push(' ');
                    }
                    out.push('}');
                }
                Some(Object::List(list)) => {
                    out.push('[');
                    for (i, item) in list.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        walk(world, item, out, depth + 1);
                    }
                    out.push(']');
                }
                Some(Object::Tuple(items)) => {
                    out.push('(');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        walk(world, item, out, depth + 1);
                    }
                    if items.len() == 1 {
                        out.push(',');
                    }
                    out.push(')');
                }
                _ => {
                    let _ = write!(out, "{}", v);
                }
            }
        }
        let mut out = String::new();
        walk(&self.world, v, &mut out, 0);
        out
    }

    /// Enqueue an event from the host (streaming sessions, D4): the
    /// embedder-side mirror of `Op::Emit`. The payload joins `events_next`
    /// with a fresh trace id and a causality emit record, exactly as if a
    /// rad program had executed `emit Name { .. }` — so `why()` answers for
    /// host-pushed events too, and the next `flush_events()` dispatches it
    /// on the same frame clock.
    pub(crate) fn enqueue_event(&mut self, payload: Value) -> Result<(), String> {
        let event_name = payload.type_name().to_string();
        if !self.event_handlers.contains_key(&event_name)
            && !self.component_layouts.contains_key(&event_name)
        {
            return Err(format!(
                "enqueue_event: '{}' is not a declared event",
                event_name
            ));
        }
        let trace_id = self.next_trace_id;
        self.next_trace_id += 1;
        let emit_id = {
            let summary = crate::causality::summarize(&self.ledger_payload(&payload));
            // Host pushes are top-level-equivalent: there is no rad frame
            // above them to attribute to.
            self.ledger.record_emit(
                self.causality_frame,
                &event_name,
                summary,
                crate::causality::Cause::Main,
            )
        };
        self.emit_ids_next.push(emit_id);
        self.events_next.push((event_name, payload, trace_id));
        Ok(())
    }

    /// Restore the in-flight event queue from a snapshot (the other half of
    /// `snapshot_with_events`): pending events and their causality ids come
    /// back exactly as captured.
    pub(crate) fn restore_events_from(&mut self, snap: &crate::world::WorldSnapshot) {
        self.events_current.clear();
        self.emit_ids_current.clear();
        self.events_next = snap.events.as_ref().clone();
        self.emit_ids_next = snap.emit_ids.as_ref().clone();
        // delayed timers travel with the snapshot: a rewind must not keep
        // the abandoned timeline's timers ticking, nor lose the target's
        self.delayed_events = snap.delayed.as_ref().clone();
    }

    /// Content digest of the live world (pub for the CLI).
    pub fn world_digest(&self) -> String {
        self.world.content_digest()
    }

    /// Finish a time-travel replay session: appends the program-end world as
    /// the final timeline entry and returns (timeline, report).
    pub fn finish_replay_session(
        &mut self,
    ) -> Option<(
        Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
        crate::replay::ReplayReport,
    )> {
        self.finish_replay_session_with_outcome(None)
    }

    pub fn finish_replay_session_with_outcome(
        &mut self,
        error: Option<&str>,
    ) -> Option<(
        Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
        crate::replay::ReplayReport,
    )> {
        let end_snap = self.world.snapshot();
        let digest = self.world.content_digest();
        self.replayer.take().map(|mut replayer| {
            replayer.push_timeline_snapshot(end_snap);
            let timeline = replayer.take_timeline();
            (timeline, replayer.report_with_outcome(&digest, error))
        })
    }

    pub fn suppress_output(&mut self) {
        self.suppress_output = true;
    }

    /// Force `schedule`/phases to run serially in topological order (the
    /// `--serial-schedule` lever). No effect on explicit `simulate_par`.
    pub fn set_serial_schedule(&mut self, serial: bool) {
        self.serial_schedule = serial;
    }

    pub fn set_profile_copies(&mut self, enabled: bool) {
        self.profile_copies = enabled;
        crate::value::set_profile_copy_context(enabled, 0);
    }

    fn initial_rng_seed() -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            0xA5A5_5A5A_C3C3_3C3C
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xA5A5_5A5A_C3C3_3C3C);
            let stack_addr = (&nanos as *const u64 as usize) as u64;
            let seed = nanos ^ stack_addr.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15;
            if seed == 0 {
                0xD1B5_4A32_D192_ED03
            } else {
                seed
            }
        }
    }

    fn normalize_random_seed(seed: u64) -> u64 {
        if seed == 0 {
            0xD1B5_4A32_D192_ED03
        } else {
            seed
        }
    }

    pub(crate) fn set_random_seed(&mut self, seed: u64) {
        self.rng_state = Self::normalize_random_seed(seed);
    }

    pub(crate) fn next_random_u64(&mut self) -> u64 {
        if self.rng_state == 0 {
            self.rng_state = 0xD1B5_4A32_D192_ED03;
        }
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(crate) fn next_random_f64(&mut self) -> f64 {
        let n = self.next_random_u64() >> 11;
        (n as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    pub(crate) fn random_bounded_u64(&mut self, bound: u64) -> u64 {
        if bound <= 1 {
            return 0;
        }
        let threshold = u64::MAX - (u64::MAX % bound);
        loop {
            let n = self.next_random_u64();
            if n < threshold {
                return n % bound;
            }
        }
    }

    /// Verify and append host-supplied bytecode. No instruction from an
    /// invalid chunk can execute.
    ///
    /// This safe raw-builder ingress accepts only immediate constants. Heap
    /// values carry untyped GC pointers, so accepting them here could not
    /// prove their ownership or lifetime. Compiler/WASM bundles use the
    /// explicit owning-heap path below.
    pub fn load_verified_chunk(&mut self, chunk: Chunk) -> Result<usize, crate::VerificationError> {
        if let Some(index) = chunk.constants().iter().position(Value::is_heap_object_tag) {
            return Err(crate::VerificationError::at(
                &chunk,
                0,
                format!(
                    "heap-backed constant {index} requires an owning GC bundle; raw chunk loading accepts immediate constants only"
                ),
            ));
        }
        let chunk = chunk.verify_and_seal()?;
        Ok(self.load_chunk(chunk))
    }

    /// Internal ingress for chunks whose constants were allocated directly
    /// in this VM's heap (tests and VM-owned construction only).
    #[cfg(test)]
    pub(crate) fn load_vm_owned_chunk(
        &mut self,
        chunk: Chunk,
    ) -> Result<usize, crate::VerificationError> {
        let chunk = chunk.verify_and_seal()?;
        Ok(self.load_chunk(chunk))
    }

    /// Append an immutable proof-bearing artifact. The bytes and proof cannot
    /// be separated or mutated through the public type.
    pub(crate) fn load_chunk(&mut self, chunk: SealedChunk) -> usize {
        let v = Arc::make_mut(&mut self.chunks);
        let id = v.len();
        v.push(chunk);
        debug_assert!(v[id].instruction_count() > 0);
        id
    }

    /// Verify bytecode before merging its separate constant heap, then append
    /// both atomically from the embedder's perspective.
    /// # Safety
    ///
    /// Every heap-backed value reachable from `chunk.constants` must be owned
    /// by `chunk_gc`. Passing unrelated storage can leave dangling raw object
    /// pointers after the source allocator is dropped.
    pub unsafe fn load_verified_chunk_with_gc(
        &mut self,
        chunk: Chunk,
        chunk_gc: GcHeap,
    ) -> Result<usize, crate::VerificationError> {
        let chunk = chunk.verify_and_seal()?;
        self.gc.merge(chunk_gc);
        Ok(self.load_chunk(chunk))
    }

    /// Compatibility spelling for [`Self::load_verified_chunk_with_gc`].
    /// # Safety
    ///
    /// See [`Self::load_verified_chunk_with_gc`].
    pub unsafe fn load_chunk_with_gc(
        &mut self,
        chunk: Chunk,
        chunk_gc: GcHeap,
    ) -> Result<usize, crate::VerificationError> {
        // SAFETY: forwarded contract is identical to this function's.
        unsafe { self.load_verified_chunk_with_gc(chunk, chunk_gc) }
    }

    /// Runtime-defense tests deliberately bypass the host verifier. This is
    /// never compiled into a production library.
    #[cfg(test)]
    pub(crate) fn load_unchecked_chunk(&mut self, chunk: Chunk) -> usize {
        let chunks = Arc::make_mut(&mut self.chunks);
        let id = chunks.len();
        chunks.push(SealedChunk::from_unchecked_for_test(chunk));
        id
    }

    /// Borrow this VM’s [`GcHeap`] to build [`Value`]s from host/embedder code (e.g. arguments to
    /// [`Self::call_value`]). All such values must live on this heap before entering the VM.
    #[inline]
    pub(crate) fn gc_mut(&mut self) -> &mut GcHeap {
        &mut self.gc
    }

    pub fn load_compile_result(&mut self, mut result: CompileResult) {
        // Verify the complete compiler product before mutating VM registries
        // or merging its heap. Compiler corruption is an internal fault; host
        // supplied chunks use the fallible load_verified_chunk API above.
        let sealed_chunks = std::mem::take(&mut result.chunks)
            .into_iter()
            .map(|chunk| {
                chunk
                    .verify_and_seal()
                    .unwrap_or_else(|error| panic!("compiler produced invalid bytecode: {error}"))
            })
            .collect::<Vec<_>>();
        self.gc.merge(result.gc);
        {
            let intents = Arc::make_mut(&mut self.intent_registry);
            for intent in &result.intents {
                intents.insert(
                    intent.name.clone(),
                    IntentRuntimeInfo {
                        name: intent.name.clone(),
                        key_field: intent.key_field.clone(),
                        fields: Arc::new(intent.fields.clone()),
                    },
                );
            }
            let resolvers = Arc::make_mut(&mut self.resolver_registry);
            for resolver in &result.resolvers {
                resolvers.insert(
                    resolver.intent.clone(),
                    ResolverRuntimeInfo {
                        name: resolver.name.clone(),
                        intent: resolver.intent.clone(),
                        global_slot: resolver.global_slot,
                    },
                );
            }
            let constraints = Arc::make_mut(&mut self.constraint_registry);
            constraints.extend(
                result
                    .constraints
                    .iter()
                    .map(|constraint| ConstraintRuntimeInfo {
                        name: constraint.name.clone(),
                        attached_component: constraint.attached_component.clone(),
                        watches: Arc::new(constraint.watches.clone()),
                        global_slot: constraint.global_slot,
                    }),
            );
            constraints.sort_by(|left, right| {
                (&left.name, &left.attached_component)
                    .cmp(&(&right.name, &right.attached_component))
            });
        }
        {
            let sm = Arc::make_mut(&mut self.state_machines);
            for machine in result.state_machines {
                sm.insert(machine.name, machine.states);
            }
        }
        {
            let systems = Arc::make_mut(&mut self.systems);
            for sys in result.systems {
                let mut params: Vec<(String, bool, String)> = Vec::new();
                let mut resource_params: Vec<(String, bool, String)> = Vec::new();
                let mut accum_resources = std::collections::HashSet::new();
                for p in sys.params {
                    if p.is_accum && p.is_resource {
                        accum_resources.insert(p.comp_type.clone());
                    }
                    if p.is_resource {
                        resource_params.push((p.name, p.is_mut, p.comp_type));
                    } else {
                        params.push((p.name, p.is_mut, p.comp_type));
                    }
                }
                systems.insert(
                    sys.name,
                    SystemRuntimeInfo {
                        params,
                        resource_params,
                        chunk_id: sys.chunk_id,
                        after: sys.after,
                        before: sys.before,
                        serial_group: sys.serial_group,
                        accum_resources,
                    },
                );
            }
        }
        for m in &result.migrations {
            self.migrations.insert(
                m.component.clone(),
                MigrationEntry {
                    chunk_id: m.chunk_id,
                    param_slot: m.param_slot,
                    version_slot: m.version_slot,
                },
            );
        }
        {
            let eh = Arc::make_mut(&mut self.event_handlers);
            for h in result.handlers {
                eh.entry(h.event_name).or_default().push(HandlerEntry {
                    chunk_id: h.chunk_id,
                    param_slot: h.param_slot,
                    once: h.once,
                    fired: false,
                    has_guard: h.has_guard,
                });
            }
        }
        {
            let cl = Arc::make_mut(&mut self.component_layouts);
            for (name, layout) in result.component_layouts {
                cl.insert(name, Arc::new(layout));
            }
        }
        {
            let ft = Arc::make_mut(&mut self.component_field_types);
            for (name, fields) in result.component_field_types {
                ft.insert(name, Arc::new(fields));
            }
        }
        {
            let cv = Arc::make_mut(&mut self.component_versions);
            for (name, version) in result.component_versions {
                cv.insert(name, version);
            }
        }
        {
            let mut indexed_fields: HashMap<String, HashSet<String>> = (*self.indexed_decl).clone();
            for (name, fields) in result.indexed_component_fields {
                indexed_fields.insert(name, fields.into_iter().collect());
            }
            self.indexed_decl = Arc::new(indexed_fields);
            self.world
                .set_indexed_fields_arc(Arc::clone(&self.indexed_decl));
        }
        {
            let vl = Arc::make_mut(&mut self.variant_layouts);
            for (key, layout) in result.variant_layouts {
                vl.insert(key, layout);
            }
        }
        {
            let tr = Arc::make_mut(&mut self.transient_resources);
            tr.extend(result.transient_resources);
        }
        self.global_names = Arc::new(result.global_names);
        self.program_source_identity = result.program_source_identity.map(Arc::from);
        if self.globals.len() < self.global_names.len() {
            self.globals.resize(self.global_names.len(), Value::NIL);
        }
        for chunk in sealed_chunks {
            self.load_chunk(chunk);
        }
    }

    /// Execute a chunk and preserve structured settlement rejection data.
    pub fn run_detailed(
        &mut self,
        chunk_id: usize,
    ) -> Result<(), crate::constraint_types::VmFailure> {
        if chunk_id >= self.chunks.len() {
            return Err(format!("Invalid chunk id {}", chunk_id).into());
        }
        // A previous public boundary must already have enforced this
        // invariant. Restore transaction-local state defensively instead of
        // merely dropping a stale context if an internal caller violated it.
        self.abort_settlement();
        let next_frame_id_before = self.next_frame_id;
        crate::value::set_profile_copy_context(self.profile_copies, 0);
        self.frames.clear();
        self.print_buffer.clear();
        self.eprint_buffer.clear();
        self.events_current.clear();
        self.events_next.clear();
        self.events_processing.clear();
        self.emit_ids_current.clear();
        self.emit_ids_next.clear();
        self.tasks.clear();
        self.next_task_id = 1;
        self.pending_io.clear();
        self.in_async_context = false;
        let stack_base = self.stack.len();
        let frame_id = self.allocate_frame_id();
        self.frames.push(CallFrame {
            frame_id,
            chunk_id,
            ip: 0,
            stack_base,
            captures: None,
            system_writeback: None,
        });
        let result = self.run_frames(0).map_err(|failure| match failure {
            crate::constraint_types::VmFailure::Runtime(mut error) => {
                if !error.message.starts_with("[line") {
                    error.message = self.runtime_error(error.message);
                }
                crate::constraint_types::VmFailure::Runtime(error)
            }
            other => other,
        });
        let result = self.enforce_settlement_balance(result);
        if result.is_err() {
            // BeginSettlement and EndSettlement are separate bytecodes. A
            // body/law failure can bypass EndSettlement, so every public run
            // return must explicitly discard an unfinished transaction.
            self.abort_settlement();
            self.frames.clear();
            self.stack.truncate(stack_base);
            self.next_frame_id = next_frame_id_before;
        }
        crate::value::set_profile_copy_context(false, 0);
        if self.op_profile {
            let total: u64 = self.op_counts.iter().sum();
            eprintln!("== op profile: {} dispatches ==", total);
            let mut counted: Vec<(usize, u64)> = self
                .op_counts
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, n)| *n > 0)
                .collect();
            counted.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
            for (byte, n) in counted.into_iter().take(24) {
                let name = crate::opcode::Op::from_byte(byte as u8)
                    .map(|o| format!("{:?}", o))
                    .unwrap_or_else(|_| format!("op#{}", byte));
                eprintln!(
                    "  {:<18} {:>12}  {:>5.1}%",
                    name,
                    n,
                    n as f64 * 100.0 / total as f64
                );
            }
        }
        result
    }

    /// Compatibility execution API. New embedders should prefer
    /// [`Self::run_detailed`] so rejected settlements remain structured.
    pub fn run(&mut self, chunk_id: usize) -> Result<(), String> {
        self.run_detailed(chunk_id)
            .map_err(|failure| failure.render_compat())
    }

    pub(crate) fn allocate_task_id(&mut self) -> u64 {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        task_id
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn spawn_io_task<F>(&mut self, work: F) -> Value
    where
        F: FnOnce() -> Result<IoTaskPayload, String> + Send + 'static,
    {
        let task_id = self.allocate_task_id();
        self.tasks.insert(
            task_id,
            TaskRecord {
                id: task_id,
                status: TaskStatus::Ready,
            },
        );
        let rx = self.io_pool.submit(work);
        self.pending_io.insert(task_id, rx);
        Value::from_task(&mut self.gc, task_id)
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl VM {
    /// Complete stable-enough signature for adversarial transaction tests.
    /// Pointer identities inside Values are deliberately rendered only while
    /// comparing one reused VM before/after a fault.
    pub(crate) fn observable_state_signature(&self) -> String {
        let mut pending_io: Vec<u64> = self.pending_io.keys().copied().collect();
        pending_io.sort_unstable();
        let frames = self
            .frames
            .iter()
            .map(|frame| {
                (
                    frame.frame_id,
                    frame.chunk_id,
                    frame.ip,
                    frame.stack_base,
                    frame.captures.as_ref().map(|captures| captures.len()),
                    frame.system_writeback.is_some(),
                )
            })
            .collect::<Vec<_>>();
        format!(
            "chunks={};world={};ledger={:?};events={:?}|{:?}|{:?}|{:?};emit_ids={:?}|{:?};globals={:?};output={:?}|{:?};rng={};fuel={};mem={};tasks={:?};next_task={};pending_io={:?};async={};timeline={};event_log={:?};commands={:?};settlement={};next_frame={};next_settlement={};frames={:?};stack={:?};sandbox={:?}|{:?}|{:?}|{};trace={:?}|{};cause={:?}|{};once={}",
            self.chunks.len(),
            self.world.content_digest(),
            self.ledger,
            self.events_current,
            self.events_next,
            self.events_processing,
            self.delayed_events,
            self.emit_ids_current,
            self.emit_ids_next,
            self.globals,
            self.print_buffer,
            self.eprint_buffer,
            self.rng_state,
            self.fuel,
            self.mem_limit,
            self.tasks,
            self.next_task_id,
            pending_io,
            self.in_async_context,
            self.timeline.len(),
            self.event_log,
            self.command_buffer,
            self.settlement.is_some(),
            self.next_frame_id,
            self.next_settlement_id,
            frames,
            self.stack,
            self.sandbox_input_json,
            self.sandbox_output_json,
            self.last_sandbox_output_json,
            self.last_sandbox_fuel_spent,
            self.current_trace_id,
            self.next_trace_id,
            self.current_cause,
            self.causality_frame,
            self.once_guard_passed,
        )
    }
}
