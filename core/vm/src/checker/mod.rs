mod declarations;
mod diagnostics;
mod reachability;
mod resolve;
mod scope;
mod typeck;
use crate::ast::*;
use crate::builtins;
use crate::simulate_syntax::{self, SystemsListForm};
use crate::types::*;
use crate::visitor::{walk_call_expr, walk_schedule_stmt, AstVisitor};
use std::collections::{HashMap, HashSet};

pub(crate) fn is_cross_file(decl_file: Option<FileId>, use_file: Option<FileId>) -> bool {
    matches!((decl_file, use_file), (Some(d), Some(u)) if d != u)
}

fn decl_name(decl: &Decl) -> Option<&str> {
    match decl {
        Decl::Component(c) => Some(&c.name),
        Decl::Resource(r) => Some(&r.name),
        Decl::Struct(s) => Some(&s.name),
        Decl::Entity(e) => Some(&e.name),
        Decl::State(s) => Some(&s.name),
        Decl::System(s) => Some(&s.name),
        Decl::Event(e) => Some(&e.name),
        Decl::Phase(p) => Some(&p.name),
        Decl::Fn(f) => Some(&f.name),
        Decl::Type(t) => Some(&t.name),
        Decl::TypeAlias(a) => Some(&a.name),
        // Top-level lets are deliberately absent: pub lets export through
        // bare `use` (merged namespace), not module aliases — see the
        // targeted diagnostic in the alias-member check.
        _ => None,
    }
}

fn decl_is_pub(decl: &Decl) -> bool {
    match decl {
        Decl::Component(c) => c.is_pub,
        Decl::Resource(r) => r.is_pub,
        Decl::Struct(s) => s.is_pub,
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

pub(super) fn format_type_expr(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Named(s) => s.clone(),
        TypeExpr::Union(variants) => variants
            .iter()
            .map(format_type_expr)
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::Generic(name, params) => {
            let ps: Vec<String> = params.iter().map(format_type_expr).collect();
            format!("{}<{}>", name, ps.join(", "))
        }
        TypeExpr::FnType(args, ret, purity) => {
            let ps: Vec<String> = args.iter().map(format_type_expr).collect();
            let prefix = match purity {
                FnTypePurity::Pure => "pure ",
                FnTypePurity::Readonly => "readonly ",
                FnTypePurity::Default => "",
            };
            format!(
                "{}fn({}) -> {}",
                prefix,
                ps.join(", "),
                format_type_expr(ret)
            )
        }
        TypeExpr::Tuple(elems) => {
            let ps: Vec<String> = elems.iter().map(format_type_expr).collect();
            format!("({})", ps.join(", "))
        }
    }
}
#[derive(Debug, Clone)]
pub struct TypeError {
    pub line: u32,
    pub col: u32,
    pub file: Option<FileId>,
    pub message: String,
    pub hint: Option<String>,
}
#[derive(Debug, Clone)]
pub struct TypeWarning {
    pub line: u32,
    pub col: u32,
    pub file: Option<FileId>,
    pub message: String,
    pub hint: Option<String>,
}
impl std::fmt::Display for TypeWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[line {}:{}] {}", self.line, self.col, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {}", hint)?;
        }
        Ok(())
    }
}
impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[line {}:{}] {}", self.line, self.col, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {}", hint)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct Binding {
    pub(crate) ty: Ty,
    pub(crate) mutable: bool,
    pub(crate) is_unique: bool,
    pub(crate) defined_at: Span,
    pub(crate) is_pub: bool,
    /// When true, warn on pop_scope if the binding was never read (let bindings only).
    pub(crate) track_unused: bool,
    pub(crate) read: bool,
}
#[derive(Debug, Clone)]
pub(crate) struct Scope {
    pub(crate) bindings: HashMap<String, Binding>,
    pub(crate) in_system: Option<String>,
    pub(crate) in_pipeline: bool,
    pub(crate) in_async: bool,
    pub(crate) in_loop: bool,
    pub(crate) effect_context: EffectSet,
}
#[derive(Debug, Clone)]
pub(crate) struct FunctionSig {
    pub(crate) type_params: Vec<String>,
    pub(crate) params: Vec<Ty>,
    pub(crate) ret: Ty,
    pub(crate) is_pure: bool,
    pub(crate) effects: EffectSet,
}
impl Scope {
    fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            in_system: None,
            in_pipeline: false,
            in_async: false,
            in_loop: false,
            effect_context: EffectSet::unrestricted(),
        }
    }
}
pub struct Checker {
    pub(crate) scopes: Vec<Scope>,
    /// When set, the purity-breach finder tolerates read-only ECS builtins
    /// (`get`, `res`, `has`, …) and `readonly fn` calls: the mode used for
    /// `query_where` predicates and `query_map` mappers, whose real contract
    /// is "no writes, IO, or events", not full purity. A `Cell` because the
    /// finder runs on `&self`.
    pub(crate) purity_allow_read_ecs: std::cell::Cell<bool>,
    pub(crate) components: HashMap<String, ComponentType>,
    pub(crate) resources: HashMap<String, ResourceType>,
    pub(crate) structs: HashMap<String, StructType>,
    pub(crate) state_machines: HashMap<String, StateMachineType>,
    pub(crate) systems: HashMap<String, SystemType>,
    pub(crate) phases: HashMap<String, Vec<String>>,
    pub(crate) system_deps: HashMap<String, (Vec<String>, Vec<String>, Span)>,
    pub(crate) events: HashMap<String, EventType>,
    pub(crate) functions: HashMap<String, FunctionSig>,
    /// Declared parameter names per user function, kept aside so arity
    /// errors can print a teaching signature (`fn heal(target: entity, ...)`).
    pub(crate) fn_param_names: HashMap<String, Vec<String>>,
    pub(crate) sum_types: HashMap<String, SumTypeDef>,
    pub(crate) type_aliases: HashMap<String, TypeScheme>,
    pub(crate) errors: Vec<TypeError>,
    pub(crate) warnings: Vec<TypeWarning>,
    pub(crate) system_params: HashMap<String, (String, bool)>,
    pub(crate) current_fn_name: Option<String>,
    pub(crate) current_fn_returns: Vec<(Ty, Span)>,
    pub(crate) for_iter_kinds: HashMap<NodeId, ForIterKind>,
    pub(crate) subst: Substitution,
    pub(crate) next_var: u32,
    pub(crate) options: CheckerOptions,
    pub(crate) variant_shorthand: std::collections::HashSet<(String, String)>,
    /// Fields (per component/resource/struct) whose declaration carries a
    /// usable default â€” these may be omitted from literals, the compiler
    /// fills them from the declaration. `Tag { }` is `Tag` with all defaults.
    pub(crate) defaultable_fields: HashMap<String, HashSet<String>>,
    pub(crate) suppress_mixed_list_warnings: usize,
    pub(crate) type_param_scopes: Vec<HashSet<String>>,
    pub(crate) spread_lengths: HashMap<Span, usize>,
    /// For each function whose purity inference failed, stores a human-readable
    /// explanation of *why* (e.g. "calls impure builtin 'set'" or "calls non-pure
    /// function 'helper' which calls impure builtin 'emit'").
    pub(crate) purity_breach_reasons: HashMap<String, String>,
    pub(crate) module_aliases: HashMap<String, HashMap<String, String>>,
    pub(crate) type_redirects: HashMap<String, String>,
    pub(crate) alias_decls: HashMap<String, Vec<Decl>>,
    /// Active during alias body checking: maps original names â†’ mangled names
    pub(crate) current_alias_redirects: Option<HashMap<String, String>>,
    /// `Some(name)` while type-checking an assignment RHS for `name = ...`.
    pub(crate) current_assign_target: Option<String>,
    /// Stack of scope depths that mark anonymous-function boundaries.
    pub(crate) anon_fn_scope_bases: Vec<usize>,
    /// When true, the simulation-breach walk tolerates `rand_*` builtins.
    /// Used to compute the lenient breach for `simulate_par`, whose explicit
    /// per-fork seeding makes guest randomness deterministic.
    pub(crate) sim_breach_allow_rand: bool,
    /// Handler bodies by event name — the transitive walk behind
    /// allowing `emit` in simulated systems: a system may emit if every
    /// reachable handler is itself simulation-safe.
    pub(crate) event_handler_blocks: std::collections::HashMap<String, Vec<crate::ast::Block>>,
    /// Top-level immutable `let NAME = [system::A, …]` bindings, mapped to
    /// their static system-ref items. A reference to such a name in a
    /// `simulate`/`simulate_par` schedule argument is const-folded to its
    /// list, so the static schedule can be shared across call sites without
    /// copy-paste while keeping every compile-time guarantee (dogfood feature
    /// seq 22). Only immutable, top-level, all-`system::…`-literal lists
    /// qualify — no dataflow analysis.
    pub(crate) system_list_consts: std::collections::HashMap<String, Vec<crate::ast::Expr>>,
}

#[derive(Debug, Clone)]
pub struct CheckerOptions {
    pub compat_v0_5_dx: bool,
    pub warn_compat: bool,
    pub strict_types: bool,
    pub features: Vec<String>,
}

impl Default for CheckerOptions {
    fn default() -> Self {
        Self {
            compat_v0_5_dx: false,
            warn_compat: true,
            strict_types: false,
            features: Vec::new(),
        }
    }
}

impl Checker {
    pub fn new() -> Self {
        Self::new_with_options(CheckerOptions::default())
    }

    pub fn new_with_options(options: CheckerOptions) -> Self {
        let mut sum_types = HashMap::new();
        sum_types.insert(
            "Option".to_string(),
            SumTypeDef {
                name: "Option".to_string(),
                is_pub: false,
                file_id: None,
                type_params: vec!["T".to_string()],
                variants: vec![
                    VariantType {
                        name: "Some".to_string(),
                        fields: vec![("value".to_string(), Ty::App("T".to_string(), vec![]))],
                    },
                    VariantType {
                        name: "None".to_string(),
                        fields: vec![],
                    },
                ],
            },
        );
        sum_types.insert(
            "Result".to_string(),
            SumTypeDef {
                name: "Result".to_string(),
                is_pub: false,
                file_id: None,
                type_params: vec!["T".to_string(), "E".to_string()],
                variants: vec![
                    VariantType {
                        name: "Ok".to_string(),
                        fields: vec![("value".to_string(), Ty::App("T".to_string(), vec![]))],
                    },
                    VariantType {
                        name: "Err".to_string(),
                        fields: vec![("message".to_string(), Ty::App("E".to_string(), vec![]))],
                    },
                ],
            },
        );
        // `merge_forks` conflicts are data, not prose: each is a `Conflict`
        // sum value carrying the subject and the diverging values, so a
        // resolution policy is a `match` in user code. Only `FieldConflict`
        // and `ResourceFieldConflict` are mechanically resolvable (via
        // `merge_forks_with`); the rest are structural refusals.
        sum_types.insert(
            "Conflict".to_string(),
            SumTypeDef {
                name: "Conflict".to_string(),
                is_pub: false,
                file_id: None,
                type_params: vec![],
                variants: vec![
                    VariantType {
                        name: "FieldConflict".to_string(),
                        fields: vec![
                            ("ent".to_string(), Ty::EntityId),
                            ("name".to_string(), Ty::Str),
                            ("comp".to_string(), Ty::Str),
                            ("field".to_string(), Ty::Str),
                            ("base".to_string(), Ty::Any),
                            ("ours".to_string(), Ty::Any),
                            ("theirs".to_string(), Ty::Any),
                        ],
                    },
                    VariantType {
                        name: "ComponentConflict".to_string(),
                        fields: vec![
                            ("ent".to_string(), Ty::EntityId),
                            ("name".to_string(), Ty::Str),
                            ("comp".to_string(), Ty::Str),
                            ("detail".to_string(), Ty::Str),
                        ],
                    },
                    VariantType {
                        name: "DespawnConflict".to_string(),
                        fields: vec![
                            ("ent".to_string(), Ty::EntityId),
                            ("name".to_string(), Ty::Str),
                            ("detail".to_string(), Ty::Str),
                        ],
                    },
                    VariantType {
                        name: "RenameConflict".to_string(),
                        fields: vec![
                            ("ent".to_string(), Ty::EntityId),
                            ("base".to_string(), Ty::Str),
                            ("ours".to_string(), Ty::Str),
                            ("theirs".to_string(), Ty::Str),
                        ],
                    },
                    VariantType {
                        name: "NameConflict".to_string(),
                        fields: vec![
                            ("name".to_string(), Ty::Str),
                            ("entities".to_string(), Ty::List(Box::new(Ty::EntityId))),
                        ],
                    },
                    VariantType {
                        name: "ResourceFieldConflict".to_string(),
                        fields: vec![
                            ("res".to_string(), Ty::Str),
                            ("field".to_string(), Ty::Str),
                            ("base".to_string(), Ty::Any),
                            ("ours".to_string(), Ty::Any),
                            ("theirs".to_string(), Ty::Any),
                        ],
                    },
                    VariantType {
                        name: "ResourceConflict".to_string(),
                        fields: vec![
                            ("res".to_string(), Ty::Str),
                            ("detail".to_string(), Ty::Str),
                        ],
                    },
                    VariantType {
                        name: "EventConflict".to_string(),
                        fields: vec![
                            ("detail".to_string(), Ty::Str),
                            ("base".to_string(), Ty::Int),
                            ("ours".to_string(), Ty::Int),
                            ("theirs".to_string(), Ty::Int),
                        ],
                    },
                ],
            },
        );
        Self {
            scopes: vec![Scope::new()],
            purity_allow_read_ecs: std::cell::Cell::new(false),
            components: HashMap::new(),
            resources: HashMap::new(),
            structs: HashMap::new(),
            state_machines: HashMap::new(),
            systems: HashMap::new(),
            phases: HashMap::new(),
            system_deps: HashMap::new(),
            events: HashMap::new(),
            functions: HashMap::new(),
            fn_param_names: HashMap::new(),
            sum_types,
            type_aliases: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            system_params: HashMap::new(),
            current_fn_name: None,
            current_fn_returns: Vec::new(),
            for_iter_kinds: HashMap::new(),
            subst: Substitution::new(),
            next_var: 0,
            options,
            variant_shorthand: std::collections::HashSet::new(),
            defaultable_fields: HashMap::new(),
            suppress_mixed_list_warnings: 0,
            type_param_scopes: Vec::new(),
            spread_lengths: HashMap::new(),
            purity_breach_reasons: HashMap::new(),
            module_aliases: HashMap::new(),
            type_redirects: HashMap::new(),
            alias_decls: HashMap::new(),
            current_alias_redirects: None,
            current_assign_target: None,
            anon_fn_scope_bases: Vec::new(),
            sim_breach_allow_rand: false,
            event_handler_blocks: std::collections::HashMap::new(),
            system_list_consts: std::collections::HashMap::new(),
        }
    }

    pub fn set_aliases(&mut self, aliases: HashMap<String, Vec<Decl>>) {
        for (alias_name, decls) in &aliases {
            let mut pub_map = HashMap::new();
            for d in decls {
                if let Some(name) = decl_name(d) {
                    if decl_is_pub(d) {
                        let mangled = format!("__mod_{}__{}", alias_name, name);
                        pub_map.insert(name.to_string(), mangled);
                    }
                }
            }
            self.module_aliases.insert(alias_name.clone(), pub_map);
        }
        self.alias_decls = aliases;
    }

    pub(crate) fn resolve_qualified_name(&self, qualified: &str) -> Option<String> {
        if let Some(dot_pos) = qualified.find('.') {
            let alias = &qualified[..dot_pos];
            let member = &qualified[dot_pos + 1..];
            if let Some(alias_map) = self.module_aliases.get(alias) {
                return alias_map.get(member).cloned();
            }
        }
        None
    }

    pub(crate) fn resolve_alias_member(&self, alias: &str, member: &str) -> Option<String> {
        self.module_aliases
            .get(alias)
            .and_then(|m| m.get(member).cloned())
    }

    pub(crate) fn resolve_canonical_name(&self, name: &str) -> String {
        let mut current = name.to_string();
        if let Some(resolved) = self.resolve_qualified_name(&current) {
            current = resolved;
        } else if let Some(redirected) = self.redirect_alias_name(&current) {
            current = redirected;
        }

        while let Some(canonical) = self.type_redirects.get(&current) {
            current = canonical.clone();
        }
        current
    }

    fn find_canonical_type_name(&self, file_id: Option<FileId>, orig_name: &str) -> Option<String> {
        let file_id = file_id?;
        for (name, def) in &self.sum_types {
            if def.file_id == Some(file_id)
                && (name == orig_name || name.ends_with(&format!("__{}", orig_name)))
            {
                return Some(name.clone());
            }
        }
        for (name, def) in &self.structs {
            if def.file_id == Some(file_id)
                && (name == orig_name || name.ends_with(&format!("__{}", orig_name)))
            {
                return Some(name.clone());
            }
        }
        for (name, def) in &self.components {
            if def.file_id == Some(file_id)
                && (name == orig_name || name.ends_with(&format!("__{}", orig_name)))
            {
                return Some(name.clone());
            }
        }
        for (name, def) in &self.resources {
            if def.file_id == Some(file_id)
                && (name == orig_name || name.ends_with(&format!("__{}", orig_name)))
            {
                return Some(name.clone());
            }
        }
        for (name, def) in &self.events {
            if def.file_id == Some(file_id)
                && (name == orig_name || name.ends_with(&format!("__{}", orig_name)))
            {
                return Some(name.clone());
            }
        }
        for (name, def) in &self.type_aliases {
            if def.file_id == Some(file_id)
                && (name == orig_name || name.ends_with(&format!("__{}", orig_name)))
            {
                return Some(name.clone());
            }
        }
        None
    }

    pub fn check(&mut self, program: &Program) -> Vec<TypeError> {
        for feature in self.options.features.clone() {
            let name = format!("FEATURE_{}", feature.to_uppercase());
            self.define(&name, Ty::Bool, false, Span::default(), false, false);
        }
        self.collect_declarations(program);
        self.collect_system_list_consts(program);
        // handler bodies indexed before any system is checked: the
        // simulate() safety walk follows emits into their handlers.
        // Keyed by the raw name AND its last segment, and looked up the
        // same way — matching too many handlers is conservative (more
        // bodies vetted); missing one would let IO into a simulation.
        for decl in &program.declarations {
            if let Decl::OnHandler(h) = decl {
                self.event_handler_blocks
                    .entry(h.event_name.clone())
                    .or_default()
                    .push(h.body.clone());
                if let Some(last) = h.event_name.rsplit('.').next() {
                    if last != h.event_name {
                        self.event_handler_blocks
                            .entry(last.to_string())
                            .or_default()
                            .push(h.body.clone());
                    }
                }
            }
        }
        self.register_alias_declarations();
        // forward references resolved: re-infer purity/effects with the
        // complete function table (declaration order must not matter)
        self.refine_fn_effects(program);
        self.check_alias_bodies();
        self.check_public_reachability(program);
        for decl in &program.declarations {
            self.check_decl(decl);
        }
        self.warn_unused_systems_and_entity_ecs(program);
        self.flush_unused_top_level_lets();
        self.check_reachability(program);
        self.errors.clone()
    }

    /// Gather top-level immutable `let NAME = [system::A, …]` bindings so a
    /// reference to `NAME` in a `simulate`/`simulate_par` schedule argument
    /// const-folds to its list (dogfood feature seq 22). Only a non-empty,
    /// single-name, non-mutable binding whose value is a list literal of
    /// `system::…` references qualifies — the same static shape an inline
    /// literal must have, so no dataflow analysis is introduced.
    fn collect_system_list_consts(&mut self, program: &Program) {
        use crate::simulate_syntax::{classify_systems_argument, SystemsListForm};
        for decl in &program.declarations {
            if let Decl::Stmt(Stmt::Let(l)) = decl {
                if l.mutable || l.tuple_destructure || l.names.len() != 1 {
                    continue;
                }
                if let SystemsListForm::StaticSchedule(items) = classify_systems_argument(&l.value)
                {
                    if !items.is_empty() {
                        self.system_list_consts
                            .insert(l.names[0].clone(), items.to_vec());
                    }
                }
            }
        }
    }

    fn register_alias_declarations(&mut self) {
        let alias_decls = std::mem::take(&mut self.alias_decls);
        for (alias_name, decls) in &alias_decls {
            let mut all_names: HashMap<String, String> = HashMap::new();
            for d in decls {
                if let Some(name) = decl_name(d) {
                    all_names.insert(name.to_string(), format!("__mod_{}__{}", alias_name, name));
                }
            }
            self.current_alias_redirects = Some(all_names.clone());
            for d in decls {
                let orig = match decl_name(d) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let mangled = match all_names.get(&orig) {
                    Some(m) => m.clone(),
                    None => continue,
                };
                match d {
                    Decl::Component(c) => {
                        if let Some(canonical) = self.find_canonical_type_name(c.span.file, &orig) {
                            self.type_redirects.insert(mangled.clone(), canonical);
                            self.define(&mangled, Ty::Str, false, c.span.clone(), c.is_pub, false);
                        } else {
                            let mut mc = c.clone();
                            mc.name = mangled.clone();
                            self.register_component(&mc);
                            self.define(&mangled, Ty::Str, false, c.span.clone(), c.is_pub, false);
                        }
                    }
                    Decl::Resource(r) => {
                        if let Some(canonical) = self.find_canonical_type_name(r.span.file, &orig) {
                            self.type_redirects.insert(mangled.clone(), canonical);
                            self.define(&mangled, Ty::Str, false, r.span.clone(), r.is_pub, false);
                        } else {
                            let mut mr = r.clone();
                            mr.name = mangled.clone();
                            self.register_resource(&mr);
                            self.define(&mangled, Ty::Str, false, r.span.clone(), r.is_pub, false);
                        }
                    }
                    Decl::Struct(s) => {
                        if let Some(canonical) = self.find_canonical_type_name(s.span.file, &orig) {
                            self.type_redirects.insert(mangled.clone(), canonical);
                            self.define(&mangled, Ty::Str, false, s.span.clone(), s.is_pub, false);
                        } else {
                            let mut ms = s.clone();
                            ms.name = mangled.clone();
                            self.register_struct(&ms);
                            self.define(&mangled, Ty::Str, false, s.span.clone(), s.is_pub, false);
                        }
                    }
                    Decl::State(s) => {
                        if let Some(canonical) = self.find_canonical_type_name(s.span.file, &orig) {
                            self.type_redirects.insert(mangled.clone(), canonical);
                            self.define(&mangled, Ty::Any, false, s.span.clone(), s.is_pub, false);
                        } else {
                            let mut ms = s.clone();
                            ms.name = mangled.clone();
                            self.register_state_machine(&ms);
                            self.define(&mangled, Ty::Any, false, s.span.clone(), s.is_pub, false);
                        }
                    }
                    Decl::System(s) => {
                        let mut ms = s.clone();
                        ms.name = mangled.clone();
                        self.register_system(&ms);
                    }
                    Decl::Event(e) => {
                        if let Some(canonical) = self.find_canonical_type_name(e.span.file, &orig) {
                            self.type_redirects.insert(mangled.clone(), canonical);
                            self.define(&mangled, Ty::Str, false, e.span.clone(), e.is_pub, false);
                        } else {
                            let mut me = e.clone();
                            me.name = mangled.clone();
                            self.register_event(&me);
                            self.define(&mangled, Ty::Str, false, e.span.clone(), e.is_pub, false);
                        }
                    }
                    Decl::Fn(f) => {
                        let mut mf = f.clone();
                        mf.name = mangled.clone();
                        self.register_function(&mf);
                        if let Some(sig) = self.functions.get(&mangled) {
                            let fn_ty = Ty::Fn {
                                params: sig.params.clone(),
                                ret: Box::new(sig.ret.clone()),
                                purity: if sig.effects.is_pure() {
                                    FnPurity::Pure
                                } else if sig.effects.is_readonly() {
                                    FnPurity::Readonly
                                } else {
                                    FnPurity::Impure
                                },
                            };
                            self.define(&mangled, fn_ty, false, f.span.clone(), f.is_pub, false);
                        }
                    }
                    Decl::Type(t) => {
                        if let Some(canonical) = self.find_canonical_type_name(t.span.file, &orig) {
                            self.type_redirects.insert(mangled.clone(), canonical);
                            self.define(&mangled, Ty::Any, false, t.span.clone(), t.is_pub, false);
                        } else {
                            let mut mt = t.clone();
                            mt.name = mangled.clone();
                            self.register_sum_type(&mt);
                            self.define(&mangled, Ty::Any, false, t.span.clone(), t.is_pub, false);
                        }
                    }
                    Decl::TypeAlias(a) => {
                        if let Some(canonical) = self.find_canonical_type_name(a.span.file, &orig) {
                            self.type_redirects.insert(mangled.clone(), canonical);
                            self.define(&mangled, Ty::Any, false, a.span.clone(), a.is_pub, false);
                        } else {
                            let mut ma = a.clone();
                            ma.name = mangled.clone();
                            self.register_type_alias(&ma);
                            self.define(&mangled, Ty::Any, false, a.span.clone(), a.is_pub, false);
                        }
                    }
                    Decl::Entity(e) => {
                        self.define(
                            &mangled,
                            Ty::EntityId,
                            false,
                            e.span.clone(),
                            e.is_pub,
                            false,
                        );
                    }
                    _ => {}
                }
            }
            self.current_alias_redirects = None;
        }
        self.alias_decls = alias_decls;
    }

    fn check_alias_bodies(&mut self) {
        let alias_decls = std::mem::take(&mut self.alias_decls);
        for (alias_name, decls) in &alias_decls {
            let mut all_names: HashMap<String, String> = HashMap::new();
            for d in decls {
                if let Some(name) = decl_name(d) {
                    all_names.insert(name.to_string(), format!("__mod_{}__{}", alias_name, name));
                }
            }
            self.current_alias_redirects = Some(all_names.clone());
            // Top-level lets of the aliased module first: its fns/systems
            // read them (the compiler defines these globals when it compiles
            // the alias decls, so the checker must see them too). Skip names
            // already in scope — the module may also be imported bare, and
            // re-defining would fire a spurious same-scope shadow warning.
            for d in decls {
                if let Decl::Stmt(Stmt::Let(l)) = d {
                    let already_defined = l.names.first().is_some_and(|n| self.lookup(n).is_some());
                    if !already_defined {
                        self.check_decl(d);
                    }
                }
            }
            for d in decls {
                match d {
                    Decl::Fn(f) => {
                        let mut mf = f.clone();
                        mf.name = all_names
                            .get(&f.name)
                            .cloned()
                            .unwrap_or_else(|| f.name.clone());
                        self.check_decl(&Decl::Fn(mf));
                    }
                    Decl::System(s) => {
                        let mut ms = s.clone();
                        ms.name = all_names
                            .get(&s.name)
                            .cloned()
                            .unwrap_or_else(|| s.name.clone());
                        self.check_decl(&Decl::System(ms));
                    }
                    _ => {}
                }
            }
            self.current_alias_redirects = None;
        }
        self.alias_decls = alias_decls;
    }

    pub(crate) fn redirect_alias_name(&self, name: &str) -> Option<String> {
        self.current_alias_redirects
            .as_ref()
            .and_then(|m| m.get(name).cloned())
    }

    pub fn for_iter_kinds(&self) -> HashMap<NodeId, ForIterKind> {
        self.for_iter_kinds.clone()
    }
    pub fn output(&self) -> crate::types::CheckerOutput {
        crate::types::CheckerOutput {
            for_iter_kinds: self.for_iter_kinds.clone(),
            components: self.components.clone(),
            resources: self.resources.clone(),
            structs: self.structs.clone(),
            functions: self.functions.clone(),
            systems: self.systems.clone(),
            sum_types: self.sum_types.clone(),
            variant_shorthand: self.variant_shorthand.clone(),
            spread_lengths: self.spread_lengths.clone(),
            type_redirects: self.type_redirects.clone(),
        }
    }
    pub fn warnings(&self) -> Vec<TypeWarning> {
        self.warnings.clone()
    }
    pub(super) fn fresh_var(&mut self) -> Ty {
        let id = self.next_var;
        self.next_var += 1;
        Ty::Var(id)
    }
    pub(super) fn resolve_ty(&self, ty: &Ty) -> Ty {
        self.subst.resolve(ty)
    }
    pub(super) fn instantiate_sig(&mut self, sig: &FunctionSig) -> (Vec<Ty>, Ty) {
        if sig.type_params.is_empty() {
            return (sig.params.clone(), sig.ret.clone());
        }
        let mut mapping: HashMap<String, Ty> = HashMap::new();
        for tp in &sig.type_params {
            mapping.insert(tp.clone(), self.fresh_var());
        }
        let params = sig
            .params
            .iter()
            .map(|p| self.substitute_type_params(p, &mapping))
            .collect();
        let ret = self.substitute_type_params(&sig.ret, &mapping);
        (params, ret)
    }
    pub(super) fn substitute_type_params(&self, ty: &Ty, mapping: &HashMap<String, Ty>) -> Ty {
        match ty {
            Ty::SumType(name) if mapping.contains_key(name) => mapping[name].clone(),
            Ty::Component(name) if mapping.contains_key(name) => mapping[name].clone(),
            Ty::Struct(name) if mapping.contains_key(name) => mapping[name].clone(),
            Ty::List(inner) => Ty::List(Box::new(self.substitute_type_params(inner, mapping))),
            Ty::Map(key, val) => Ty::Map(
                Box::new(self.substitute_type_params(key, mapping)),
                Box::new(self.substitute_type_params(val, mapping)),
            ),
            Ty::Fn {
                params,
                ret,
                purity,
            } => Ty::Fn {
                params: params
                    .iter()
                    .map(|p| self.substitute_type_params(p, mapping))
                    .collect(),
                ret: Box::new(self.substitute_type_params(ret, mapping)),
                purity: *purity,
            },
            Ty::Task(inner) => Ty::Task(Box::new(self.substitute_type_params(inner, mapping))),
            Ty::App(name, args) => {
                if args.is_empty() && mapping.contains_key(name) {
                    return mapping[name].clone();
                }
                Ty::App(
                    name.clone(),
                    args.iter()
                        .map(|a| self.substitute_type_params(a, mapping))
                        .collect(),
                )
            }
            _ => ty.clone(),
        }
    }
    pub(super) fn substitute_type_params_with_vars(&self, ty: &Ty, vars: &[(String, Ty)]) -> Ty {
        let mapping: HashMap<String, Ty> = vars.iter().cloned().collect();
        self.substitute_type_params(ty, &mapping)
    }

    pub(super) fn push_type_param_scope(&mut self, type_params: &[String]) {
        self.type_param_scopes
            .push(type_params.iter().cloned().collect());
    }

    pub(super) fn pop_type_param_scope(&mut self) {
        let _ = self.type_param_scopes.pop();
    }

    fn warn_unused_systems_and_entity_ecs(&mut self, program: &Program) {
        if !self.errors.is_empty() {
            return;
        }

        let mut first_entity_span: Option<Span> = None;
        let mut has_any_system_decl = false;

        for decl in &program.declarations {
            match decl {
                Decl::Entity(e) => {
                    if first_entity_span.is_none() {
                        first_entity_span = Some(e.span.clone());
                    }
                }
                Decl::System(_) => {
                    has_any_system_decl = true;
                }
                _ => {}
            }
        }

        if let Some(span) = first_entity_span {
            if !has_any_system_decl {
                self.warning(
                    &span,
                    "Entity declarations found without any systems".to_string(),
                    Some(
                        "Rad ECS is most effective when logic lives in `system` blocks that query components"
                            .to_string(),
                    ),
                );
                return;
            }
        }

        let invoked = self.collect_definite_system_invocations(program);
        let unused_systems: Vec<(String, Span)> = self
            .systems
            .iter()
            .filter(|(sys_name, _)| !invoked.contains(*sys_name))
            .map(|(sys_name, sys_ty)| {
                let span = self
                    .system_deps
                    .get(sys_name)
                    .map(|(_, _, s)| s.clone())
                    .unwrap_or_else(|| Span {
                        line: 1,
                        col: 0,
                        file: sys_ty.file_id,
                    });
                (sys_name.clone(), span)
            })
            .collect();
        for (sys_name, span) in unused_systems {
            self.warning(
                &span,
                format!("System '{}' is declared but never run", sys_name),
                Some(
                    "Run it with `SystemName()`, `schedule [system::SystemName]` or `schedule [A, B]`, or list it in `simulate(fork, [system::SystemName], ticks)`"
                        .to_string(),
                ),
            );
        }
    }

    /// Systems definitely invoked: direct `Sys()`, `schedule [...]`, and static
    /// `simulate(.., [system::S, ...], ..)` list literals.
    fn collect_definite_system_invocations(&self, program: &Program) -> HashSet<String> {
        let mut set = HashSet::new();
        SystemInvocationCollector {
            checker: self,
            set: &mut set,
        }
        .visit_program(program);
        set
    }
}

/// [`AstVisitor`] that records which `system`s are definitely run (for unused-system warnings).
struct SystemInvocationCollector<'a> {
    checker: &'a Checker,
    set: &'a mut HashSet<String>,
}

impl AstVisitor for SystemInvocationCollector<'_> {
    fn visit_schedule_stmt(&mut self, stmt: &ScheduleStmt) {
        for sys in &stmt.systems {
            if let Some(phase_systems) = self.checker.phases.get(sys) {
                for ps in phase_systems {
                    let resolved = self.checker.resolve_canonical_name(ps);
                    if self.checker.systems.contains_key(&resolved) {
                        self.set.insert(resolved);
                    }
                }
            } else {
                let resolved = self.checker.resolve_canonical_name(sys);
                if self.checker.systems.contains_key(&resolved) {
                    self.set.insert(resolved);
                }
            }
        }
        walk_schedule_stmt(self, stmt);
    }

    fn visit_call_expr(&mut self, callee: &Expr, args: &[Expr], _span: &Span) {
        if simulate_syntax::is_expr_call(callee, args) {
            if let SystemsListForm::StaticSchedule(items) =
                simulate_syntax::classify_systems_argument(
                    &args[simulate_syntax::SYSTEMS_ARG_INDEX],
                )
            {
                for item in items {
                    let Expr::SystemRef(path, _) = item else {
                        continue;
                    };
                    let q = simulate_syntax::system_ref_qualified_string(path);
                    let resolved = self.checker.resolve_canonical_name(&q);
                    if self.checker.systems.contains_key(&resolved) {
                        self.set.insert(resolved);
                    }
                }
            }
        }
        if let Expr::Ident(callee_name, _) = callee {
            let resolved = self.checker.resolve_canonical_name(callee_name);
            if self.checker.systems.contains_key(&resolved) {
                self.set.insert(resolved);
            }
        }
        walk_call_expr(self, callee, args);
    }
}

impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
