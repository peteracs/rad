pub struct LintIssue {
    pub line: u32,
    pub col: u32,
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
}

pub struct LintPreset {
    pub description: &'static str,
    pub vm_flags: Vec<&'static str>,
    pub require_type_annotations: bool,
    pub require_pure_pipelines: bool,
    pub require_ecs_system_flow: bool,
    pub max_function_lines: usize,
    pub max_file_lines: usize,
    pub require_event_handlers: bool,
    pub no_unused_imports: bool,
    pub require_match_exhaustive: bool,
    pub require_effect_annotations: bool,
    pub naming_convention: &'static str,
    pub suggest_type_annotations: bool,
    pub suggest_pure_fn: bool,
    pub warn_complex_pipelines: bool,
    pub warn_imperative_collection_building: bool,
    pub warn_bare_print: bool,
    pub require_aliased_imports: bool,
    /// Opt-in (strict/enterprise): flag system bodies that directly read or
    /// write component/resource types absent from the system's signature.
    /// The scheduler's parallel conflict analysis only sees declared
    /// parameters, so out-of-signature accesses are invisible to it.
    pub require_system_signature_access: bool,
}
// Lexical sections preserve one private semantic namespace.
include!("linter/engine.rs");
include!("linter/tests.rs");
