

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
    pub(crate) relation_runtime_manifest:
        Option<Arc<crate::relation_runtime::RelationRuntimeManifest>>,
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
    pub(crate) loaded_libraries: Vec<crate::ffi::LoadedNativeLibrary>,
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

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
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
