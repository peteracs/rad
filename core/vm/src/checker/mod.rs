mod causal;
mod declarations;
mod diagnostics;
mod reachability;
mod resolve;
mod scope;
mod semantic_index;
mod typeck;

pub use semantic_index::{CheckerSemanticIndex, SemanticConstraint, SemanticIntent, SemanticLaw};

#[cfg(test)]
mod tests;
use crate::ast::*;
use crate::builtins;
use crate::simulate_syntax::{self, SystemsListForm};
use crate::types::*;
use crate::visitor::{walk_call_expr, walk_schedule_stmt, AstVisitor};
use std::collections::{HashMap, HashSet};
// Checker state and lifecycle share one private implementation namespace.
include!("state.rs");
include!("lifecycle.rs");
