/// Canonical static signature shared by checking, runtime arity, and API docs.
pub struct BuiltinSig {
    pub type_params: Vec<String>,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub is_pure: bool,
}

impl BuiltinSig {
    pub fn effects(&self, name: &str) -> EffectSet {
        builtin_effect(name)
    }
}

/// Whether a source name belongs to the runtime-owned builtin namespace.
pub fn is_builtin(name: &str) -> bool {
    Builtin::from_name(name).is_some() || name == "emit"
}

impl Builtin {
    /// Canonical declared return type for this builtin.
    ///
    /// Signatures, checker inference, diagnostics, and generated API docs all
    /// consume `builtin_type_scheme`; keeping a second 256-arm return-type
    /// table here previously let `get_entity` drift from its runtime
    /// `entity | nil` contract to `Option`. The catalog completeness test
    /// guarantees every runtime builtin owns one scheme, so absence is an
    /// internal invariant violation rather than an `any` fallback.
    pub fn return_type(self) -> Ty {
        builtin_type_scheme(self.name())
            .unwrap_or_else(|| panic!("builtin '{}' has no canonical type scheme", self.name()))
            .ret
    }
}
