


impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[line {}:{}] {}", self.line, self.col, self.message)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Local {
    pub(crate) name: String,
    pub(crate) depth: usize,
    pub(crate) mutable: bool,
    pub(crate) is_captured: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Upvalue {
    pub(crate) is_local: bool,
    pub(crate) index: u16,
}

pub(crate) struct FnScope {
    pub(crate) chunk: Chunk,
    pub(crate) locals: Vec<Local>,
    pub(crate) upvalues: Vec<Upvalue>,
    pub(crate) scope_depth: usize,
    pub(crate) settlement_depth: usize,
    pub(crate) loop_contexts: Vec<LoopCtx>,
    pub(crate) last_get_local: std::collections::HashMap<u16, usize>,
    pub(crate) unique_locals: std::collections::HashSet<String>,
    /// Offset of the most recently emitted opcode byte (not operands) —
    /// lets peepholes verify "the previous instruction starts here".
    pub(crate) prev_instr_start: usize,
    /// High-water mark of every offset that is (or will be) a jump target.
    /// Peephole fusions never rewrite bytes at or after a label, so a jump
    /// can never land inside a fused instruction.
    pub(crate) label_high_water: usize,
}

pub(crate) struct LoopCtx {
    pub(crate) loop_depth: usize,
    pub(crate) settlement_depth: usize,
    pub(crate) loop_start: usize,
    pub(crate) break_holes: Vec<usize>,
    pub(crate) continue_holes: Vec<usize>,
    pub(crate) writebacks: Vec<(u16, u16)>, // (entity_slot, comp_slot)
}

#[derive(Debug, Clone)]
pub struct SystemChunkInfo {
    pub name: String,
    pub params: Vec<SystemParam>,
    pub chunk_id: usize,
    pub after: Vec<String>,
    pub before: Vec<String>,
    /// Set when the system belongs to a `serial phase`: systems sharing a
    /// group id never share a parallel batch (dogfood feature seq 83).
    /// Stamped after all declarations compile, since a phase may be
    /// declared before or after its member systems.
    pub serial_group: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SystemParam {
    pub name: String,
    pub is_mut: bool,
    /// `accum` param (dogfood seq 83 IDEA 02): writable like `mut`, but in a
    /// parallel batch each worker's per-field DELTA is folded into the base
    /// instead of last-write-wins. Only meaningful with `is_resource`.
    pub is_accum: bool,
    pub comp_type: String,
    pub is_resource: bool,
}

#[derive(Debug, Clone)]
pub struct HandlerChunkInfo {
    pub event_name: String,
    pub param_name: String,
    pub param_slot: u16,
    pub chunk_id: usize,
    pub once: bool,
    pub is_async: bool,
    pub has_guard: bool,
}

/// Schema migration (list item #5): one compiled `migrate X(old) { … }`
/// body, invoked by `load_world` when the persisted shape of `X` differs
/// from the declared one.
#[derive(Debug, Clone)]
pub struct MigrationChunkInfo {
    pub component: String,
    pub param_slot: u16,
    /// Slot of the optional `from_version` parameter (dogfood seq 69):
    /// `migrate X(old, from_version)` receives the save's declared schema
    /// version for X as an int (0 for saves without one).
    pub version_slot: Option<u16>,
    pub chunk_id: usize,
}

#[derive(Debug, Clone)]
pub struct StateTransitionInfo {
    pub event: String,
    pub target: String,
    pub guard_chunk_id: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct StateMachineInfo {
    pub name: String,
    pub states: HashMap<String, Vec<StateTransitionInfo>>,
}

#[derive(Debug, Clone)]
pub struct IntentChunkInfo {
    pub name: String,
    pub key_field: String,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolverChunkInfo {
    pub name: String,
    pub intent: String,
    pub global_slot: u16,
}

#[derive(Debug, Clone)]
pub struct ConstraintChunkInfo {
    pub name: String,
    pub attached_component: String,
    pub watches: Vec<String>,
    pub global_slot: u16,
}

pub struct CompileResult {
    pub(crate) chunks: Vec<Chunk>,
    pub systems: Vec<SystemChunkInfo>,
    pub handlers: Vec<HandlerChunkInfo>,
    pub migrations: Vec<MigrationChunkInfo>,
    pub state_machines: Vec<StateMachineInfo>,
    pub intents: Vec<IntentChunkInfo>,
    pub resolvers: Vec<ResolverChunkInfo>,
    pub constraints: Vec<ConstraintChunkInfo>,
    pub(crate) layout_analysis: layout_analysis::LayoutAnalysis,
    pub(crate) materialization_plan: materialization::MaterializationPlan,
    pub component_layouts: HashMap<String, Vec<String>>,
    /// Declared field types per component/resource/struct, from the checker.
    /// The deserialization boundary (`load_world`, `fork_from_bytes`,
    /// `migrate` results) validates incoming values against these. Empty for
    /// checker-less compiles (replay of embedded trace source) — validation
    /// is skipped there, never wrongly strict.
    pub component_field_types: HashMap<String, Vec<(String, crate::types::Ty)>>,
    pub indexed_component_fields: HashMap<String, Vec<String>>,
    /// `transient resource` names — excluded from world_digest()/save_world().
    pub transient_resources: std::collections::HashSet<String>,
    /// Declared schema versions (`component X v2`), resolved name →
    /// version; nonzero entries only. Embedded per type in `save_world()`
    /// output and handed to `migrate X(old, from_version)` on load
    /// (dogfood feature seq 69 IDEA 03).
    pub component_versions: HashMap<String, u32>,
    pub variant_layouts: HashMap<(String, String), Vec<String>>,
    pub global_names: Vec<String>,
    /// Canonical identity of the source/module graph that produced this
    /// artifact. Bytecode-only embedders may leave this absent; normal CLI
    /// and replay compilation install the authenticated `SourceLayout`
    /// digest so portable program identity also binds module resolution.
    pub program_source_identity: Option<String>,
    pub warnings: Vec<CompileWarning>,
    /// Heap allocations for constants embedded in chunks (merged into VM on load).
    pub(crate) gc: GcHeap,
}

impl std::fmt::Debug for CompileResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompileResult")
            .field("chunks", &self.chunks)
            .field("systems", &self.systems)
            .field("handlers", &self.handlers)
            .field("state_machines", &self.state_machines)
            .field("layout_analysis", &self.layout_analysis)
            .field("materialization_plan", &self.materialization_plan)
            .field("component_layouts", &self.component_layouts)
            .field("variant_layouts", &self.variant_layouts)
            .field("global_names", &self.global_names)
            .field("program_source_identity", &self.program_source_identity)
            .field("warnings", &self.warnings)
            .field("gc", &"<GcHeap>")
            .finish()
    }
}

pub struct Compiler {
    pub(crate) functions: Vec<FnScope>,
    pub(crate) component_types: HashMap<String, Vec<(String, Option<TypeExpr>, Expr)>>,
    pub(crate) resource_types: HashMap<String, Vec<(String, Option<TypeExpr>, Expr)>>,
    pub(crate) chunks: Vec<Chunk>,
    pub(crate) systems: Vec<SystemChunkInfo>,
    pub(crate) handlers: Vec<HandlerChunkInfo>,
    pub(crate) migrations: Vec<MigrationChunkInfo>,
    pub(crate) state_machines: Vec<StateMachineInfo>,
    pub(crate) intent_types: HashMap<String, (String, Vec<String>)>,
    pub(crate) resolvers: Vec<ResolverChunkInfo>,
    pub(crate) constraints: Vec<ConstraintChunkInfo>,
    pub(crate) temp_counter: u32,
    pub(crate) global_mutability: HashMap<String, bool>,
    pub(crate) for_iter_kinds: HashMap<NodeId, ForIterKind>,
    pub(crate) checker_components: HashMap<String, ComponentType>,
    pub(crate) checker_resources: HashMap<String, ResourceType>,
    pub(crate) checker_sum_types: HashMap<String, SumTypeDef>,
    pub(crate) type_redirects: HashMap<String, String>,
    pub(crate) variant_shorthand: std::collections::HashSet<(String, String)>,
    pub(crate) spread_lengths: HashMap<crate::ast::Span, usize>,
    pub(crate) global_slots: HashMap<String, u16>,
    pub(crate) global_names: Vec<String>,
    pub(crate) program_source_identity: Option<String>,
    pub(crate) module_aliases: HashMap<String, HashMap<String, String>>,
    pub(crate) alias_decls: HashMap<String, Vec<Decl>>,
    pub(crate) current_alias_scope: Option<HashMap<String, String>>,
    pub(crate) file_private_scopes: HashMap<u32, HashMap<String, String>>,
    pub(crate) current_file_scope: Option<HashMap<String, String>>,
    pub(crate) features: Vec<String>,
    /// When true, `debug_trace` is compiled as a no-op pass-through (no I/O).
    pub(crate) release: bool,
    pub(crate) warnings: Vec<CompileWarning>,
    pub(crate) gc: GcHeap,
    pub(crate) phases: HashMap<String, Vec<String>>,
    /// `serial phase` declarations in declaration order: (phase name,
    /// member system names resolved at declaration time, while the module
    /// scope is live). The index is the serial-group id stamped onto member
    /// systems at the end of `compile()` (dogfood feature seq 83).
    pub(crate) serial_phases: Vec<(String, Vec<String>)>,
    /// Declared schema versions (`component X v2` / `resource Y v3`),
    /// resolved name → version; only nonzero versions are recorded
    /// (dogfood feature seq 69 IDEA 03).
    pub(crate) component_versions: HashMap<String, u32>,
    /// System names known before any body compiles (declaration-metadata
    /// pre-pass). `self.systems` only gains an entry once the system's body
    /// has compiled, which is too late for a hoisted fn body that calls a
    /// system declared later in the file — the call would compile as a plain
    /// global call and trap on `nil` at runtime.
    pub(crate) declared_systems: std::collections::HashSet<String>,
    pub(crate) checker_output: Option<CheckerOutput>,
    /// Pipeline loop-fusion is only stack-safe where the operand stack
    /// is empty (statement roots). Granted by the statement compiler,
    /// consumed by the immediate expression; nested expressions never
    /// see it. The vectorized path uses a global accumulator and is therefore
    /// disabled while causal lowering is active.
    pub(crate) allow_pipe_fusion: bool,
    /// Function bodies declared as laws/resolvers are compiled outside the
    /// lexical `settle` block that eventually invokes them.  This depth keeps
    /// causal lowering conservative in those bodies as well: no optimizer may
    /// emit an opcode that performs in-place heap/interior mutation.
    pub(crate) causal_lowering_depth: usize,
}