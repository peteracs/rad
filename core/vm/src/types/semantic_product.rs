use super::*;

#[derive(Debug, Clone, Default)]
pub struct CheckerOutput {
    /// Fingerprint of the AST, aliased module declarations, and complete
    /// checker configuration that produced every map below. `None` is not a
    /// checked semantic product and is rejected by `Compiler`.
    pub(crate) semantic_input_fingerprint: Option<[u8; 32]>,
    pub(crate) semantic_options: Option<crate::checker::CheckerOptions>,
    /// Integrity digest over every checker-derived map plus its semantic
    /// input identity. This detects crate-internal accidental mutation while
    /// private fields make mutation impossible through the public API.
    pub(crate) product_fingerprint: Option<[u8; 32]>,
    pub(crate) for_iter_kinds: HashMap<crate::ast::NodeId, ForIterKind>,
    pub(crate) components: HashMap<String, ComponentType>,
    pub(crate) resources: HashMap<String, ResourceType>,
    pub(crate) structs: HashMap<String, StructType>,
    pub(crate) functions: HashMap<String, crate::checker::FunctionSig>,
    pub(crate) systems: HashMap<String, SystemType>,
    pub(crate) sum_types: HashMap<String, SumTypeDef>,
    pub(crate) events: HashMap<String, EventType>,
    /// StateRef nodes the checker resolved as zero-field sum variant constructors.
    /// Keyed by (type_name, variant_name) so the compiler can emit MakeVariant
    /// without re-deriving the disambiguation.
    pub(crate) variant_shorthand: std::collections::HashSet<(String, String)>,
    pub(crate) spread_lengths: HashMap<crate::ast::Span, usize>,
    pub(crate) type_redirects: HashMap<String, String>,
    /// Fine-grained, transitive world-authority effects and the cached call
    /// graph used by `rad effects`, `writers`, `readers`, and `path`.
    pub(crate) authority: AuthorityReport,
    /// Authority violations are retained separately so every compiler entry
    /// point enforces them even when callers use the lower-level API without
    /// requesting the checker's unrelated type/style diagnostics.
    pub(crate) authority_errors: Vec<crate::checker::TypeError>,
}

impl CheckerOutput {
    pub fn authority(&self) -> &AuthorityReport {
        &self.authority
    }
}

/// Absorb one length-prefixed segment.
///
/// The length prefix is what stops two different field splits from hashing
/// identically ("ab" + "c" must not collide with "a" + "bc"). Both
/// fingerprints defined their own copy of this, so the property held only as
/// long as nobody edited one of them.
fn update_segment(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

pub(crate) fn semantic_program_fingerprint(
    program: &crate::ast::Program,
    aliases: &HashMap<String, crate::ast::ModuleAlias>,
    options: &crate::checker::CheckerOptions,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"RAD_CHECKED_PROGRAM_V3");
    let mut normalized_program = program.clone();
    super::semantic_input::normalize_program(&mut normalized_program, aliases);
    let program_debug = format!("{normalized_program:#?}");
    update_segment(&mut hasher, program_debug.as_bytes());
    let mut modules = std::collections::BTreeMap::new();
    for binding in aliases.values() {
        modules
            .entry(binding.module_identity())
            .or_insert(binding.declarations());
    }
    for (module_identity, declarations) in modules {
        update_segment(&mut hasher, module_identity.as_bytes());
        let mut declarations = declarations.to_vec();
        super::semantic_input::normalize_declarations(&mut declarations, aliases);
        let declarations = format!("{declarations:#?}");
        update_segment(&mut hasher, declarations.as_bytes());
    }
    let mut features = options.features.clone();
    features.sort_unstable();
    features.dedup();
    for feature in features {
        update_segment(&mut hasher, feature.as_bytes());
    }
    hasher.update(&[
        u8::from(options.compat_v0_5_dx),
        u8::from(options.warn_compat),
        u8::from(options.strict_types),
    ]);
    *hasher.finalize().as_bytes()
}

pub(crate) fn semantic_product_fingerprint(output: &CheckerOutput) -> [u8; 32] {
    fn update_debug_map<K: std::fmt::Debug, V: std::fmt::Debug>(
        hasher: &mut blake3::Hasher,
        values: &HashMap<K, V>,
    ) {
        let mut entries = values
            .iter()
            .map(|(key, value)| format!("{key:?}\0{value:#?}"))
            .collect::<Vec<_>>();
        entries.sort_unstable();
        for entry in entries {
            update_segment(hasher, entry.as_bytes());
        }
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"RAD_CHECKER_PRODUCT_V1");
    update_segment(
        &mut hasher,
        format!("{:?}", output.semantic_input_fingerprint).as_bytes(),
    );
    update_segment(
        &mut hasher,
        format!("{:#?}", output.semantic_options).as_bytes(),
    );
    update_debug_map(&mut hasher, &output.for_iter_kinds);
    update_debug_map(&mut hasher, &output.components);
    update_debug_map(&mut hasher, &output.resources);
    update_debug_map(&mut hasher, &output.structs);
    update_debug_map(&mut hasher, &output.functions);
    update_debug_map(&mut hasher, &output.systems);
    update_debug_map(&mut hasher, &output.sum_types);
    update_debug_map(&mut hasher, &output.events);
    let mut variants = output.variant_shorthand.iter().collect::<Vec<_>>();
    variants.sort_unstable();
    update_segment(&mut hasher, format!("{variants:#?}").as_bytes());
    let mut spreads = output
        .spread_lengths
        .iter()
        .map(|(span, length)| format!("{span:?}\0{length}"))
        .collect::<Vec<_>>();
    spreads.sort_unstable();
    update_segment(&mut hasher, format!("{spreads:#?}").as_bytes());
    update_debug_map(&mut hasher, &output.type_redirects);
    update_segment(&mut hasher, format!("{:#?}", output.authority).as_bytes());
    update_segment(
        &mut hasher,
        format!("{:#?}", output.authority_errors).as_bytes(),
    );
    *hasher.finalize().as_bytes()
}
