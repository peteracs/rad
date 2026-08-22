// One editor analysis entry point.
//
// Hover, go-to-definition, completion, and diagnostics each built their own
// `Checker` with the same options spelled out again. Four copies is how the
// editor starts disagreeing with itself: change one and hover explains a
// program the diagnostics never checked. Routing them through
// `rad_vm::pipeline` also keeps the editor on the parse/check flow the CLI
// compiles with, rather than a parallel one that can drift from it.

/// Checker configuration every editor analysis runs under.
///
/// The editor keeps `causal_laws` on so experimental syntax still resolves
/// while typing, and leaves `strict_types` off so an in-progress edit reports
/// its real error rather than a cascade.
fn editor_checker_options() -> CheckerOptions {
    CheckerOptions {
        features: vec!["causal_laws".to_string()],
        strict_types: false,
    }
}

/// Analyse a loaded program for diagnostics only.
///
/// Deliberately not `analyze_program`: that builds the compiler's semantic
/// product and fingerprints it, which the editor drops. Squiggles run on every
/// keystroke, so they take the path that stops at diagnostics.
fn editor_analysis(loaded: &LoadResult) -> Diagnostics {
    rad_vm::pipeline::check_program(&loaded.program, &loaded.aliases, editor_checker_options())
}

/// Analyse a loaded program and return the projection hover and completion
/// query. One checker run serves both.
fn editor_semantics(loaded: &LoadResult) -> CheckerSemanticIndex {
    rad_vm::pipeline::index_program(&loaded.program, &loaded.aliases, editor_checker_options()).1
}
