use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    IO,
    ECS,
    ReadECS,
    Event,
    Async,
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Effect::IO => write!(f, "io"),
            Effect::ECS => write!(f, "ecs"),
            Effect::ReadECS => write!(f, "readonly"),
            Effect::Event => write!(f, "event"),
            Effect::Async => write!(f, "async"),
        }
    }
}

impl Effect {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "io" => Some(Effect::IO),
            "ecs" => Some(Effect::ECS),
            "readonly" => Some(Effect::ReadECS),
            "event" => Some(Effect::Event),
            "async" => Some(Effect::Async),
            _ => None,
        }
    }

    pub fn all() -> HashSet<Effect> {
        let mut s = HashSet::new();
        s.insert(Effect::IO);
        s.insert(Effect::ECS);
        s.insert(Effect::ReadECS);
        s.insert(Effect::Event);
        s.insert(Effect::Async);
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectSet {
    Unrestricted,
    Restricted(HashSet<Effect>),
}

impl EffectSet {
    pub fn pure() -> Self {
        EffectSet::Restricted(HashSet::new())
    }

    pub fn unrestricted() -> Self {
        EffectSet::Unrestricted
    }

    pub fn single(e: Effect) -> Self {
        let mut s = HashSet::new();
        s.insert(e);
        EffectSet::Restricted(s)
    }

    pub fn from_vec(effects: &[Effect]) -> Self {
        if effects.is_empty() {
            return EffectSet::pure();
        }
        EffectSet::Restricted(effects.iter().copied().collect())
    }

    pub fn allows(&self, effect: Effect) -> bool {
        match self {
            EffectSet::Unrestricted => true,
            EffectSet::Restricted(set) => set.contains(&effect),
        }
    }

    pub fn is_subset_of(&self, other: &EffectSet) -> bool {
        match (self, other) {
            (_, EffectSet::Unrestricted) => true,
            (EffectSet::Unrestricted, EffectSet::Restricted(_)) => false,
            (EffectSet::Restricted(a), EffectSet::Restricted(b)) => a.is_subset(b),
        }
    }

    pub fn is_pure(&self) -> bool {
        matches!(self, EffectSet::Restricted(s) if s.is_empty())
    }

    pub fn is_readonly(&self) -> bool {
        matches!(
            self,
            EffectSet::Restricted(s)
                if !s.is_empty() && s.iter().all(|e| matches!(e, Effect::ReadECS))
        )
    }

    pub fn forbidden_in(&self, effects: &[Effect]) -> Vec<Effect> {
        effects
            .iter()
            .filter(|e| !self.allows(**e))
            .copied()
            .collect()
    }
}

impl fmt::Display for EffectSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EffectSet::Unrestricted => write!(f, "unrestricted"),
            EffectSet::Restricted(set) if set.is_empty() => write!(f, "pure"),
            EffectSet::Restricted(set) => {
                let mut names: Vec<_> = set.iter().map(|e| format!("{}", e)).collect();
                names.sort();
                write!(f, "{}", names.join("+"))
            }
        }
    }
}

/// Purity rank of a function TYPE (`Ty::Fn`). Ordered by capability —
/// `Pure < Readonly < Impure` — so "assignable" is simply `arg <= param`:
/// a pure fn value goes anywhere, a readonly value satisfies readonly or
/// impure expectations, an impure value only impure ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FnPurity {
    Pure,
    Readonly,
    Impure,
}

impl FnPurity {
    /// Display prefix inside a fn type: `pure fn(...)`, `readonly fn(...)`.
    pub fn prefix(&self) -> &'static str {
        match self {
            FnPurity::Pure => "pure ",
            FnPurity::Readonly => "readonly ",
            FnPurity::Impure => "",
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            FnPurity::Pure => "pure",
            FnPurity::Readonly => "readonly",
            FnPurity::Impure => "impure",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Str,
    Bool,
    Nil,
    List(Box<Ty>),
    Tuple(Vec<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Component(String),
    Struct(String),
    State(String),
    Fn {
        params: Vec<Ty>,
        ret: Box<Ty>,
        purity: FnPurity,
    },
    Task(Box<Ty>),
    SumType(String),
    Event(String),
    EntityId,
    BitSet,
    WorldFork,
    /// Compile-time reference to a declared `system` (see `Expr::SystemRef`).
    SystemRef,
    Any,
    Void,
    Union(Vec<Ty>),
    Var(u32),
    App(String, Vec<Ty>),
}

impl Ty {
    pub fn is_valid_map_key(&self) -> bool {
        match self {
            Ty::Int | Ty::Str | Ty::Bool | Ty::EntityId | Ty::Any => true,
            // tuples of valid keys hash by value; floats stay excluded
            Ty::Tuple(elems) => elems.iter().all(|t| t.is_valid_map_key()),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForIterKind {
    List,
    Str,
    Map,
    Unknown,
}

impl Ty {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float)
    }

    pub fn assignable_from(&self, other: &Ty) -> bool {
        if self == other || *other == Ty::Any || *self == Ty::Any {
            return true;
        }
        if *other == Ty::Void {
            return true;
        }
        if let Ty::Union(variants) = other {
            return variants.iter().all(|v| self.assignable_from(v));
        }
        if let Ty::Union(variants) = self {
            return variants.iter().any(|v| v.assignable_from(other));
        }
        if *self == Ty::Float && *other == Ty::Int {
            return true;
        }
        if let (Ty::List(a), Ty::List(b)) = (self, other) {
            return a.assignable_from(b);
        }
        if let (Ty::Tuple(a), Ty::Tuple(b)) = (self, other) {
            return a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(ta, tb)| ta.assignable_from(tb));
        }
        if let (Ty::Map(self_k, self_v), Ty::Map(other_k, other_v)) = (self, other) {
            return self_k.assignable_from(other_k) && self_v.assignable_from(other_v);
        }
        if let (Ty::Task(a), Ty::Task(b)) = (self, other) {
            return a.assignable_from(b);
        }
        if matches!((self, other), (Ty::SystemRef, Ty::SystemRef)) {
            return true;
        }
        if let (Ty::SumType(a), Ty::SumType(b)) = (self, other) {
            return a == b;
        }
        if let (Ty::Struct(a), Ty::Struct(b)) = (self, other) {
            return a == b;
        }
        if let (Ty::App(a_name, a_args), Ty::App(b_name, b_args)) = (self, other) {
            return a_name == b_name
                && a_args.len() == b_args.len()
                && a_args
                    .iter()
                    .zip(b_args.iter())
                    .all(|(a, b)| a.assignable_from(b));
        }
        if let (
            Ty::Fn {
                params: a_params,
                ret: a_ret,
                purity: a_purity,
            },
            Ty::Fn {
                params: b_params,
                ret: b_ret,
                purity: b_purity,
            },
        ) = (self, other)
        {
            // The argument may be at most as effectful as the parameter.
            if b_purity > a_purity {
                return false;
            }
            if a_params.len() != b_params.len() {
                return false;
            }
            for (a_p, b_p) in a_params.iter().zip(b_params.iter()) {
                if !b_p.assignable_from(a_p) {
                    return false;
                }
            }
            return a_ret.assignable_from(b_ret);
        }
        false
    }

    pub fn union(a: &Ty, b: &Ty) -> Ty {
        if *a == Ty::Void {
            return b.clone();
        }
        if *b == Ty::Void {
            return a.clone();
        }
        if a.assignable_from(b) {
            return a.clone();
        }
        if b.assignable_from(a) {
            return b.clone();
        }
        if let (Ty::App(a_name, a_args), Ty::App(b_name, b_args)) = (a, b) {
            if a_name == b_name && a_args.len() == b_args.len() {
                let merged_args = a_args
                    .iter()
                    .zip(b_args.iter())
                    .map(|(aa, ba)| Ty::union(aa, ba))
                    .collect();
                return Ty::App(a_name.clone(), merged_args);
            }
        }

        let mut variants = Vec::new();
        if let Ty::Union(a_vars) = a {
            variants.extend(a_vars.iter().cloned());
        } else {
            variants.push(a.clone());
        }
        if let Ty::Union(b_vars) = b {
            variants.extend(b_vars.iter().cloned());
        } else {
            variants.push(b.clone());
        }

        // Deduplicate
        let mut unique = Vec::new();
        for var in variants {
            if !unique.iter().any(|u| u == &var) {
                unique.push(var);
            }
        }

        if unique.len() == 1 {
            unique.pop().unwrap()
        } else {
            Ty::Union(unique)
        }
    }

    pub fn contains_var(&self, id: u32) -> bool {
        match self {
            Ty::Var(v) => *v == id,
            Ty::SystemRef => false,
            Ty::List(inner) => inner.contains_var(id),
            Ty::Map(key, val) => key.contains_var(id) || val.contains_var(id),
            Ty::Fn {
                params,
                ret,
                purity: _,
            } => params.iter().any(|p| p.contains_var(id)) || ret.contains_var(id),
            Ty::Task(inner) => inner.contains_var(id),
            Ty::App(_, args) => args.iter().any(|a| a.contains_var(id)),
            _ => false,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Float => write!(f, "float"),
            Ty::Str => write!(f, "str"),
            Ty::Bool => write!(f, "bool"),
            Ty::Nil => write!(f, "nil"),
            Ty::List(inner) => write!(f, "list<{}>", inner),
            Ty::Tuple(tys) => {
                write!(f, "(")?;
                for (i, t) in tys.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            Ty::Map(key, value) => write!(f, "map<{}, {}>", key, value),
            Ty::Component(name) => write!(f, "{}", name),
            Ty::Struct(name) => write!(f, "{}", name),
            Ty::State(name) => write!(f, "state<{}>", name),
            Ty::Event(name) => write!(f, "event<{}>", name),
            Ty::EntityId => write!(f, "entity"),
            Ty::BitSet => write!(f, "bitset"),
            Ty::WorldFork => write!(f, "world_fork"),
            Ty::SystemRef => write!(f, "system"),
            Ty::Fn {
                params,
                ret,
                purity,
            } => {
                write!(f, "{}", purity.prefix())?;
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
            Ty::Task(inner) => write!(f, "task<{}>", inner),
            Ty::SumType(name) => write!(f, "{}", name),
            Ty::Any => write!(f, "any"),
            Ty::Void => write!(f, "void"),
            Ty::Union(variants) => {
                let formatted: Vec<String> = variants.iter().map(|v| format!("{}", v)).collect();
                write!(f, "{}", formatted.join(" | "))
            }
            Ty::Var(id) => write!(f, "?T{}", id),
            Ty::App(name, args) => {
                write!(f, "{}<", name)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, ">")
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CheckerOutput {
    pub for_iter_kinds: HashMap<crate::ast::NodeId, ForIterKind>,
    pub components: HashMap<String, ComponentType>,
    pub resources: HashMap<String, ResourceType>,
    pub structs: HashMap<String, StructType>,
    pub(crate) functions: HashMap<String, crate::checker::FunctionSig>,
    pub systems: HashMap<String, SystemType>,
    pub sum_types: HashMap<String, SumTypeDef>,
    /// StateRef nodes the checker resolved as zero-field sum variant constructors.
    /// Keyed by (type_name, variant_name) so the compiler can emit MakeVariant
    /// without re-deriving the disambiguation.
    pub variant_shorthand: std::collections::HashSet<(String, String)>,
    pub spread_lengths: HashMap<crate::ast::Span, usize>,
    pub type_redirects: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ComponentType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
    pub indexed_fields: HashSet<String>,
}

impl ComponentType {
    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub struct ResourceType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
}

impl ResourceType {
    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub struct StructType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
}

impl StructType {
    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub struct StateMachineType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub states: Vec<String>,
    pub transitions: HashMap<String, Vec<(String, String)>>,
}

impl StateMachineType {
    pub fn has_state(&self, name: &str) -> bool {
        self.states.iter().any(|s| s == name)
    }
}

#[derive(Debug, Clone)]
pub struct SystemType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub params: Vec<SystemParam>,
    /// Why this system may not run under `simulate()` (IO, events, async…),
    /// if anything. `simulate()` is strict: even `rand_*` is banned because
    /// plain forks carry no explicit seed.
    pub simulation_breach: Option<String>,
    /// The lenient variant consulted by `simulate_par()`: identical, except
    /// `rand_*` builtins are permitted — every parallel fork is seeded
    /// explicitly (`fork_seed(seed, k)`), so guest randomness is
    /// deterministic and is precisely how opponent-model jitter is meant
    /// to be expressed.
    pub simulation_breach_par: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SystemParam {
    pub name: String,
    pub component_type: String,
    pub is_mut: bool,
    pub is_resource: bool,
}

#[derive(Debug, Clone)]
pub struct SumTypeDef {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub type_params: Vec<String>,
    pub variants: Vec<VariantType>,
}

#[derive(Debug, Clone)]
pub struct VariantType {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
}

#[derive(Debug, Clone)]
pub struct EventType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
}

impl EventType {
    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub struct TypeScheme {
    pub type_params: Vec<String>,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub ty: Ty,
}

#[derive(Debug, Clone)]
pub struct Substitution {
    map: HashMap<u32, Ty>,
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn bind(&mut self, id: u32, ty: Ty) {
        self.map.insert(id, ty);
    }

    pub fn lookup(&self, id: u32) -> Option<&Ty> {
        self.map.get(&id)
    }

    pub fn resolve(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(id) => {
                if let Some(bound) = self.map.get(id) {
                    self.resolve(bound)
                } else {
                    ty.clone()
                }
            }
            Ty::List(inner) => Ty::List(Box::new(self.resolve(inner))),
            Ty::Map(key, val) => Ty::Map(Box::new(self.resolve(key)), Box::new(self.resolve(val))),
            Ty::Fn {
                params,
                ret,
                purity,
            } => Ty::Fn {
                params: params.iter().map(|p| self.resolve(p)).collect(),
                ret: Box::new(self.resolve(ret)),
                purity: *purity,
            },
            Ty::Task(inner) => Ty::Task(Box::new(self.resolve(inner))),
            Ty::App(name, args) => {
                Ty::App(name.clone(), args.iter().map(|a| self.resolve(a)).collect())
            }
            _ => ty.clone(),
        }
    }

    pub fn unify(&mut self, a: &Ty, b: &Ty) -> Result<(), String> {
        let a = self.resolve(a);
        let b = self.resolve(b);

        if a == b {
            return Ok(());
        }

        match (&a, &b) {
            (Ty::Any, _) | (_, Ty::Any) => Ok(()),

            (Ty::Var(id), _) => {
                if b.contains_var(*id) {
                    return Err(format!("Infinite type: ?T{} occurs in {}", id, b));
                }
                self.bind(*id, b);
                Ok(())
            }
            (_, Ty::Var(id)) => {
                if a.contains_var(*id) {
                    return Err(format!("Infinite type: ?T{} occurs in {}", id, a));
                }
                self.bind(*id, a);
                Ok(())
            }

            (Ty::Float, Ty::Int) | (Ty::Int, Ty::Float) => Ok(()),

            (Ty::List(a_inner), Ty::List(b_inner)) => self.unify(a_inner, b_inner),
            (Ty::Map(a_key, a_val), Ty::Map(b_key, b_val)) => {
                self.unify(a_key, b_key)?;
                self.unify(a_val, b_val)
            }
            (Ty::Task(a_inner), Ty::Task(b_inner)) => self.unify(a_inner, b_inner),

            (
                Ty::Fn {
                    params: ap,
                    ret: ar,
                    purity: a_purity,
                },
                Ty::Fn {
                    params: bp,
                    ret: br,
                    purity: b_purity,
                },
            ) => {
                if b_purity > a_purity {
                    return Err(format!(
                        "Cannot unify {} function with {} function",
                        a_purity.word(),
                        b_purity.word()
                    ));
                }
                if ap.len() != bp.len() {
                    return Err(format!(
                        "Function arity mismatch: {} vs {} parameters",
                        ap.len(),
                        bp.len()
                    ));
                }
                for (pa, pb) in ap.iter().zip(bp.iter()) {
                    self.unify(pa, pb)?;
                }
                self.unify(ar, br)
            }

            (Ty::SumType(a_name), Ty::SumType(b_name)) if a_name == b_name => Ok(()),
            (Ty::Component(a_name), Ty::Component(b_name)) if a_name == b_name => Ok(()),
            (Ty::Struct(a_name), Ty::Struct(b_name)) if a_name == b_name => Ok(()),
            (Ty::State(a_name), Ty::State(b_name)) if a_name == b_name => Ok(()),

            (Ty::App(a_name, a_args), Ty::App(b_name, b_args)) => {
                if a_name != b_name {
                    return Err(format!(
                        "Type constructor mismatch: {} vs {}",
                        a_name, b_name
                    ));
                }
                if a_args.len() != b_args.len() {
                    return Err(format!(
                        "Type argument count mismatch for {}: {} vs {}",
                        a_name,
                        a_args.len(),
                        b_args.len()
                    ));
                }
                for (aa, ba) in a_args.iter().zip(b_args.iter()) {
                    self.unify(aa, ba)?;
                }
                Ok(())
            }

            _ => Err(format!("Cannot unify {} with {}", a, b)),
        }
    }
}

impl Default for Substitution {
    fn default() -> Self {
        Self::new()
    }
}
