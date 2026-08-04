use crate::types::{Effect, EffectSet, FnPurity, Ty};
use crate::value::Builtin;
// Lexical sections preserve one private semantic namespace.
include!("builtins/value_and_world_schemes.rs");
include!("builtins/host_buffer_and_simulation_schemes.rs");
include!("builtins/identity_and_return_types.rs");
