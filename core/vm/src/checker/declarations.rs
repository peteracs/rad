use super::*;
use crate::ast::*;
use crate::types::*;
use std::collections::HashMap;
// Lexical sections preserve one private semantic namespace.
include!("declarations/registration_and_effects.rs");
include!("declarations/effect_analysis.rs");
