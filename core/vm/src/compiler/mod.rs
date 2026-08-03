mod causal;
mod decl;
mod egraph;
mod emit;
mod escape;
mod expr;
mod layout_analysis;
mod materialization;
mod pipeline;
mod stmt;

use std::collections::HashMap;

use crate::ast::*;
use crate::gc::GcHeap;
use crate::opcode::{Chunk, Op};
use crate::types::{
    CheckerOutput, ComponentType, Effect, EffectSet, ForIterKind, ResourceType, SumTypeDef,
};
use crate::value::{Builtin, FnValue, Value};

#[derive(Debug)]
pub struct CompileError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug)]
pub struct CompileWarning {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

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

pub struct CompileResult {
    pub chunks: Vec<Chunk>,
    pub systems: Vec<SystemChunkInfo>,
    pub handlers: Vec<HandlerChunkInfo>,
    pub migrations: Vec<MigrationChunkInfo>,
    pub state_machines: Vec<StateMachineInfo>,
    pub intents: Vec<IntentChunkInfo>,
    pub resolvers: Vec<ResolverChunkInfo>,
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
    pub warnings: Vec<CompileWarning>,
    /// Heap allocations for constants embedded in chunks (merged into VM on load).
    pub gc: GcHeap,
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

impl Compiler {
    fn component_fields_as_defaults(fields: &[FieldDef]) -> Vec<(String, Option<TypeExpr>, Expr)> {
        fields
            .iter()
            .map(|field| {
                (
                    field.name.clone(),
                    field.type_annotation.clone(),
                    field.default_value.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn should_optimize_egraph(&self, fn_name: &str) -> bool {
        let Some(output) = &self.checker_output else {
            return false;
        };

        let canonical_name = self.resolve_canonical_name(fn_name);
        [fn_name, canonical_name.as_str()].iter().any(|name| {
            output.functions.get(*name).is_some_and(|sig| {
                matches!(
                    &sig.effects,
                    EffectSet::Restricted(set)
                        if set.contains(&Effect::ECS) || set.contains(&Effect::ReadECS)
                )
            })
        })
    }

    pub fn new() -> Self {
        let main_scope = FnScope {
            chunk: Chunk::new("main"),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            settlement_depth: 0,
            loop_contexts: Vec::new(),
            last_get_local: HashMap::new(),
            unique_locals: std::collections::HashSet::new(),
            prev_instr_start: usize::MAX,
            label_high_water: 0,
        };
        let mut global_slots = HashMap::new();
        let mut global_names = Vec::new();
        for builtin in Builtin::ALL {
            let name = builtin.name().to_string();
            let slot = global_names.len() as u16;
            global_slots.insert(name.clone(), slot);
            global_names.push(name);
        }
        Self {
            functions: vec![main_scope],
            component_types: HashMap::new(),
            resource_types: HashMap::new(),
            chunks: Vec::new(),
            systems: Vec::new(),
            handlers: Vec::new(),
            migrations: Vec::new(),
            state_machines: Vec::new(),
            intent_types: HashMap::new(),
            resolvers: Vec::new(),
            temp_counter: 0,
            global_mutability: HashMap::new(),
            for_iter_kinds: HashMap::new(),
            checker_components: HashMap::new(),
            checker_resources: HashMap::new(),
            checker_sum_types: HashMap::new(),
            type_redirects: HashMap::new(),
            variant_shorthand: std::collections::HashSet::new(),
            spread_lengths: HashMap::new(),
            global_slots,
            global_names,
            module_aliases: HashMap::new(),
            alias_decls: HashMap::new(),
            current_alias_scope: None,
            file_private_scopes: HashMap::new(),
            current_file_scope: None,
            features: Vec::new(),
            release: false,
            warnings: Vec::new(),
            gc: GcHeap::new(),
            phases: HashMap::new(),
            serial_phases: Vec::new(),
            component_versions: HashMap::new(),
            declared_systems: std::collections::HashSet::new(),
            checker_output: None,
            allow_pipe_fusion: false,
            causal_lowering_depth: 0,
        }
    }

    pub(crate) fn in_causal_region(&self) -> bool {
        self.functions
            .last()
            .is_some_and(|scope| scope.settlement_depth > 0)
            || self.causal_lowering_depth > 0
    }

    pub fn with_release(mut self, release: bool) -> Self {
        self.release = release;
        self
    }

    pub fn with_features(mut self, features: Vec<String>) -> Self {
        self.features = features;
        self
    }

    pub fn with_aliases(mut self, aliases: HashMap<String, Vec<Decl>>) -> Self {
        for (alias_name, decls) in &aliases {
            let mut pub_map = HashMap::new();
            for d in decls {
                if let Some(name) = Self::decl_name_static(d) {
                    if Self::decl_is_pub_static(d) {
                        let mangled = format!("__mod_{}__{}", alias_name, name);
                        pub_map.insert(name.to_string(), mangled);
                    }
                }
            }
            self.module_aliases.insert(alias_name.clone(), pub_map);
        }
        self.alias_decls = aliases;
        self
    }

    fn decl_name_static(decl: &Decl) -> Option<&str> {
        match decl {
            Decl::Component(c) => Some(&c.name),
            Decl::Resource(r) => Some(&r.name),
            Decl::Struct(s) => Some(&s.name),
            Decl::Intent(i) => Some(&i.name),
            Decl::Law(l) => Some(&l.name),
            Decl::Resolver(r) => Some(&r.name),
            Decl::Entity(e) => Some(&e.name),
            Decl::State(s) => Some(&s.name),
            Decl::System(s) => Some(&s.name),
            Decl::Event(e) => Some(&e.name),
            Decl::Phase(p) => Some(&p.name),
            Decl::Fn(f) => Some(&f.name),
            Decl::Type(t) => Some(&t.name),
            Decl::TypeAlias(a) => Some(&a.name),
            _ => None,
        }
    }

    fn decl_is_pub_static(decl: &Decl) -> bool {
        match decl {
            Decl::Component(c) => c.is_pub,
            Decl::Resource(r) => r.is_pub,
            Decl::Struct(s) => s.is_pub,
            Decl::Intent(i) => i.is_pub,
            Decl::Law(l) => l.is_pub,
            Decl::Resolver(r) => r.is_pub,
            Decl::Entity(e) => e.is_pub,
            Decl::State(s) => s.is_pub,
            Decl::System(s) => s.is_pub,
            Decl::Event(e) => e.is_pub,
            Decl::Phase(p) => p.is_pub,
            Decl::Fn(f) => f.is_pub,
            Decl::Type(t) => t.is_pub,
            Decl::TypeAlias(a) => a.is_pub,
            Decl::Stmt(Stmt::Let(l)) => l.is_pub,
            _ => false,
        }
    }

    pub(crate) fn resolve_canonical_name(&self, name: &str) -> String {
        let mut current = name.to_string();
        if let Some(dot_pos) = current.find('.') {
            let alias = &current[..dot_pos];
            let member = &current[dot_pos + 1..];
            if let Some(alias_map) = self.module_aliases.get(alias) {
                if let Some(resolved) = alias_map.get(member) {
                    current = resolved.clone();
                }
            }
        } else if let Some(resolved) = self.resolve_current_alias(&current) {
            current = resolved;
        }
        while let Some(canonical) = self.type_redirects.get(&current) {
            current = canonical.clone();
        }
        current
    }

    pub(crate) fn resolve_alias_member(&self, alias: &str, member: &str) -> Option<String> {
        self.module_aliases
            .get(alias)
            .and_then(|m| m.get(member).cloned())
    }

    pub(crate) fn resolve_current_alias(&self, name: &str) -> Option<String> {
        if let Some(res) = self
            .current_alias_scope
            .as_ref()
            .and_then(|m| m.get(name).cloned())
        {
            return Some(res);
        }
        if let Some(res) = self
            .current_file_scope
            .as_ref()
            .and_then(|m| m.get(name).cloned())
        {
            return Some(res);
        }
        None
    }

    pub fn with_for_iter_kinds(mut self, hints: HashMap<NodeId, ForIterKind>) -> Self {
        self.for_iter_kinds = hints;
        self
    }

    pub fn with_checker_output(mut self, output: CheckerOutput) -> Self {
        self.checker_output = Some(output.clone());
        self.for_iter_kinds = output.for_iter_kinds;
        self.checker_components = output.components;
        self.checker_resources = output.resources;
        for (name, rs) in &self.checker_resources {
            self.checker_components.insert(
                name.clone(),
                ComponentType {
                    name: rs.name.clone(),
                    fields: rs.fields.clone(),
                    is_pub: rs.is_pub,
                    file_id: rs.file_id,
                    indexed_fields: std::collections::HashSet::new(),
                },
            );
        }
        for (name, st) in output.structs {
            self.checker_components.insert(
                name,
                ComponentType {
                    name: st.name,
                    fields: st.fields,
                    is_pub: st.is_pub,
                    file_id: st.file_id,
                    indexed_fields: std::collections::HashSet::new(),
                },
            );
        }
        self.checker_sum_types = output.sum_types;
        self.type_redirects = output.type_redirects;
        self.variant_shorthand = output.variant_shorthand;
        self.spread_lengths = output.spread_lengths;
        self
    }

    pub(crate) fn ensure_global_slot(&mut self, name: &str) -> u16 {
        let mut resolved = None;
        if let Some(ref scope) = self.current_alias_scope {
            resolved = scope.get(name).map(|s| s.as_str());
        }
        if resolved.is_none() {
            if let Some(ref scope) = self.current_file_scope {
                resolved = scope.get(name).map(|s| s.as_str());
            }
        }
        let effective = resolved.unwrap_or(name);
        if let Some(&slot) = self.global_slots.get(effective) {
            return slot;
        }
        let slot = self.global_names.len() as u16;
        self.global_slots.insert(effective.to_owned(), slot);
        self.global_names.push(effective.to_owned());
        slot
    }

    pub(crate) fn is_system(&self, name: &str) -> bool {
        // declared_systems covers the current program's declarations
        // position-independently; self.systems additionally holds systems
        // from alias modules (compiled before the main declaration loop).
        self.declared_systems.contains(name) || self.systems.iter().any(|s| s.name == name)
    }

    pub(crate) fn component_field_order(&self, comp_name: &str) -> Option<Vec<String>> {
        self.checker_components
            .get(comp_name)
            .map(|ct| ct.fields.iter().map(|(n, _)| n.clone()).collect())
    }

    fn compile_alias_decls(&mut self) -> Result<(), CompileError> {
        let alias_decls = std::mem::take(&mut self.alias_decls);
        for (alias_name, decls) in &alias_decls {
            let mut all_names: HashMap<String, String> = HashMap::new();
            for d in decls {
                if let Some(name) = Self::decl_name_static(d) {
                    all_names.insert(name.to_string(), format!("__mod_{}__{}", alias_name, name));
                }
            }
            self.current_alias_scope = Some(all_names.clone());
            for d in decls {
                match d {
                    Decl::Component(c) => {
                        self.component_types.insert(
                            all_names
                                .get(&c.name)
                                .cloned()
                                .unwrap_or_else(|| c.name.clone()),
                            Self::component_fields_as_defaults(&c.fields),
                        );
                    }
                    Decl::Resource(r) => {
                        self.resource_types.insert(
                            all_names
                                .get(&r.name)
                                .cloned()
                                .unwrap_or_else(|| r.name.clone()),
                            Self::component_fields_as_defaults(&r.fields),
                        );
                    }
                    Decl::Struct(s) => {
                        self.component_types.insert(
                            all_names
                                .get(&s.name)
                                .cloned()
                                .unwrap_or_else(|| s.name.clone()),
                            Self::component_fields_as_defaults(&s.fields),
                        );
                    }
                    _ => {}
                }
            }
            for d in decls {
                self.compile_decl(d)?;
            }
            self.current_alias_scope = None;
        }
        self.alias_decls = alias_decls;
        Ok(())
    }

    pub(crate) fn new_fn_scope(name: &str) -> FnScope {
        FnScope {
            chunk: Chunk::new(name),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 1,
            settlement_depth: 0,
            loop_contexts: Vec::new(),
            last_get_local: HashMap::new(),
            unique_locals: std::collections::HashSet::new(),
            prev_instr_start: usize::MAX,
            label_high_water: 0,
        }
    }

    pub(crate) fn fresh_name(&mut self, prefix: &str) -> String {
        self.temp_counter += 1;
        format!("__{}{}", prefix, self.temp_counter)
    }

    pub fn compile(mut self, program: &Program) -> Result<CompileResult, CompileError> {
        let mut file_private_scopes: HashMap<u32, HashMap<String, String>> = HashMap::new();
        for decl in &program.declarations {
            if let Some(span) = decl.span() {
                if let Some(file_id) = span.file {
                    if file_id.0 != 0 && !Self::decl_is_pub_static(decl) {
                        if let Some(name) = Self::decl_name_static(decl) {
                            let mangled = format!("__priv_{}__{}", file_id.0, name);
                            file_private_scopes
                                .entry(file_id.0)
                                .or_default()
                                .insert(name.to_string(), mangled);
                        }
                    }
                }
            }
        }
        self.file_private_scopes = file_private_scopes;

        for feature in &self.features.clone() {
            let name = format!("FEATURE_{}", feature.to_uppercase());
            let slot = self.ensure_global_slot(&name);
            self.emit_constant(Value::from_bool(true), 0);
            self.emit_op(Op::DefGlobal, 0);
            self.emit_u16(slot, 0);
        }

        let has_main_fn = program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "main"));

        for decl in &program.declarations {
            let mut resolved_name = None;
            if let Some(span) = decl.span() {
                if let Some(file_id) = span.file {
                    if let Some(scope) = self.file_private_scopes.get(&file_id.0) {
                        if let Some(name) = Self::decl_name_static(decl) {
                            if let Some(mangled) = scope.get(name) {
                                resolved_name = Some(mangled.clone());
                            }
                        }
                    }
                }
            }

            match decl {
                Decl::Component(c) => {
                    self.component_types.insert(
                        resolved_name.unwrap_or_else(|| c.name.clone()),
                        Self::component_fields_as_defaults(&c.fields),
                    );
                }
                Decl::Resource(r) => {
                    self.resource_types.insert(
                        resolved_name.unwrap_or_else(|| r.name.clone()),
                        Self::component_fields_as_defaults(&r.fields),
                    );
                }
                Decl::Struct(s) => {
                    self.component_types.insert(
                        resolved_name.unwrap_or_else(|| s.name.clone()),
                        Self::component_fields_as_defaults(&s.fields),
                    );
                }
                _ => {}
            }
        }

        self.compile_alias_decls()?;

        // Declaration-metadata pre-pass, then hoist top-level `fn`
        // definitions ahead of every other declaration. The checker places
        // every top-level fn in scope everywhere and the docs promise
        // forward references work; without hoisting the binding only exists
        // once execution reaches the `fn` statement, so an earlier call
        // trapped on `nil`. Hoisting is observation-free: a top-level fn
        // decl only emits DefGlobal of a constant fn value (top-level fns
        // capture no upvalues — main's top-level lets are globals, not
        // locals), so entity-spawn order and statement effects are
        // unchanged. Compiling fn bodies first is only correct because the
        // pre-pass has already registered every later declaration's
        // compile-time facts: which names are systems, which globals are
        // immutable, which names are phases.
        for decl in &program.declarations {
            self.predeclare_decl_metadata(decl);
        }
        for decl in &program.declarations {
            if matches!(decl, Decl::Fn(_) | Decl::Law(_) | Decl::Resolver(_)) {
                self.compile_decl(decl)?;
            }
        }
        for decl in &program.declarations {
            if !matches!(decl, Decl::Fn(_) | Decl::Law(_) | Decl::Resolver(_)) {
                self.compile_decl(decl)?;
            }
        }

        let layout_analysis = if let Some(output) = &self.checker_output {
            layout_analysis::LayoutAnalysis::analyze(output, |name| {
                self.resolve_canonical_name(name)
            })
        } else {
            layout_analysis::LayoutAnalysis::default()
        };

        if has_main_fn {
            let line = 0;
            let main_slot = self.ensure_global_slot("main");
            self.emit_op(Op::GetGlobal, line);
            self.emit_u16(main_slot, line);
            self.emit_op(Op::Call, line);
            self.emit_byte(0, line);
            self.emit_op(Op::PopCheckErr, line);
        }

        self.emit_op(Op::Halt, 0);
        let main_chunk = self.functions.pop().unwrap().chunk;
        let mut result = vec![main_chunk];
        result.extend(self.chunks);

        let mut component_layouts = HashMap::new();
        let mut component_field_types = HashMap::new();
        let mut indexed_component_fields = HashMap::new();
        let mut transient_resources = std::collections::HashSet::new();
        for (name, (_, fields)) in &self.intent_types {
            component_layouts.insert(Self::intent_runtime_type(name), fields.clone());
        }
        for (name, ct) in &self.checker_components {
            component_layouts.insert(
                name.clone(),
                ct.fields
                    .iter()
                    .map(|(n, _)| n.clone())
                    .collect::<Vec<String>>(),
            );
            component_field_types.insert(name.clone(), ct.fields.clone());
            indexed_component_fields.insert(
                name.clone(),
                ct.indexed_fields.iter().cloned().collect::<Vec<String>>(),
            );
        }
        for decl in &program.declarations {
            let mut resolved_name = None;
            if let Some(span) = decl.span() {
                if let Some(file_id) = span.file {
                    if let Some(scope) = self.file_private_scopes.get(&file_id.0) {
                        if let Some(name) = Self::decl_name_static(decl) {
                            if let Some(mangled) = scope.get(name) {
                                resolved_name = Some(mangled.clone());
                            }
                        }
                    }
                }
            }

            match decl {
                Decl::Event(e) => {
                    component_layouts.insert(
                        resolved_name.unwrap_or_else(|| e.name.clone()),
                        e.fields.iter().map(|(n, _)| n.clone()).collect(),
                    );
                }
                Decl::Component(c) => {
                    let name = resolved_name.unwrap_or_else(|| c.name.clone());
                    component_layouts.insert(
                        name.clone(),
                        c.fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect::<Vec<String>>(),
                    );
                    // `indexed` declarations must survive checker-less
                    // compiles (replay of embedded trace source) — the AST
                    // is the source of truth, the checker copy is a cache.
                    indexed_component_fields
                        .entry(name)
                        .or_insert_with(|| c.indexed_fields.clone());
                }
                Decl::Resource(r) => {
                    let name = resolved_name.unwrap_or_else(|| r.name.clone());
                    if r.transient {
                        transient_resources.insert(name.clone());
                    }
                    component_layouts.insert(
                        name,
                        r.fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect::<Vec<String>>(),
                    );
                }
                Decl::Struct(s) => {
                    component_layouts.insert(
                        resolved_name.unwrap_or_else(|| s.name.clone()),
                        s.fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect::<Vec<String>>(),
                    );
                }
                _ => {}
            }
        }
        for (alias_name, decls) in &self.alias_decls {
            for decl in decls {
                match decl {
                    Decl::Event(e) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, e.name),
                            e.fields.iter().map(|(n, _)| n.clone()).collect(),
                        );
                    }
                    Decl::Component(c) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, c.name),
                            c.fields
                                .iter()
                                .map(|field| field.name.clone())
                                .collect::<Vec<String>>(),
                        );
                    }
                    Decl::Resource(r) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, r.name),
                            r.fields
                                .iter()
                                .map(|field| field.name.clone())
                                .collect::<Vec<String>>(),
                        );
                    }
                    Decl::Struct(s) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, s.name),
                            s.fields
                                .iter()
                                .map(|field| field.name.clone())
                                .collect::<Vec<String>>(),
                        );
                    }
                    _ => {}
                }
            }
        }
        let mut variant_layouts = HashMap::new();
        for (type_name, stdef) in &self.checker_sum_types {
            for variant in &stdef.variants {
                let key = (type_name.clone(), variant.name.clone());
                variant_layouts
                    .insert(key, variant.fields.iter().map(|(n, _)| n.clone()).collect());
            }
        }

        let materialization_plan =
            materialization::MaterializationPlan::from_layout_analysis(&layout_analysis);

        // Stamp serial-phase membership onto the compiled systems (dogfood
        // feature seq 83). Done here — not at phase-compile time — because a
        // `serial phase` may be declared before or after its member systems.
        // A system in several serial phases keeps the first group; groups
        // only ever ADD conflicts, so the batches stay correct either way.
        for (gid, (_phase_name, members)) in self.serial_phases.iter().enumerate() {
            for sys in &mut self.systems {
                if members.contains(&sys.name) && sys.serial_group.is_none() {
                    sys.serial_group = Some(gid as u32);
                }
            }
        }

        Ok(CompileResult {
            chunks: result,
            systems: self.systems,
            handlers: self.handlers,
            migrations: self.migrations,
            state_machines: self.state_machines,
            intents: self
                .intent_types
                .iter()
                .map(|(name, (key_field, fields))| IntentChunkInfo {
                    name: name.clone(),
                    key_field: key_field.clone(),
                    fields: fields.clone(),
                })
                .collect(),
            resolvers: self.resolvers,
            layout_analysis,
            materialization_plan,
            component_layouts,
            component_field_types,
            indexed_component_fields,
            transient_resources,
            component_versions: std::mem::take(&mut self.component_versions),
            variant_layouts,
            global_names: self.global_names,
            warnings: std::mem::take(&mut self.warnings),
            gc: std::mem::take(&mut self.gc),
        })
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
