

pub(crate) fn is_cross_file(decl_file: Option<FileId>, use_file: Option<FileId>) -> bool {
    matches!((decl_file, use_file), (Some(d), Some(u)) if d != u)
}

fn decl_name(decl: &Decl) -> Option<&str> {
    match decl {
        Decl::Component(c) => Some(&c.name),
        Decl::Resource(r) => Some(&r.name),
        Decl::Struct(s) => Some(&s.name),
        Decl::Intent(i) => Some(&i.name),
        Decl::Law(l) => Some(&l.name),
        Decl::Resolver(r) => Some(&r.name),
        Decl::Constraint(c) => Some(&c.name),
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
        Decl::Intent(i) => i.is_pub,
        Decl::Law(l) => l.is_pub,
        Decl::Resolver(r) => r.is_pub,
        Decl::Constraint(c) => c.is_pub,
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

fn register_alias_local_names(names: &mut HashMap<String, String>, alias_name: &str, decl: &Decl) {
    if let Some(name) = decl_name(decl) {
        names.insert(name.to_string(), format!("__mod_{}__{}", alias_name, name));
        return;
    }
    match decl {
        Decl::Stmt(Stmt::Let(binding)) => {
            for name in &binding.names {
                names.insert(name.clone(), format!("__mod_{}__{}", alias_name, name));
            }
        }
        Decl::Stmt(Stmt::LetElse(binding)) => {
            if let Some(name) = binding.primary_binding_name() {
                names.insert(name.clone(), format!("__mod_{}__{}", alias_name, name));
            }
        }
        _ => {}
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
    /// Number of lexical `settle` boundaries containing this scope.
    pub(crate) settlement_depth: usize,
    /// Set only on the scope that introduces a loop. `break` and `continue`
    /// target the innermost such scope and may not cross a settlement depth.
    pub(crate) loop_target_settlement_depth: Option<usize>,
    pub(crate) effect_context: EffectSet,
    pub(crate) causal_context: CausalContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CausalContext {
    None,
    Settlement,
    Law(String),
    Resolver {
        name: String,
        intent: String,
        key_param: String,
    },
    Constraint {
        name: String,
        attached_component: String,
        subject_param: String,
        proposed_param: String,
        watches: HashSet<String>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct IntentType {
    pub(crate) name: String,
    pub(crate) fields: Vec<(String, Ty)>,
    pub(crate) file_id: Option<FileId>,
}

#[derive(Debug, Clone)]
pub(crate) struct LawType {
    pub(crate) params: Vec<Ty>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolverOwner {
    pub(crate) name: String,
    pub(crate) span: Span,
}
#[derive(Debug, Clone)]
pub(crate) struct ConstraintType {
    pub(crate) attached_component: String,
    pub(crate) watches: HashSet<String>,
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
            settlement_depth: 0,
            loop_target_settlement_depth: None,
            effect_context: EffectSet::unrestricted(),
            causal_context: CausalContext::None,
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
    pub(crate) intents: HashMap<String, IntentType>,
    pub(crate) laws: HashMap<String, LawType>,
    pub(crate) resolvers: HashMap<String, Vec<ResolverOwner>>,
    pub(crate) constraints: HashMap<String, ConstraintType>,
    pub(crate) proposed_intents: HashSet<String>,
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
    /// usable default — these may be omitted from literals, the compiler
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
    /// Active during alias body checking: maps original names → mangled names
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