use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;

use serde::Serialize;

mod semantic_input;
mod semantic_product;

pub use semantic_product::CheckerOutput;
pub(crate) use semantic_product::{semantic_product_fingerprint, semantic_program_fingerprint};

// Cohesive semantic domains share the public `types` namespace.
include!("types/type_system.rs");
include!("types/authority.rs");
include!("types/declarations.rs");
include!("types/substitution.rs");
