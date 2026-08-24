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
    pub const ALL: [Effect; 5] = [
        Effect::IO,
        Effect::ECS,
        Effect::ReadECS,
        Effect::Event,
        Effect::Async,
    ];

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
        Self::ALL.into_iter().collect()
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

    /// Effects in stable diagnostic order. Builtin metadata uses this as the
    /// single bridge from its canonical [`EffectSet`] to checker diagnostics.
    pub fn effects(&self) -> Vec<Effect> {
        match self {
            EffectSet::Unrestricted => Effect::ALL.to_vec(),
            EffectSet::Restricted(set) => Effect::ALL
                .into_iter()
                .filter(|effect| set.contains(effect))
                .collect(),
        }
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
    Native {
        name: String,
        repr: crate::native_types::NativeScalarKind,
        flavor: crate::native_types::NativeTypeFlavor,
    },
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
    /// Contiguous mutable bytes. Unlike `list<int>`, one byte occupies one
    /// byte in the backing allocation and crosses WASM as `Uint8Array`.
    ByteBuf,
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
            Ty::Int | Ty::Native { .. } | Ty::Str | Ty::Bool | Ty::EntityId | Ty::Any => true,
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
        matches!(self, Ty::Int | Ty::Float | Ty::Native { .. })
    }

    /// A numeric type that also carries a nominal identity: `opaque type`,
    /// `enum`, and `bitflags` are distinct types even when they share a
    /// representation, so `DeviceId` and `SessionId` over `u32` are not
    /// interchangeable. `Scalar` natives stay ordinary numbers.
    pub fn is_nominal_numeric(&self) -> bool {
        use crate::native_types::NativeTypeFlavor;
        matches!(
            self,
            Ty::Native {
                flavor: NativeTypeFlavor::Opaque
                    | NativeTypeFlavor::Enum
                    | NativeTypeFlavor::Bitflags,
                ..
            }
        )
    }

    /// Whether an argument of type `actual` may be passed where `self` is the
    /// declared parameter type.
    ///
    /// This lived as two identical inline loops, one per call-checking path.
    /// Both skipped the check whenever the pair was numeric, which erased
    /// nominal identity in argument position: `open_device(session)` compiled
    /// with `DeviceId` and `SessionId` both `opaque ... = u32`, though the
    /// same `let` binding was rejected. Fixing one loop left the hole open in
    /// the other, so the rule now has exactly one home.
    pub fn accepts_argument(&self, actual: &Ty) -> bool {
        if *self == Ty::Any || *actual == Ty::Any {
            return true;
        }
        // Numbers convert freely, but never across a nominal boundary.
        if self.is_numeric()
            && actual.is_numeric()
            && !self.is_nominal_numeric()
            && !actual.is_nominal_numeric()
        {
            return true;
        }
        self.assignable_from(actual)
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
            Ty::Tuple(items) | Ty::Union(items) => items.iter().any(|item| item.contains_var(id)),
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
            Ty::Native { name, .. } => write!(f, "{}", name),
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
            Ty::ByteBuf => write!(f, "bytebuf"),
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
                if args.is_empty() {
                    return write!(f, "{}", name);
                }
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
