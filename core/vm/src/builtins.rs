use crate::types::{Effect, EffectSet, FnPurity, Ty};
use crate::value::Builtin;
// Lexical sections preserve one private semantic namespace.
include!("builtins/value_and_world_schemes.rs");
include!("builtins/host_buffer_and_simulation_schemes.rs");
include!("builtins/identity_and_return_types.rs");
include!("builtins/api_metadata.rs");

/// Stable documentation projection of one runtime-owned builtin.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinApiEntry {
    pub name: &'static str,
    pub category: &'static str,
    pub arity: String,
    pub signature: String,
    pub type_parameters: Vec<String>,
    pub effects: String,
    pub pure: bool,
    pub errors: &'static str,
    pub determinism: &'static str,
    pub complexity: &'static str,
    pub allocation: &'static str,
    pub native: &'static str,
    pub wasm: &'static str,
    pub sandbox: &'static str,
    pub transaction: String,
    pub post_commit: String,
    pub settlement: String,
}

/// One canonical builtin argument-count contract shared by static checking,
/// runtime/embedding dispatch, diagnostics, and generated API documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinCallShape {
    Exact(usize),
    Range { min: usize, max: usize },
    Variadic { min: usize },
}

impl BuiltinCallShape {
    pub fn accepts(self, count: usize) -> bool {
        match self {
            Self::Exact(expected) => count == expected,
            Self::Range { min, max } => (min..=max).contains(&count),
            Self::Variadic { min } => count >= min,
        }
    }

    pub fn requirement(self) -> String {
        match self {
            Self::Exact(expected) => format!("exactly {expected}"),
            Self::Range { min, max } => format!("{min} to {max}"),
            Self::Variadic { min } => format!("at least {min}"),
        }
    }
}

fn derive_builtin_call_shape(builtin: Builtin) -> BuiltinCallShape {
    use Builtin::*;
    match builtin {
        Print | Eprint | Entities | Spawn => BuiltinCallShape::Variadic { min: 0 },
        Format => BuiltinCallShape::Variadic { min: 1 },
        QueryWhere | QueryMap | RequireAll => BuiltinCallShape::Variadic { min: 2 },
        QueryCount => BuiltinCallShape::Variadic { min: 1 },
        // Numeric ranges accept one through three integers. The four-argument
        // form is the ordered-index range operation:
        // `range(Component, "field", lower, upper)`.
        Range => BuiltinCallShape::Range { min: 1, max: 4 },
        Slice | Lookup => BuiltinCallShape::Range { min: 2, max: 3 },
        Input | WorldDigest => BuiltinCallShape::Range { min: 0, max: 1 },
        SandboxRun => BuiltinCallShape::Range { min: 3, max: 4 },
        SimulatePar => BuiltinCallShape::Range { min: 5, max: 6 },
        _ => BuiltinCallShape::Exact(
            builtin_type_scheme(builtin.name())
                .unwrap_or_else(|| {
                    panic!("builtin '{}' has no canonical type scheme", builtin.name())
                })
                .params
                .len(),
        ),
    }
}

static BUILTIN_CALL_SHAPES: std::sync::LazyLock<[BuiltinCallShape; Builtin::ALL.len()]> =
    std::sync::LazyLock::new(|| {
        std::array::from_fn(|index| derive_builtin_call_shape(Builtin::ALL[index]))
    });

/// Return the allocation-free argument-count contract for one builtin.
///
/// The checker derives the exact entries once from the authoritative type
/// schemes. Runtime dispatch then indexes the fixed table instead of rebuilding
/// generic `Ty` trees on every builtin call.
#[inline]
pub fn builtin_call_shape(builtin: Builtin) -> BuiltinCallShape {
    BUILTIN_CALL_SHAPES[builtin.index()]
}

pub(crate) fn initialize_builtin_call_shapes() {
    std::sync::LazyLock::force(&BUILTIN_CALL_SHAPES);
}

pub fn validate_builtin_arity(builtin: Builtin, count: usize) -> Result<(), String> {
    let shape = builtin_call_shape(builtin);
    if shape.accepts(count) {
        Ok(())
    } else {
        Err(format!(
            "{}() expects {} argument(s), got {}",
            builtin.name(),
            shape.requirement(),
            count
        ))
    }
}

/// Return every builtin exactly once in runtime catalog order.
///
/// Documentation generators consume this projection instead of maintaining a
/// second list of names, signatures, or effects.
pub fn builtin_api_catalog() -> Vec<BuiltinApiEntry> {
    Builtin::ALL
        .iter()
        .copied()
        .map(|builtin| {
            let name = builtin.name();
            let signature = builtin_signature_help(name).unwrap_or_else(|| {
                panic!("builtin '{name}' is missing its canonical API signature")
            });
            let scheme = builtin_type_scheme(name);
            let metadata = builtin_api_metadata(builtin, &signature);
            BuiltinApiEntry {
                name,
                category: metadata.category,
                arity: builtin_call_shape(builtin).requirement(),
                signature,
                type_parameters: scheme
                    .as_ref()
                    .map(|value| value.type_params.clone())
                    .unwrap_or_default(),
                effects: builtin_effect(name).to_string(),
                pure: scheme.as_ref().is_some_and(|value| value.is_pure),
                errors: metadata.errors,
                determinism: metadata.determinism,
                complexity: metadata.complexity,
                allocation: metadata.allocation,
                native: metadata.native,
                wasm: metadata.wasm,
                sandbox: metadata.sandbox,
                transaction: metadata.transaction,
                post_commit: metadata.post_commit,
                settlement: metadata.settlement,
            }
        })
        .collect()
}
